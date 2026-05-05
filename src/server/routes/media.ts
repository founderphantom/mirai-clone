import { Hono } from "hono";
import { all } from "../db";
import type { AppBindings } from "../env";
import { HttpError, requireUser } from "../http/errors";
import { storeUpload } from "../services/media";

export const mediaRoutes = new Hono<AppBindings>();

mediaRoutes.get("/", async (c) => {
  const user = requireUser(c);
  const rows = await all(
    c.env.DB,
    `SELECT * FROM media_assets
     WHERE user_id = ?
     ORDER BY created_at DESC
     LIMIT 100`,
    [user.id]
  );
  return c.json({ media: rows });
});

mediaRoutes.post("/upload", async (c) => {
  const user = requireUser(c);
  const form = await c.req.formData();
  const file = form.get("file");
  if (!(file instanceof File)) {
    throw new HttpError(400, "Upload requires a file field.", "missing_file");
  }

  const asset = await storeUpload(c.env, user, file, {
    cloneId: stringValue(form.get("cloneId")),
    kind: stringValue(form.get("kind")) ?? "inspiration"
  });

  return c.json({ media: asset }, 201);
});

mediaRoutes.get("/:id", async (c) => {
  const user = requireUser(c);
  const media = await all<{ storage_key: string | null; content_type: string | null; remote_url: string | null }>(
    c.env.DB,
    `SELECT storage_key, content_type, remote_url FROM media_assets WHERE id = ? AND user_id = ?`,
    [c.req.param("id"), user.id]
  );
  const asset = media[0];
  if (!asset) throw new HttpError(404, "Media asset was not found.", "media_not_found");

  if (asset.storage_key) {
    const object = await c.env.MEDIA.get(asset.storage_key);
    if (!object) throw new HttpError(404, "Media object was not found in storage.", "media_object_missing");
    const contentType = normalizeContentType(
      object.httpMetadata?.contentType || asset.content_type,
      asset.storage_key
    );
    return new Response(object.body, {
      headers: {
        "content-type": contentType,
        "cache-control": "private, max-age=300"
      }
    });
  }

  if (asset.remote_url) return c.redirect(asset.remote_url);
  throw new HttpError(404, "Media asset has no retrievable location.", "media_unavailable");
});

function stringValue(value: FormDataEntryValue | null): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function normalizeContentType(contentType: string | null | undefined, path: string): string {
  if (contentType && contentType !== "binary/octet-stream" && contentType !== "application/octet-stream") {
    return contentType;
  }
  const lowerPath = path.toLowerCase();
  if (lowerPath.endsWith(".png")) return "image/png";
  if (lowerPath.endsWith(".jpg") || lowerPath.endsWith(".jpeg")) return "image/jpeg";
  if (lowerPath.endsWith(".webp")) return "image/webp";
  return "application/octet-stream";
}
