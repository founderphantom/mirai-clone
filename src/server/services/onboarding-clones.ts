import { all, createId, first, nowIso, run, slugify, toJson } from "../db";
import type { AuthUser, Env } from "../env";

export type OnboardingCloneInput = {
  name: string;
  handleBase: string;
  persona?: string;
  stylePrompt?: string;
  source: "instagram" | "manual_upload" | "starter";
  starterCharacterId?: string | null;
  providerConfig?: Record<string, unknown>;
  sourceSnapshot?: Record<string, unknown>;
  soulStatus?: "pending_script" | "ready" | "failed";
};

export async function createOnboardingClone(
  env: Env,
  user: AuthUser,
  input: OnboardingCloneInput
) {
  const id = createId("clone");
  const handle = await uniqueCloneHandle(env, user.id, input.handleBase || input.name);
  const createdAt = nowIso();
  const providerConfig = {
    ...(input.providerConfig ?? {}),
    soulCreation: {
      status: input.soulStatus ?? "pending_script",
      note: "Soul Character creation pending script"
    }
  };

  await run(
    env.DB,
    `INSERT INTO clone_profiles
      (id, user_id, name, handle, persona, voice, style_prompt, default_provider,
       provider_config_json, visibility, status, created_at, updated_at,
       starter_character_id, soul_source, soul_status, soul_character_id,
       soul_script_job_id, source_snapshot_json)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    [
      id,
      user.id,
      input.name,
      handle,
      input.persona ?? "",
      "",
      input.stylePrompt ?? "",
      "higgsfield",
      toJson(providerConfig),
      "private",
      "active",
      createdAt,
      createdAt,
      input.starterCharacterId ?? null,
      input.source,
      input.soulStatus ?? "pending_script",
      null,
      null,
      toJson(input.sourceSnapshot ?? {})
    ]
  );

  return await getClone(env, user.id, id);
}

export async function attachCloneReferenceAssets(
  env: Env,
  userId: string,
  cloneId: string,
  mediaAssetIds: string[],
  options: { role?: "identity" | "style" | "inspiration"; label?: string; weight?: number } = {}
) {
  const createdAt = nowIso();
  const uniqueIds = [...new Set(mediaAssetIds)].filter(Boolean);
  for (const mediaAssetId of uniqueIds) {
    await run(env.DB, `UPDATE media_assets SET clone_id = ? WHERE id = ? AND user_id = ?`, [
      cloneId,
      mediaAssetId,
      userId
    ]);

    const existing = await first<{ id: string }>(
      env.DB,
      `SELECT id FROM clone_reference_assets WHERE clone_id = ? AND media_asset_id = ?`,
      [cloneId, mediaAssetId]
    );
    if (existing) continue;

    await run(
      env.DB,
      `INSERT INTO clone_reference_assets
        (id, clone_id, user_id, media_asset_id, role, label, weight, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
      [
        createId("ref"),
        cloneId,
        userId,
        mediaAssetId,
        options.role ?? "identity",
        options.label ?? "",
        options.weight ?? 1,
        createdAt
      ]
    );
  }
}

export async function getClone(env: Env, userId: string, cloneId: string) {
  return await first<any>(env.DB, `SELECT * FROM clone_profiles WHERE id = ? AND user_id = ?`, [
    cloneId,
    userId
  ]);
}

async function uniqueCloneHandle(env: Env, userId: string, value: string): Promise<string> {
  const base = slugify(value) || "soul";
  const candidates = [base, ...Array.from({ length: 98 }, (_, index) => `${base}-${index + 2}`)];
  const existingRows = await all<{ handle: string }>(
    env.DB,
    `SELECT handle FROM clone_profiles WHERE user_id = ? AND handle LIKE ?`,
    [userId, `${base}%`]
  );
  const existing = new Set(existingRows.map((row) => row.handle));
  return candidates.find((candidate) => !existing.has(candidate)) ?? `${base}-${Date.now()}`;
}
