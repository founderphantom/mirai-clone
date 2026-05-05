import { Hono } from "hono";
import { z } from "zod";
import { all, createId, first, nowIso, parseJson, run, toJson } from "../db";
import type { AppBindings } from "../env";
import { closestAspectRatio } from "../generation/aspect-ratio";
import { HttpError, readJson, requireUser } from "../http/errors";
import { getOwnedAsset, materializeDiscoveryItem } from "../services/media";

export const generationRoutes = new Hono<AppBindings>();

const createGenerationSchema = z.object({
  cloneId: z.string().min(1),
  prompt: z.string().max(2000).optional(),
  inspirationAssetId: z.string().min(1).optional(),
  discoveryItemId: z.string().min(1).optional(),
  aspectRatio: z.string().optional(),
  quality: z.enum(["1080p", "2K"]).default("1080p"),
  batchSize: z.number().int().min(1).max(4).default(4),
  mode: z.enum(["image", "video"]).default("image")
});

generationRoutes.get("/", async (c) => {
  const user = requireUser(c);
  const rows = await all(
    c.env.DB,
    `SELECT gj.*, cp.name AS clone_name,
      (SELECT COUNT(*) FROM generation_outputs go WHERE go.job_id = gj.id) AS output_count,
      (SELECT go.media_asset_id
       FROM generation_outputs go
       WHERE go.job_id = gj.id
       ORDER BY go.output_index ASC
       LIMIT 1) AS preview_media_id
     FROM generation_jobs gj
     JOIN clone_profiles cp ON cp.id = gj.clone_id
     WHERE gj.user_id = ?
     ORDER BY gj.updated_at DESC
     LIMIT 100`,
    [user.id]
  );
  return c.json({ jobs: rows });
});

generationRoutes.post("/", async (c) => {
  const user = requireUser(c);
  const parsed = createGenerationSchema.safeParse(await readJson(c));
  if (!parsed.success) throw new HttpError(400, parsed.error.message, "invalid_generation");
  if (!parsed.data.inspirationAssetId && !parsed.data.discoveryItemId) {
    throw new HttpError(400, "Choose an uploaded or discovery inspiration image.", "missing_inspiration");
  }

  const clone = await first<{ id: string; provider_config_json: string }>(
    c.env.DB,
    `SELECT id, provider_config_json FROM clone_profiles
     WHERE id = ? AND user_id = ? AND status = 'active'`,
    [parsed.data.cloneId, user.id]
  );
  if (!clone) throw new HttpError(404, "Clone profile was not found.", "clone_not_found");

  const inputAsset = parsed.data.inspirationAssetId
    ? await getOwnedAsset(c.env, user.id, parsed.data.inspirationAssetId)
    : await materializeDiscoveryItem(c.env, user, parsed.data.discoveryItemId!, parsed.data.cloneId);

  const aspectRatio =
    parsed.data.aspectRatio || closestAspectRatio(inputAsset.width, inputAsset.height);
  const jobId = createId("gen");
  const createdAt = nowIso();

  await run(
    c.env.DB,
    `INSERT INTO generation_jobs
      (id, user_id, clone_id, provider, provider_job_ids_json, status, mode, prompt,
       input_asset_id, inspiration_discovery_item_id, aspect_ratio, quality, batch_size,
       request_json, provider_payload_json, queued_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    [
      jobId,
      user.id,
      clone.id,
      "higgsfield",
      "[]",
      "queued",
      parsed.data.mode,
      parsed.data.prompt ?? "",
      inputAsset.id,
      parsed.data.discoveryItemId ?? null,
      aspectRatio,
      parsed.data.quality,
      parsed.data.batchSize,
      toJson({ ...parsed.data, providerConfig: parseJson(clone.provider_config_json, {}) }),
      "{}",
      createdAt,
      createdAt
    ]
  );

  await c.env.GENERATION_QUEUE.send({ type: "submit_generation", jobId, userId: user.id });
  return c.json({ job: await getJob(c.env.DB, user.id, jobId) }, 202);
});

generationRoutes.get("/:id", async (c) => {
  const user = requireUser(c);
  const job = await getJob(c.env.DB, user.id, c.req.param("id"));
  const outputs = await all(
    c.env.DB,
    `SELECT go.*, ma.storage_key, ma.content_type
     FROM generation_outputs go
     LEFT JOIN media_assets ma ON ma.id = go.media_asset_id
     WHERE go.job_id = ? AND go.user_id = ?
     ORDER BY go.output_index ASC`,
    [job.id, user.id]
  );
  return c.json({ job, outputs });
});

generationRoutes.post("/:id/retry", async (c) => {
  const user = requireUser(c);
  const job = await getJob(c.env.DB, user.id, c.req.param("id"));
  if (!["failed", "canceled"].includes(job.status)) {
    throw new HttpError(409, "Only failed or canceled jobs can be retried.", "job_not_retryable");
  }

  await run(
    c.env.DB,
    `UPDATE generation_jobs
     SET status = 'queued', error_message = NULL, provider_job_ids_json = '[]', updated_at = ?
     WHERE id = ? AND user_id = ?`,
    [nowIso(), job.id, user.id]
  );
  await c.env.GENERATION_QUEUE.send({ type: "submit_generation", jobId: job.id, userId: user.id });
  return c.json({ job: await getJob(c.env.DB, user.id, job.id) });
});

async function getJob(db: D1Database, userId: string, jobId: string): Promise<any> {
  const job = await first<any>(db, `SELECT * FROM generation_jobs WHERE id = ? AND user_id = ?`, [
    jobId,
    userId
  ]);
  if (!job) throw new HttpError(404, "Generation job was not found.", "job_not_found");
  return job;
}
