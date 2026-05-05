import { all, createId, first, nowIso, run, toJson } from "../db";
import type { Env } from "../env";
import { HttpError } from "../http/errors";
import { type DiscoverySourceConfig, getDiscoverySource } from "./sources";

type FeedRequest = {
  source: string;
  region?: string;
  query?: string;
  limit?: number;
  force?: boolean;
};

export type DiscoveryItem = {
  id: string;
  externalId: string;
  platform: string;
  mediaType: string;
  title: string;
  authorHandle: string;
  authorName: string;
  thumbnailUrl: string | null;
  imageUrl: string | null;
  sourceUrl: string | null;
  metrics: Record<string, unknown>;
  raw: Record<string, unknown>;
};

export async function getDiscoveryFeed(env: Env, request: FeedRequest) {
  const source = getDiscoverySource(request.source);
  if (!source) throw new HttpError(400, "Unknown discovery source.", "unknown_discovery_source");

  const params = resolveParams(env, source, request);
  const sourceId = sourceCacheId(source.id, params);
  const cachedSource = await first<{ expires_at: string | null; status: string }>(
    env.DB,
    `SELECT expires_at, status FROM discovery_sources WHERE id = ?`,
    [sourceId]
  );

  if (!request.force && cachedSource?.expires_at && Date.parse(cachedSource.expires_at) > Date.now()) {
    return await readCachedItems(env, sourceId, request.limit);
  }

  if (!env.SCRAPECREATORS_API_KEY) {
    if (cachedSource) return await readCachedItems(env, sourceId, request.limit);
    throw new HttpError(503, "SCRAPECREATORS_API_KEY is not configured.", "discovery_unconfigured");
  }

  const fresh = await fetchScrapeCreators(env, source, params);
  await writeCache(env, sourceId, source, params, fresh);
  return await readCachedItems(env, sourceId, request.limit);
}

function resolveParams(env: Env, source: DiscoverySourceConfig, request: FeedRequest) {
  const region = request.region || env.DISCOVERY_DEFAULT_REGION || "US";
  return {
    ...source.defaultParams,
    ...(source.id !== "youtube-shorts" ? { region } : {}),
    ...(request.query ? { query: request.query } : {})
  };
}

async function fetchScrapeCreators(
  env: Env,
  source: DiscoverySourceConfig,
  params: Record<string, string>
): Promise<DiscoveryItem[]> {
  const endpoint = resolveEndpoint(env, source);
  const url = new URL(`https://api.scrapecreators.com${endpoint}`);
  Object.entries(params).forEach(([key, value]) => url.searchParams.set(key, value));

  const response = await fetch(url, {
    headers: {
      "x-api-key": env.SCRAPECREATORS_API_KEY || "",
      accept: "application/json"
    }
  });

  if (!response.ok) {
    throw new HttpError(502, `ScrapeCreators returned ${response.status}.`, "discovery_fetch_failed");
  }

  const body = (await response.json()) as Record<string, unknown>;
  return normalizeItems(source, body);
}

function resolveEndpoint(env: Env, source: DiscoverySourceConfig): string {
  if (source.id === "tiktok-trending" && env.SCRAPECREATORS_TIKTOK_TRENDING_ENDPOINT) {
    return env.SCRAPECREATORS_TIKTOK_TRENDING_ENDPOINT;
  }
  if (source.id === "instagram-reels" && env.SCRAPECREATORS_INSTAGRAM_REELS_ENDPOINT) {
    return env.SCRAPECREATORS_INSTAGRAM_REELS_ENDPOINT;
  }
  return source.defaultEndpoint;
}

export function normalizeItems(source: DiscoverySourceConfig, body: Record<string, unknown>): DiscoveryItem[] {
  const candidates = arrayFrom(body.shorts) || arrayFrom(body.aweme_list) || arrayFrom(body.items) ||
    arrayFrom(body.videos) || arrayFrom(body.reels) || arrayFrom(body.posts) || [];

  return candidates.slice(0, 60).map((item, index) => {
    const record = item as Record<string, unknown>;
    const externalId = stringField(record, "id") || stringField(record, "aweme_id") ||
      stringField(record, "shortcode") || `${source.id}-${index}`;
    const imageUrl = pickImageUrl(record);
    const thumbnailUrl = stringField(record, "thumbnail") || stringField(record, "thumbnailUrl") || imageUrl;
    const channel = objectField(record, "channel");
    const author = objectField(record, "author") || objectField(record, "user");

    return {
      id: "",
      externalId,
      platform: source.platform,
      mediaType: imageUrl === thumbnailUrl ? "image" : "video_thumbnail",
      title: stringField(record, "title") || stringField(record, "desc") || stringField(record, "description") || "",
      authorHandle: stringField(channel, "handle") || stringField(author, "unique_id") || stringField(author, "username") || "",
      authorName: stringField(channel, "title") || stringField(author, "nickname") || stringField(author, "full_name") || "",
      thumbnailUrl,
      imageUrl,
      sourceUrl: stringField(record, "url") || stringField(record, "share_url") || stringField(record, "permalink"),
      metrics: {
        views: numberField(record, "viewCountInt") || numberField(record, "play_count"),
        likes: numberField(record, "likeCountInt") || numberField(record, "digg_count"),
        comments: numberField(record, "commentCountInt") || numberField(record, "comment_count")
      },
      raw: record
    };
  });
}

async function writeCache(
  env: Env,
  sourceId: string,
  source: DiscoverySourceConfig,
  params: Record<string, string>,
  items: DiscoveryItem[]
) {
  const ttl = Number(env.SCRAPECREATORS_CACHE_TTL_SECONDS || "1800");
  const refreshedAt = nowIso();
  const expiresAt = new Date(Date.now() + ttl * 1000).toISOString();

  await run(
    env.DB,
    `INSERT INTO discovery_sources (id, provider, source, params_json, refreshed_at, expires_at, status)
     VALUES (?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(id) DO UPDATE SET refreshed_at = excluded.refreshed_at,
       expires_at = excluded.expires_at, status = excluded.status`,
    [sourceId, "scrapecreators", source.id, toJson(params), refreshedAt, expiresAt, "ready"]
  );

  for (const item of items) {
    const itemId = createId("disc");
    await run(
      env.DB,
      `INSERT INTO discovery_items
        (id, source_id, external_id, platform, media_type, title, author_handle,
         author_name, thumbnail_url, image_url, source_url, metrics_json, raw_json,
         discovered_at, expires_at, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT(platform, external_id) DO UPDATE SET
         source_id = excluded.source_id,
         title = excluded.title,
         thumbnail_url = excluded.thumbnail_url,
         image_url = excluded.image_url,
         source_url = excluded.source_url,
         metrics_json = excluded.metrics_json,
         raw_json = excluded.raw_json,
         discovered_at = excluded.discovered_at,
         expires_at = excluded.expires_at`,
      [
        itemId,
        sourceId,
        item.externalId,
        item.platform,
        item.mediaType,
        item.title,
        item.authorHandle,
        item.authorName,
        item.thumbnailUrl,
        item.imageUrl,
        item.sourceUrl,
        toJson(item.metrics),
        toJson(item.raw),
        refreshedAt,
        expiresAt,
        refreshedAt
      ]
    );
  }
}

async function readCachedItems(env: Env, sourceId: string, limit = 48) {
  const rows = await all<any>(
    env.DB,
    `SELECT id, external_id, platform, media_type, title, author_handle, author_name,
      thumbnail_url, image_url, source_url, metrics_json, discovered_at, expires_at
     FROM discovery_items
     WHERE source_id = ?
     ORDER BY discovered_at DESC
     LIMIT ?`,
    [sourceId, Math.min(limit, 60)]
  );
  return {
    items: rows.map((row) => ({
      ...row,
      metrics: safeParse(row.metrics_json)
    }))
  };
}

function sourceCacheId(source: string, params: Record<string, string>) {
  const value = `${source}:${JSON.stringify(Object.keys(params).sort().map((key) => [key, params[key]]))}`;
  let hash = 0;
  for (let i = 0; i < value.length; i += 1) hash = (hash * 31 + value.charCodeAt(i)) >>> 0;
  return `dsrc_${source.replace(/[^a-z0-9]/g, "_")}_${hash.toString(16)}`;
}

function pickImageUrl(record: Record<string, unknown>): string | null {
  const direct = stringField(record, "image") || stringField(record, "imageUrl") ||
    stringField(record, "displayUrl") || stringField(record, "cover");
  if (direct) return direct;

  const image = objectField(record, "image") || objectField(record, "cover") || objectField(record, "video");
  const urlList = arrayFrom(image?.url_list) || arrayFrom(image?.cover?.url_list);
  const firstUrl = urlList?.find((value): value is string => typeof value === "string");
  return firstUrl || stringField(image, "url") || stringField(image, "thumbnail") || null;
}

function objectField(value: unknown, key: string): Record<string, any> | undefined {
  const next = value && typeof value === "object" ? (value as Record<string, any>)[key] : undefined;
  return next && typeof next === "object" ? next : undefined;
}

function stringField(value: unknown, key: string): string | null {
  const next = value && typeof value === "object" ? (value as Record<string, unknown>)[key] : undefined;
  return typeof next === "string" && next.length > 0 ? next : null;
}

function numberField(value: unknown, key: string): number | null {
  const next = value && typeof value === "object" ? (value as Record<string, unknown>)[key] : undefined;
  return typeof next === "number" ? next : null;
}

function arrayFrom(value: unknown): unknown[] | undefined {
  return Array.isArray(value) ? value : undefined;
}

function safeParse(value: string) {
  try {
    return JSON.parse(value);
  } catch {
    return {};
  }
}
