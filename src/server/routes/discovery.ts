import { Hono } from "hono";
import type { AppBindings } from "../env";
import { HttpError, requireUser } from "../http/errors";
import { getDiscoveryFeed } from "../discovery/scrapecreators";
import { DISCOVERY_SOURCES } from "../discovery/sources";

export const discoveryRoutes = new Hono<AppBindings>();

discoveryRoutes.get("/sources", (c) => c.json({ sources: DISCOVERY_SOURCES }));

discoveryRoutes.get("/feed", async (c) => {
  requireUser(c);
  const source = c.req.query("source") || "youtube-shorts";
  const feed = await getDiscoveryFeed(c.env, {
    source,
    region: c.req.query("region"),
    query: c.req.query("query"),
    limit: Number(c.req.query("limit") || "48")
  });
  return c.json(feed);
});

discoveryRoutes.post("/refresh", async (c) => {
  requireUser(c);
  let body: Record<string, string>;
  try {
    body = (await c.req.json()) as Record<string, string>;
  } catch {
    throw new HttpError(400, "Expected a JSON request body.", "invalid_json");
  }

  const feed = await getDiscoveryFeed(c.env, {
    source: body.source || "youtube-shorts",
    region: body.region,
    query: body.query,
    limit: Number(body.limit || "48"),
    force: true
  });
  return c.json(feed);
});
