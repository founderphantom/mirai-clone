import { Hono } from "hono";
import { z } from "zod";
import { all, first, parseJson, run } from "../db";
import type { AppBindings } from "../env";
import { HttpError, readJson, requireUser } from "../http/errors";
import type { OnboardingQueueMessage } from "../queue/messages";
import {
  getInstagramHarvestJob,
  getLatestInstagramHarvestJob,
  readHarvestAssets,
  readHarvestClone,
  startInstagramHarvest
} from "../services/instagram-harvest";
import { attachCloneReferenceAssets, createOnboardingClone } from "../services/onboarding-clones";
import { ensurePersonaBubblesForClone } from "../services/persona-agent";
import { adoptStarterCharacter, listStarterCharacters } from "../services/starter-characters";
import { storeUpload } from "../services/media";

export const onboardingRoutes = new Hono<AppBindings>();

const instagramSchema = z.object({
  instagram: z.string().min(1).max(240)
});

const starterSchema = z.object({
  starterId: z.string().min(1)
});

const bubblesSchema = z.object({
  cloneId: z.string().min(1),
  bubbleIds: z.array(z.string().min(1)).min(1).max(5)
});

onboardingRoutes.get("/state", async (c) => {
  const user = requireUser(c);
  const clones = await all<any>(
    c.env.DB,
    `SELECT cp.*,
      (SELECT COUNT(*) FROM clone_reference_assets cra WHERE cra.clone_id = cp.id) AS reference_count
     FROM clone_profiles cp
     WHERE cp.user_id = ? AND cp.status != 'archived'
     ORDER BY cp.updated_at DESC`,
    [user.id]
  );
  const activeClone = clones[0] ?? null;
  const latestHarvest = await getLatestInstagramHarvestJob(c.env, user.id);
  const bubbles = activeClone
    ? await all<any>(
        c.env.DB,
        `SELECT * FROM inspiration_bubbles WHERE user_id = ? AND clone_id = ? ORDER BY sort ASC`,
        [user.id, activeClone.id]
      )
    : [];
  const pool = await first<{ count: number }>(
    c.env.DB,
    `SELECT COUNT(*) AS count FROM user_inspiration_pool WHERE user_id = ?`,
    [user.id]
  );

  return c.json({
    clones,
    activeClone,
    latestHarvest,
    bubbles: bubbles.map(formatBubble),
    inspirationPoolCount: pool?.count ?? 0,
    starters: await listStarterCharacters(c.env)
  });
});

onboardingRoutes.get("/starters", async (c) => {
  requireUser(c);
  return c.json({ starters: await listStarterCharacters(c.env) });
});

onboardingRoutes.post("/instagram", async (c) => {
  const user = requireUser(c);
  const parsed = instagramSchema.safeParse(await readJson(c));
  if (!parsed.success) throw new HttpError(400, parsed.error.message, "invalid_instagram_source");

  const job = await startInstagramHarvest(c.env, user, parsed.data.instagram);
  await enqueueOnboarding(c.env, { type: "run_instagram_harvest", jobId: job.id, userId: user.id });
  return c.json({ job }, 202);
});

onboardingRoutes.get("/harvest/:id", async (c) => {
  const user = requireUser(c);
  const job = await getInstagramHarvestJob(c.env, user.id, c.req.param("id"));
  return c.json({
    job,
    acceptedAssets: await readHarvestAssets(c.env, user.id, job),
    clone: await readHarvestClone(c.env, user.id, job)
  });
});

onboardingRoutes.post("/upload", async (c) => {
  const user = requireUser(c);
  const form = await c.req.formData();
  const files = uploadedFiles(form);
  if (files.length < 5 || files.length > 15) {
    throw new HttpError(400, "Upload 5 to 15 clear photos for your Soul.", "invalid_reference_count");
  }
  for (const file of files) validateReferenceUpload(file);

  const clone = await createOnboardingClone(c.env, user, {
    name: stringField(form, "name") || "My Soul",
    handleBase: stringField(form, "handle") || stringField(form, "name") || "my-soul",
    persona: stringField(form, "persona") || "Manual upload Soul created from user-provided reference photos.",
    stylePrompt: stringField(form, "stylePrompt") || "identity reference set, creator lifestyle, trend-ready",
    source: "manual_upload",
    sourceSnapshot: {
      uploadCount: files.length,
      originalNames: files.map((file) => file.name),
      note: "Soul Character creation pending script"
    }
  });

  const assets = [];
  for (const file of files) {
    assets.push(
      await storeUpload(c.env, user, file, {
        cloneId: clone.id,
        kind: "reference",
        source: "onboarding_upload"
      })
    );
  }
  await attachCloneReferenceAssets(
    c.env,
    user.id,
    clone.id,
    assets.map((asset) => asset.id),
    { role: "identity", label: "Manual upload" }
  );
  await enqueueOnboarding(c.env, { type: "analyze_persona", cloneId: clone.id, userId: user.id });

  return c.json({ clone, referenceAssets: assets }, 201);
});

onboardingRoutes.post("/starter", async (c) => {
  const user = requireUser(c);
  const parsed = starterSchema.safeParse(await readJson(c));
  if (!parsed.success) throw new HttpError(400, parsed.error.message, "invalid_starter");

  const result = await adoptStarterCharacter(c.env, user, parsed.data.starterId);
  await enqueueOnboarding(c.env, { type: "analyze_persona", cloneId: result.clone.id, userId: user.id });
  return c.json(result, 201);
});

onboardingRoutes.post("/bubbles/generate", async (c) => {
  const user = requireUser(c);
  const parsed = z.object({ cloneId: z.string().min(1) }).safeParse(await readJson(c));
  if (!parsed.success) throw new HttpError(400, parsed.error.message, "invalid_bubble_request");
  return c.json({ bubbles: (await ensurePersonaBubblesForClone(c.env, user.id, parsed.data.cloneId)).map(formatBubble) });
});

onboardingRoutes.post("/bubbles", async (c) => {
  const user = requireUser(c);
  const parsed = bubblesSchema.safeParse(await readJson(c));
  if (!parsed.success) throw new HttpError(400, parsed.error.message, "invalid_bubbles");

  await run(
    c.env.DB,
    `UPDATE inspiration_bubbles SET selected = 0 WHERE user_id = ? AND clone_id = ?`,
    [user.id, parsed.data.cloneId]
  );
  const placeholders = parsed.data.bubbleIds.map(() => "?").join(", ");
  await run(
    c.env.DB,
    `UPDATE inspiration_bubbles
     SET selected = 1
     WHERE user_id = ? AND clone_id = ? AND id IN (${placeholders})`,
    [user.id, parsed.data.cloneId, ...parsed.data.bubbleIds]
  );

  const selected = await all<any>(
    c.env.DB,
    `SELECT * FROM inspiration_bubbles WHERE user_id = ? AND clone_id = ? AND selected = 1 ORDER BY sort ASC`,
    [user.id, parsed.data.cloneId]
  );
  if (selected.length === 0) throw new HttpError(404, "No matching inspiration bubbles were found.", "bubbles_not_found");

  await enqueueOnboarding(c.env, {
    type: "seed_inspiration_pool",
    cloneId: parsed.data.cloneId,
    userId: user.id,
    bubbleIds: selected.map((bubble) => bubble.id)
  });

  return c.json({
    bubbles: selected.map(formatBubble),
    seedQueued: true
  });
});

async function enqueueOnboarding(env: AppBindings["Bindings"], message: OnboardingQueueMessage) {
  await env.ONBOARDING_QUEUE.send(message);
}

function uploadedFiles(form: FormData): File[] {
  const entries = [
    ...form.getAll("photos"),
    ...form.getAll("files"),
    ...form.getAll("file")
  ];
  return entries.filter((entry): entry is File => entry instanceof File);
}

function validateReferenceUpload(file: File) {
  if (!file.type.startsWith("image/")) {
    throw new HttpError(400, "Reference uploads must be images.", "invalid_media");
  }
  if (file.size > 15 * 1024 * 1024) {
    throw new HttpError(413, "Reference uploads must be 15 MB or smaller.", "media_too_large");
  }
}

function stringField(form: FormData, key: string): string | null {
  const value = form.get(key);
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function formatBubble(row: any) {
  return {
    ...row,
    searchQueries: parseJson<string[]>(row.search_queries_json, [])
  };
}
