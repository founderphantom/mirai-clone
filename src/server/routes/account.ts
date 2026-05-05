import { Hono } from "hono";
import { all } from "../db";
import type { AppBindings } from "../env";
import { requireUser } from "../http/errors";

export const accountRoutes = new Hono<AppBindings>();

accountRoutes.get("/", async (c) => {
  const user = requireUser(c);
  const recentBillingEvents = await all(
    c.env.DB,
    `SELECT event_type, polar_customer_id, polar_subscription_id, polar_product_id, created_at
     FROM billing_events
     WHERE user_id = ?
     ORDER BY created_at DESC
     LIMIT 20`,
    [user.id]
  );

  return c.json({
    user,
    billing: {
      polarEnabled: Boolean(c.env.POLAR_ACCESS_TOKEN),
      checkoutEnabled: Boolean(c.env.POLAR_ACCESS_TOKEN && c.env.POLAR_PRO_PRODUCT_ID),
      portalEnabled: Boolean(c.env.POLAR_ACCESS_TOKEN),
      server: c.env.POLAR_SERVER || "sandbox",
      recentEvents: recentBillingEvents
    }
  });
});

accountRoutes.get("/usage", async (c) => {
  const user = requireUser(c);
  const [cloneCounts, generationCounts, mediaCounts] = await Promise.all([
    all(c.env.DB, `SELECT status, COUNT(*) AS count FROM clone_profiles WHERE user_id = ? GROUP BY status`, [
      user.id
    ]),
    all(c.env.DB, `SELECT status, COUNT(*) AS count FROM generation_jobs WHERE user_id = ? GROUP BY status`, [
      user.id
    ]),
    all(c.env.DB, `SELECT kind, COUNT(*) AS count FROM media_assets WHERE user_id = ? GROUP BY kind`, [
      user.id
    ])
  ]);

  return c.json({ clones: cloneCounts, generations: generationCounts, media: mediaCounts });
});
