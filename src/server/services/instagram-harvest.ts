import { all, createId, first, nowIso, parseJson, run, toJson } from "../db";
import type { AuthUser, Env } from "../env";
import { HttpError } from "../http/errors";
import { storeRemoteReference } from "./media";
import { attachCloneReferenceAssets, createOnboardingClone, getClone } from "./onboarding-clones";

export type InstagramHarvestJob = {
  id: string;
  user_id: string;
  request_key: string;
  handle: string;
  status: string;
  candidate_count: number;
  accepted_count: number;
  fail_reason: string | null;
  clone_id: string | null;
  accepted_media_asset_ids_json: string;
  raw_json: string;
  created_at: string;
  updated_at: string;
};

export function normalizeInstagramHandle(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;

  const withoutAt = trimmed.startsWith("@") ? trimmed.slice(1) : trimmed;
  try {
    const url = new URL(withoutAt.startsWith("http") ? withoutAt : `https://${withoutAt}`);
    if (!url.hostname.includes("instagram.com")) return cleanHandle(withoutAt);
    const handle = url.pathname.split("/").filter(Boolean)[0] || "";
    return cleanHandle(handle);
  } catch {
    return cleanHandle(withoutAt);
  }
}

export async function startInstagramHarvest(env: Env, user: AuthUser, input: string) {
  const handle = normalizeInstagramHandle(input);
  if (!handle) throw new HttpError(400, "Enter a valid Instagram handle or profile URL.", "invalid_instagram_handle");

  const requestKey = `instagram:${user.id}:${handle}`;
  const existing = await first<InstagramHarvestJob>(
    env.DB,
    `SELECT * FROM instagram_harvest_jobs WHERE request_key = ?`,
    [requestKey]
  );
  if (existing) {
    await run(
      env.DB,
      `UPDATE instagram_harvest_jobs
       SET status = 'queued', candidate_count = 0, accepted_count = 0,
         fail_reason = NULL, clone_id = NULL, accepted_media_asset_ids_json = '[]',
         raw_json = '{}', updated_at = ?
       WHERE id = ? AND user_id = ?`,
      [nowIso(), existing.id, user.id]
    );
    return await getInstagramHarvestJob(env, user.id, existing.id);
  }

  const id = createId("igh");
  const createdAt = nowIso();
  await run(
    env.DB,
    `INSERT INTO instagram_harvest_jobs
      (id, user_id, request_key, handle, status, candidate_count, accepted_count,
       fail_reason, clone_id, accepted_media_asset_ids_json, raw_json, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    [id, user.id, requestKey, handle, "queued", 0, 0, null, null, "[]", "{}", createdAt, createdAt]
  );
  return await getInstagramHarvestJob(env, user.id, id);
}

export async function getInstagramHarvestJob(env: Env, userId: string, jobId: string) {
  const job = await first<InstagramHarvestJob>(
    env.DB,
    `SELECT * FROM instagram_harvest_jobs WHERE id = ? AND user_id = ?`,
    [jobId, userId]
  );
  if (!job) throw new HttpError(404, "Instagram harvest job was not found.", "harvest_not_found");
  return job;
}

export async function getLatestInstagramHarvestJob(env: Env, userId: string) {
  return await first<InstagramHarvestJob>(
    env.DB,
    `SELECT * FROM instagram_harvest_jobs WHERE user_id = ? ORDER BY updated_at DESC LIMIT 1`,
    [userId]
  );
}

export async function runInstagramHarvestJob(env: Env, jobId: string, userId: string) {
  const job = await getInstagramHarvestJob(env, userId, jobId);
  if (!["queued", "failed"].includes(job.status)) return job;

  await updateHarvest(env, job.id, userId, { status: "scraping", fail_reason: null });

  try {
    if (!env.SCRAPECREATORS_API_KEY) {
      throw new HttpError(503, "SCRAPECREATORS_API_KEY is not configured.", "scrapecreators_unconfigured");
    }

    const [profile, posts] = await Promise.all([
      fetchScrapeCreators(env, env.SCRAPECREATORS_INSTAGRAM_PROFILE_ENDPOINT || "/v1/instagram/profile", {
        handle: job.handle
      }),
      fetchScrapeCreators(env, env.SCRAPECREATORS_INSTAGRAM_POSTS_ENDPOINT || "/v2/instagram/user/posts", {
        handle: job.handle,
        trim: "true"
      })
    ]);
    const candidateUrls = extractInstagramCandidateUrls(posts).slice(0, 60);
    await updateHarvest(env, job.id, userId, {
      status: "filtering",
      candidate_count: candidateUrls.length,
      raw_json: toJson({ profile: compactProfile(profile), candidatePreview: candidateUrls.slice(0, 10) })
    });

    const user = { id: userId } satisfies AuthUser;
    const acceptedIds: string[] = [];
    for (const url of candidateUrls) {
      if (acceptedIds.length >= 12) break;
      try {
        const asset = await storeRemoteReference(env, user, {
          url,
          kind: "reference",
          source: "instagram",
          pathPrefix: `ig-harvest/${job.id}`,
          metadata: { harvestJobId: job.id, handle: job.handle, sourceUrl: url }
        });
        acceptedIds.push(asset.id);
      } catch {
        // The harvest should keep moving when a candidate image expires or fails basic checks.
      }
    }

    if (acceptedIds.length < 5) {
      await updateHarvest(env, job.id, userId, {
        status: "failed",
        accepted_count: acceptedIds.length,
        accepted_media_asset_ids_json: toJson(acceptedIds),
        fail_reason: acceptedIds.length === 0 ? "no_eligible_photos" : "not_enough_eligible_photos"
      });
      return await getInstagramHarvestJob(env, userId, job.id);
    }

    const clone = await createOnboardingClone(env, user, {
      name: `${job.handle} Soul`,
      handleBase: job.handle,
      persona: instagramPersona(job.handle, profile),
      stylePrompt: "Instagram-sourced lifestyle creator, trend-ready, identity-preserving reference set",
      source: "instagram",
      sourceSnapshot: {
        harvestJobId: job.id,
        handle: job.handle,
        acceptedMediaAssetIds: acceptedIds,
        note: "Soul Character creation pending script"
      }
    });
    await attachCloneReferenceAssets(env, userId, clone.id, acceptedIds, {
      role: "identity",
      label: "Instagram harvest"
    });

    await updateHarvest(env, job.id, userId, {
      status: "ready_for_soul_script",
      accepted_count: acceptedIds.length,
      accepted_media_asset_ids_json: toJson(acceptedIds),
      clone_id: clone.id,
      fail_reason: null
    });
    return await getInstagramHarvestJob(env, userId, job.id);
  } catch (error) {
    await updateHarvest(env, job.id, userId, {
      status: "failed",
      fail_reason: failureCode(error)
    });
    return await getInstagramHarvestJob(env, userId, job.id);
  }
}

export async function readHarvestAssets(env: Env, userId: string, job: InstagramHarvestJob) {
  const ids = parseJson<string[]>(job.accepted_media_asset_ids_json, []);
  if (ids.length === 0) return [];
  const placeholders = ids.map(() => "?").join(", ");
  return await all<any>(
    env.DB,
    `SELECT * FROM media_assets WHERE user_id = ? AND id IN (${placeholders}) ORDER BY created_at DESC`,
    [userId, ...ids]
  );
}

export async function readHarvestClone(env: Env, userId: string, job: InstagramHarvestJob) {
  return job.clone_id ? await getClone(env, userId, job.clone_id) : null;
}

export function extractInstagramCandidateUrls(body: unknown): string[] {
  const urls: string[] = [];
  visit(body, urls);
  return [...new Set(urls)].filter(isLikelyImageUrl);
}

async function fetchScrapeCreators(env: Env, endpoint: string, params: Record<string, string>) {
  const url = new URL(`https://api.scrapecreators.com${endpoint}`);
  Object.entries(params).forEach(([key, value]) => url.searchParams.set(key, value));
  const response = await fetch(url, {
    headers: {
      "x-api-key": env.SCRAPECREATORS_API_KEY || "",
      accept: "application/json"
    }
  });
  if (!response.ok) {
    throw new HttpError(502, `ScrapeCreators returned ${response.status}.`, "instagram_scrape_failed");
  }
  return await response.json();
}

async function updateHarvest(
  env: Env,
  jobId: string,
  userId: string,
  patch: Partial<InstagramHarvestJob>
) {
  const updates: string[] = [];
  const values: unknown[] = [];
  for (const [key, value] of Object.entries(patch)) {
    if (value === undefined || key === "id" || key === "user_id" || key === "request_key") continue;
    updates.push(`${key} = ?`);
    values.push(value);
  }
  updates.push("updated_at = ?");
  values.push(nowIso(), jobId, userId);
  await run(
    env.DB,
    `UPDATE instagram_harvest_jobs SET ${updates.join(", ")} WHERE id = ? AND user_id = ?`,
    values
  );
}

function instagramPersona(handle: string, profile: unknown): string {
  const record = profile && typeof profile === "object" ? (profile as Record<string, any>) : {};
  const bio = record.biography || record.bio || record.user?.biography || "";
  const fullName = record.full_name || record.fullName || record.user?.full_name || handle;
  return `Instagram creator @${handle}${fullName ? ` (${fullName})` : ""}. ${String(bio).slice(0, 500)}`;
}

function compactProfile(profile: unknown) {
  const record = profile && typeof profile === "object" ? (profile as Record<string, any>) : {};
  return {
    username: record.username || record.user?.username,
    fullName: record.full_name || record.fullName || record.user?.full_name,
    biography: record.biography || record.bio || record.user?.biography,
    followerCount: record.follower_count || record.followers || record.user?.follower_count
  };
}

function failureCode(error: unknown): string {
  if (error instanceof HttpError) return error.code;
  return "instagram_harvest_failed";
}

function cleanHandle(value: string): string | null {
  const handle = value.trim().replace(/^@/, "").split("?")[0].split("#")[0];
  if (!/^[a-zA-Z0-9._]{1,30}$/.test(handle)) return null;
  if (["p", "reel", "stories", "explore", "accounts"].includes(handle.toLowerCase())) return null;
  return handle;
}

function visit(value: unknown, urls: string[]) {
  if (!value) return;
  if (typeof value === "string") {
    if (value.startsWith("http")) urls.push(value);
    return;
  }
  if (Array.isArray(value)) {
    value.forEach((item) => visit(item, urls));
    return;
  }
  if (typeof value !== "object") return;
  for (const [key, nested] of Object.entries(value as Record<string, unknown>)) {
    if (isImageKey(key)) visit(nested, urls);
    if (typeof nested === "object") visit(nested, urls);
  }
}

function isImageKey(key: string): boolean {
  const lower = key.toLowerCase();
  return lower.includes("image") || lower.includes("thumbnail") || lower.includes("display") ||
    lower.includes("picture") || lower === "url" || lower === "src";
}

function isLikelyImageUrl(value: string): boolean {
  try {
    const url = new URL(value);
    const path = url.pathname.toLowerCase();
    if (/\.(jpg|jpeg|png|webp)(\?|$)/.test(path)) return true;
    return url.hostname.includes("cdninstagram") || url.hostname.includes("fbcdn");
  } catch {
    return false;
  }
}
