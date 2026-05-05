import { Hono } from "hono";
import { z } from "zod";
import { all, createId, first, nowIso, run, slugify, toJson } from "../db";
import type { AppBindings } from "../env";
import { HttpError, readJson, requireUser } from "../http/errors";

export const cloneRoutes = new Hono<AppBindings>();

const createCloneSchema = z.object({
  name: z.string().min(1).max(80),
  handle: z.string().min(1).max(48).optional(),
  persona: z.string().max(2000).optional(),
  voice: z.string().max(2000).optional(),
  stylePrompt: z.string().max(2000).optional(),
  providerConfig: z.record(z.string(), z.unknown()).optional()
});

const updateCloneSchema = createCloneSchema.partial().extend({
  status: z.enum(["active", "archived"]).optional()
});

cloneRoutes.get("/", async (c) => {
  const user = requireUser(c);
  const clones = await all(
    c.env.DB,
    `SELECT cp.*,
      (SELECT COUNT(*) FROM clone_reference_assets cra WHERE cra.clone_id = cp.id) AS reference_count,
      (SELECT COUNT(*) FROM generation_jobs gj WHERE gj.clone_id = cp.id) AS generation_count
     FROM clone_profiles cp
     WHERE cp.user_id = ? AND cp.status != 'archived'
     ORDER BY cp.updated_at DESC`,
    [user.id]
  );
  return c.json({ clones });
});

cloneRoutes.post("/", async (c) => {
  const user = requireUser(c);
  const parsed = createCloneSchema.safeParse(await readJson(c));
  if (!parsed.success) throw new HttpError(400, parsed.error.message, "invalid_clone");

  const id = createId("clone");
  const handle = slugify(parsed.data.handle || parsed.data.name);
  const existing = await first<{ id: string }>(
    c.env.DB,
    `SELECT id FROM clone_profiles WHERE user_id = ? AND handle = ?`,
    [user.id, handle]
  );
  if (existing) throw new HttpError(409, "You already have a clone with that handle.", "handle_taken");

  const createdAt = nowIso();
  await run(
    c.env.DB,
    `INSERT INTO clone_profiles
      (id, user_id, name, handle, persona, voice, style_prompt, default_provider,
       provider_config_json, visibility, status, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    [
      id,
      user.id,
      parsed.data.name,
      handle,
      parsed.data.persona ?? "",
      parsed.data.voice ?? "",
      parsed.data.stylePrompt ?? "",
      "higgsfield",
      toJson(parsed.data.providerConfig ?? {}),
      "private",
      "active",
      createdAt,
      createdAt
    ]
  );

  return c.json({ clone: await getClone(c, user.id, id) }, 201);
});

cloneRoutes.get("/:id", async (c) => {
  const user = requireUser(c);
  const clone = await getClone(c, user.id, c.req.param("id"));
  const referenceAssets = await all(
    c.env.DB,
    `SELECT cra.*, ma.storage_key, ma.remote_url, ma.content_type
     FROM clone_reference_assets cra
     JOIN media_assets ma ON ma.id = cra.media_asset_id
     WHERE cra.clone_id = ? AND cra.user_id = ?
     ORDER BY cra.created_at DESC`,
    [clone.id, user.id]
  );
  return c.json({ clone, referenceAssets });
});

cloneRoutes.patch("/:id", async (c) => {
  const user = requireUser(c);
  const id = c.req.param("id");
  await getClone(c, user.id, id);

  const parsed = updateCloneSchema.safeParse(await readJson(c));
  if (!parsed.success) throw new HttpError(400, parsed.error.message, "invalid_clone");

  const updates: string[] = [];
  const values: unknown[] = [];
  addUpdate(updates, values, "name", parsed.data.name);
  addUpdate(updates, values, "persona", parsed.data.persona);
  addUpdate(updates, values, "voice", parsed.data.voice);
  addUpdate(updates, values, "style_prompt", parsed.data.stylePrompt);
  addUpdate(updates, values, "status", parsed.data.status);
  if (parsed.data.providerConfig) {
    addUpdate(updates, values, "provider_config_json", toJson(parsed.data.providerConfig));
  }
  if (parsed.data.handle) {
    addUpdate(updates, values, "handle", slugify(parsed.data.handle));
  }

  if (updates.length === 0) return c.json({ clone: await getClone(c, user.id, id) });

  updates.push("updated_at = ?");
  values.push(nowIso(), id, user.id);
  await run(
    c.env.DB,
    `UPDATE clone_profiles SET ${updates.join(", ")} WHERE id = ? AND user_id = ?`,
    values
  );

  return c.json({ clone: await getClone(c, user.id, id) });
});

cloneRoutes.delete("/:id", async (c) => {
  const user = requireUser(c);
  const id = c.req.param("id");
  await getClone(c, user.id, id);
  await run(
    c.env.DB,
    `UPDATE clone_profiles SET status = 'archived', updated_at = ? WHERE id = ? AND user_id = ?`,
    [nowIso(), id, user.id]
  );
  return c.json({ ok: true });
});

cloneRoutes.post("/:id/reference-assets", async (c) => {
  const user = requireUser(c);
  const clone = await getClone(c, user.id, c.req.param("id"));
  const parsed = z
    .object({
      mediaAssetId: z.string().min(1),
      role: z.enum(["identity", "style", "inspiration"]).default("style"),
      label: z.string().max(120).optional(),
      weight: z.number().min(0).max(2).default(1)
    })
    .safeParse(await readJson(c));
  if (!parsed.success) throw new HttpError(400, parsed.error.message, "invalid_reference_asset");

  const asset = await first<{ id: string }>(
    c.env.DB,
    `SELECT id FROM media_assets WHERE id = ? AND user_id = ?`,
    [parsed.data.mediaAssetId, user.id]
  );
  if (!asset) throw new HttpError(404, "Media asset was not found.", "media_not_found");

  const id = createId("ref");
  await run(
    c.env.DB,
    `INSERT INTO clone_reference_assets
      (id, clone_id, user_id, media_asset_id, role, label, weight, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    [
      id,
      clone.id,
      user.id,
      parsed.data.mediaAssetId,
      parsed.data.role,
      parsed.data.label ?? "",
      parsed.data.weight,
      nowIso()
    ]
  );

  return c.json({ referenceAssetId: id }, 201);
});

async function getClone(c: any, userId: string, id: string) {
  const clone = await first<any>(c.env.DB, `SELECT * FROM clone_profiles WHERE id = ? AND user_id = ?`, [
    id,
    userId
  ]);
  if (!clone) throw new HttpError(404, "Clone profile was not found.", "clone_not_found");
  return clone;
}

function addUpdate(updates: string[], values: unknown[], column: string, value: unknown) {
  if (value === undefined) return;
  updates.push(`${column} = ?`);
  values.push(value);
}
