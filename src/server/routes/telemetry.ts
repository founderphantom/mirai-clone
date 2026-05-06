import { Hono } from "hono";
import { z } from "zod";
import { all, createId, nowIso, run, toJson } from "../db";
import type { AppBindings } from "../env";
import { HttpError, readJson } from "../http/errors";

export const telemetryRoutes = new Hono<AppBindings>();

const eventsSchema = z.object({
  events: z
    .array(
      z.object({
        event: z.string().min(1).max(120),
        props: z.record(z.string(), z.unknown()).default({})
      })
    )
    .min(1)
    .max(20)
});

telemetryRoutes.get("/config", async (c) => {
  const user = c.get("user");
  const overrides = user
    ? await all<{ key: string; value: string }>(
        c.env.DB,
        `SELECT key, value FROM feature_flag_overrides WHERE user_id = ?`,
        [user.id]
      ).catch(() => [])
    : [];

  const flags = {
    mobileShell: true,
    onboardingInstagram: true,
    onboardingStarterSouls: true,
    blitzPreview: true,
    contextualPaywalls: false,
    ...Object.fromEntries(overrides.map((row) => [row.key, parseFlagValue(row.value)]))
  };

  return c.json({ flags });
});

telemetryRoutes.post("/events", async (c) => {
  const parsed = eventsSchema.safeParse(await readJson(c));
  if (!parsed.success) throw new HttpError(400, parsed.error.message, "invalid_telemetry");

  const user = c.get("user");
  const createdAt = nowIso();
  for (const event of parsed.data.events) {
    await run(
      c.env.DB,
      `INSERT INTO app_events (id, user_id, event, props_json, created_at)
       VALUES (?, ?, ?, ?, ?)`,
      [createId("evt"), user?.id ?? null, event.event, toJson(event.props), createdAt]
    ).catch(() => undefined);
  }

  return c.json({ ok: true });
});

function parseFlagValue(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    if (value === "true") return true;
    if (value === "false") return false;
    return value;
  }
}
