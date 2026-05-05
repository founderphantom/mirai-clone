import { createId, first, nowIso, parseJson, run, safeR2Segment, toJson } from "../db";
import type { Env } from "../env";
import { HiggsfieldProvider } from "../generation/higgsfield";
import type { CloneProviderConfig, GenerationSubmission, ProviderOutput } from "../generation/provider";
import type { MediaAssetRow } from "../services/media";
import type { GenerationQueueMessage } from "./messages";

type GenerationJobRow = {
  id: string;
  user_id: string;
  clone_id: string;
  provider: string;
  provider_job_ids_json: string;
  status: string;
  mode: "image" | "video";
  prompt: string;
  input_asset_id: string;
  aspect_ratio: string | null;
  quality: string;
  batch_size: number;
  request_json: string;
};

type CloneRow = {
  id: string;
  provider_config_json: string;
};

export async function handleGenerationBatch(
  batch: MessageBatch<GenerationQueueMessage>,
  env: Env
): Promise<void> {
  for (const message of batch.messages) {
    try {
      await handleMessage(message.body, env);
      message.ack();
    } catch (error) {
      console.error("generation queue failed", error);
      message.retry({ delaySeconds: retryDelay(message.attempts) });
    }
  }
}

async function handleMessage(message: GenerationQueueMessage, env: Env) {
  if (message.type === "submit_generation") {
    await submitGeneration(env, message.jobId, message.userId);
    return;
  }
  await pollGeneration(env, message.jobId, message.userId, message.providerJobIds, message.attempt ?? 0);
}

async function submitGeneration(env: Env, jobId: string, userId: string) {
  const job = await getJob(env, jobId, userId);
  if (job.status !== "queued") return;

  const clone = await first<CloneRow>(env.DB, `SELECT id, provider_config_json FROM clone_profiles WHERE id = ?`, [
    job.clone_id
  ]);
  const asset = await first<MediaAssetRow>(env.DB, `SELECT * FROM media_assets WHERE id = ?`, [
    job.input_asset_id
  ]);
  if (!clone || !asset) throw new Error("Generation job is missing clone or input asset.");

  await run(
    env.DB,
    `UPDATE generation_jobs SET status = 'processing', started_at = ?, updated_at = ? WHERE id = ?`,
    [nowIso(), nowIso(), jobId]
  );

  const provider = new HiggsfieldProvider(env, env.MEDIA);
  const submission = toSubmission(job, clone, asset);
  const result = await provider.submit(submission);

  await run(
    env.DB,
    `UPDATE generation_jobs
     SET provider_job_ids_json = ?, provider_payload_json = ?, updated_at = ?
     WHERE id = ?`,
    [toJson(result.providerJobIds), toJson(result.providerPayload), nowIso(), jobId]
  );

  await env.GENERATION_QUEUE.send(
    {
      type: "poll_generation",
      jobId,
      userId,
      providerJobIds: result.providerJobIds,
      attempt: 0
    },
    { delaySeconds: 45 }
  );
}

async function pollGeneration(
  env: Env,
  jobId: string,
  userId: string,
  providerJobIds: string[],
  attempt: number
) {
  const job = await getJob(env, jobId, userId);
  if (job.status !== "processing") return;

  const provider = new HiggsfieldProvider(env, env.MEDIA);
  const result = await provider.poll(providerJobIds);

  if (result.status === "processing") {
    await env.GENERATION_QUEUE.send(
      {
        type: "poll_generation",
        jobId,
        userId,
        providerJobIds: result.providerJobIds,
        attempt: attempt + 1
      },
      { delaySeconds: Math.min(120, 45 + attempt * 15) }
    );
    return;
  }

  if (result.status === "failed") {
    await markFailed(env, jobId, result.errorMessage);
    return;
  }

  await persistOutputs(env, job, result.outputs);
  await run(
    env.DB,
    `UPDATE generation_jobs
     SET status = 'completed', completed_at = ?, updated_at = ?
     WHERE id = ?`,
    [nowIso(), nowIso(), jobId]
  );
}

async function persistOutputs(env: Env, job: GenerationJobRow, outputs: ProviderOutput[]) {
  let index = 0;
  for (const output of outputs) {
    index += 1;
    const response = await fetch(output.rawUrl);
    if (!response.ok) throw new Error(`Could not fetch generated output ${output.providerAssetId}.`);

    const bytes = await response.arrayBuffer();
    const contentType = normalizeImageContentType(
      response.headers.get("content-type") || output.contentType,
      output.rawUrl
    );
    const extension = contentType.includes("jpeg") || contentType.includes("jpg") ? "jpg" : "png";
    const mediaId = createId("media");
    const storageKey = [
      "users",
      safeR2Segment(job.user_id),
      "clones",
      safeR2Segment(job.clone_id),
      "generations",
      safeR2Segment(job.id),
      `${index}.${extension}`
    ].join("/");

    await env.MEDIA.put(storageKey, bytes, {
      httpMetadata: { contentType },
      customMetadata: { userId: job.user_id, cloneId: job.clone_id, jobId: job.id }
    });

    await run(
      env.DB,
      `INSERT INTO media_assets
        (id, user_id, clone_id, kind, source, storage_key, content_type, bytes,
         width, height, remote_url, sha256, metadata_json, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      [
        mediaId,
        job.user_id,
        job.clone_id,
        "generated",
        "provider",
        storageKey,
        contentType,
        bytes.byteLength,
        null,
        null,
        output.rawUrl,
        null,
        toJson({ provider: job.provider, providerAssetId: output.providerAssetId }),
        nowIso()
      ]
    );

    await run(
      env.DB,
      `INSERT INTO generation_outputs
        (id, job_id, user_id, clone_id, provider_asset_id, media_asset_id,
         share_url, raw_url, output_index, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      [
        createId("out"),
        job.id,
        job.user_id,
        job.clone_id,
        output.providerAssetId,
        mediaId,
        output.shareUrl ?? null,
        output.rawUrl,
        index,
        nowIso()
      ]
    );
  }
}

function toSubmission(job: GenerationJobRow, clone: CloneRow, asset: MediaAssetRow): GenerationSubmission {
  return {
    jobId: job.id,
    userId: job.user_id,
    cloneId: job.clone_id,
    prompt: job.prompt,
    aspectRatio: job.aspect_ratio || "3:4",
    quality: job.quality,
    batchSize: job.batch_size,
    mode: job.mode,
    inputAsset: {
      id: asset.id,
      storageKey: asset.storage_key,
      remoteUrl: asset.remote_url,
      contentType: asset.content_type
    },
    cloneConfig: parseJson<CloneProviderConfig>(clone.provider_config_json, {})
  };
}

async function getJob(env: Env, jobId: string, userId: string): Promise<GenerationJobRow> {
  const job = await first<GenerationJobRow>(
    env.DB,
    `SELECT * FROM generation_jobs WHERE id = ? AND user_id = ?`,
    [jobId, userId]
  );
  if (!job) throw new Error(`Generation job ${jobId} was not found.`);
  return job;
}

async function markFailed(env: Env, jobId: string, message: string) {
  await run(
    env.DB,
    `UPDATE generation_jobs
     SET status = 'failed', error_message = ?, completed_at = ?, updated_at = ?
     WHERE id = ?`,
    [message, nowIso(), nowIso(), jobId]
  );
}

function retryDelay(attempts: number): number {
  return Math.min(300, 30 * Math.max(1, attempts));
}

function normalizeImageContentType(contentType: string | null | undefined, url: string): string {
  if (contentType && contentType !== "binary/octet-stream" && contentType !== "application/octet-stream") {
    return contentType;
  }
  const lowerUrl = url.toLowerCase();
  if (lowerUrl.includes(".jpg") || lowerUrl.includes(".jpeg")) return "image/jpeg";
  if (lowerUrl.includes(".webp")) return "image/webp";
  return "image/png";
}
