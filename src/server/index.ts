import { Hono } from "hono";
import { cors } from "hono/cors";
import { createAuth } from "./auth";
import type { AppBindings, AuthSession, AuthUser, Env } from "./env";
import { errorResponse } from "./http/errors";
import { accountRoutes } from "./routes/account";
import { cloneRoutes } from "./routes/clones";
import { discoveryRoutes } from "./routes/discovery";
import { generationRoutes } from "./routes/generations";
import { mediaRoutes } from "./routes/media";
import { onboardingRoutes } from "./routes/onboarding";
import { telemetryRoutes } from "./routes/telemetry";
import { handleGenerationBatch } from "./queue/generation-consumer";
import { handleOnboardingBatch } from "./queue/onboarding-consumer";
import { isOnboardingQueueMessage, type AppQueueMessage, type GenerationQueueMessage, type OnboardingQueueMessage } from "./queue/messages";

const app = new Hono<AppBindings>();

app.use(
  "/api/*",
  cors({
    origin: (origin) => origin || "*",
    allowHeaders: ["Content-Type", "Authorization"],
    allowMethods: ["GET", "POST", "PATCH", "DELETE", "OPTIONS"],
    exposeHeaders: ["Content-Length"],
    credentials: true,
    maxAge: 600
  })
);

app.use("*", async (c, next) => {
  const auth = createAuth(c.env, new URL(c.req.url).origin);
  c.set("auth", auth);

  if (c.req.path.startsWith("/api/auth") || c.req.path.startsWith("/polar/webhooks")) {
    c.set("user", null);
    c.set("session", null);
    await next();
    return;
  }

  try {
    const session = await auth.api.getSession({ headers: c.req.raw.headers });
    c.set("user", (session?.user ?? null) as AuthUser | null);
    c.set("session", (session?.session ?? null) as AuthSession | null);
  } catch {
    c.set("user", null);
    c.set("session", null);
  }
  await next();
});

app.get("/api/health", (c) =>
  c.json({
    ok: true,
    app: c.env.APP_NAME || "Mirai",
    bindings: {
      d1: Boolean(c.env.DB),
      r2: Boolean(c.env.MEDIA),
      queues: Boolean(c.env.GENERATION_QUEUE),
      onboardingQueue: Boolean(c.env.ONBOARDING_QUEUE)
    }
  })
);

app.on(["GET", "POST"], "/api/auth/*", (c) => c.get("auth").handler(c.req.raw));
app.on(["POST"], "/polar/webhooks", (c) => c.get("auth").handler(c.req.raw));

app.route("/api/account", accountRoutes);
app.route("/api/clones", cloneRoutes);
app.route("/api/discovery", discoveryRoutes);
app.route("/api/generations", generationRoutes);
app.route("/api/media", mediaRoutes);
app.route("/api/onboarding", onboardingRoutes);
app.route("/api/telemetry", telemetryRoutes);

app.onError((error, c) => errorResponse(c, error));

app.notFound((c) => {
  if (c.req.path.startsWith("/api/")) {
    return c.json({ error: { code: "not_found", message: "API route not found." } }, 404);
  }
  return c.env.ASSETS.fetch(c.req.raw);
});

export default {
  fetch: app.fetch,
  queue: async (batch, env, _ctx) => {
    const firstMessage = batch.messages[0]?.body;
    if (firstMessage && isOnboardingQueueMessage(firstMessage)) {
      await handleOnboardingBatch(batch as MessageBatch<OnboardingQueueMessage>, env);
      return;
    }
    await handleGenerationBatch(batch as MessageBatch<GenerationQueueMessage>, env);
  }
} satisfies ExportedHandler<Env, AppQueueMessage>;
