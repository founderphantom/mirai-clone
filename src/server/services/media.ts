import { createId, first, nowIso, run, safeR2Segment, sha256Hex, toJson } from "../db";
import type { AuthUser, Env } from "../env";
import { HttpError } from "../http/errors";

export type MediaAssetRow = {
  id: string;
  user_id: string;
  clone_id: string | null;
  kind: string;
  source: string;
  storage_key: string | null;
  content_type: string | null;
  bytes: number | null;
  width: number | null;
  height: number | null;
  remote_url: string | null;
  sha256: string | null;
  metadata_json: string;
  created_at: string;
};

export type DiscoveryItemRow = {
  id: string;
  image_url: string | null;
  thumbnail_url: string | null;
  title: string;
  source_url: string | null;
  raw_json: string;
};

export async function storeUpload(
  env: Env,
  user: AuthUser,
  file: File,
  options: { cloneId?: string | null; kind?: string; source?: string }
): Promise<MediaAssetRow> {
  if (!file.type.startsWith("image/")) {
    throw new HttpError(400, "Only image uploads are supported for inspiration assets.", "invalid_media");
  }

  const bytes = await file.arrayBuffer();
  if (bytes.byteLength > 15 * 1024 * 1024) {
    throw new HttpError(413, "Image uploads must be 15 MB or smaller.", "media_too_large");
  }

  const id = createId("media");
  const digest = await sha256Hex(bytes);
  const extension = file.type.includes("png") ? "png" : file.type.includes("webp") ? "webp" : "jpg";
  const storageKey = [
    "users",
    safeR2Segment(user.id),
    "uploads",
    `${id}-${safeR2Segment(file.name || "inspiration")}.${extension}`
  ].join("/");

  await env.MEDIA.put(storageKey, bytes, {
    httpMetadata: { contentType: file.type },
    customMetadata: { userId: user.id, mediaId: id }
  });

  const createdAt = nowIso();
  await run(
    env.DB,
    `INSERT INTO media_assets
      (id, user_id, clone_id, kind, source, storage_key, content_type, bytes,
       width, height, remote_url, sha256, metadata_json, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    [
      id,
      user.id,
      options.cloneId ?? null,
      options.kind ?? "inspiration",
      options.source ?? "upload",
      storageKey,
      file.type,
      bytes.byteLength,
      null,
      null,
      null,
      digest,
      toJson({ originalName: file.name }),
      createdAt
    ]
  );

  return mustGetAsset(env, user.id, id);
}

export async function materializeDiscoveryItem(
  env: Env,
  user: AuthUser,
  discoveryItemId: string,
  cloneId?: string | null
): Promise<MediaAssetRow> {
  const item = await first<DiscoveryItemRow>(
    env.DB,
    `SELECT id, image_url, thumbnail_url, title, source_url, raw_json
     FROM discovery_items
     WHERE id = ?`,
    [discoveryItemId]
  );
  if (!item) throw new HttpError(404, "Discovery item was not found.", "discovery_not_found");

  const remoteUrl = item.image_url || item.thumbnail_url;
  if (!remoteUrl) throw new HttpError(422, "Discovery item has no image URL.", "missing_image");

  const response = await fetch(remoteUrl);
  if (!response.ok) {
    throw new HttpError(502, "Could not fetch the selected discovery image.", "image_fetch_failed");
  }

  const contentType = response.headers.get("content-type") || "image/jpeg";
  const bytes = await response.arrayBuffer();
  const id = createId("media");
  const digest = await sha256Hex(bytes);
  const extension = contentType.includes("png") ? "png" : contentType.includes("webp") ? "webp" : "jpg";
  const storageKey = [
    "users",
    safeR2Segment(user.id),
    "discovery",
    `${id}.${extension}`
  ].join("/");

  await env.MEDIA.put(storageKey, bytes, {
    httpMetadata: { contentType },
    customMetadata: { userId: user.id, mediaId: id, discoveryItemId }
  });

  const createdAt = nowIso();
  await run(
    env.DB,
    `INSERT INTO media_assets
      (id, user_id, clone_id, kind, source, storage_key, content_type, bytes,
       width, height, remote_url, sha256, metadata_json, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    [
      id,
      user.id,
      cloneId ?? null,
      "inspiration",
      "discovery",
      storageKey,
      contentType,
      bytes.byteLength,
      null,
      null,
      remoteUrl,
      digest,
      toJson({ discoveryItemId, sourceUrl: item.source_url, title: item.title }),
      createdAt
    ]
  );

  return mustGetAsset(env, user.id, id);
}

export async function mustGetAsset(env: Env, userId: string, assetId: string): Promise<MediaAssetRow> {
  const asset = await first<MediaAssetRow>(
    env.DB,
    `SELECT * FROM media_assets WHERE id = ? AND user_id = ?`,
    [assetId, userId]
  );
  if (!asset) throw new HttpError(404, "Media asset was not found.", "media_not_found");
  return asset;
}

export async function getOwnedAsset(env: Env, userId: string, assetId: string): Promise<MediaAssetRow> {
  return await mustGetAsset(env, userId, assetId);
}
