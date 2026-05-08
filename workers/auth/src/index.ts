import { createAuth, type AuthEnv } from "./auth";
import {
  buildEntitlements,
  type MiraiPlan,
  normalizePlan,
  resolveBillingPlanFromD1,
  type VerifiedSession
} from "./session-verify";

const POLAR_WEBHOOK_PATH = "/polar/webhooks";
const BETTER_AUTH_POLAR_WEBHOOK_PATH = "/api/auth/polar/webhooks";

export default {
  async fetch(request: Request, env: AuthEnv): Promise<Response> {
    const url = new URL(request.url);
    const auth = createAuth(env, url.origin);

    if (url.pathname === POLAR_WEBHOOK_PATH) {
      return auth.handler(rewritePolarWebhookRequest(request));
    }

    if (url.pathname.startsWith("/api/auth/")) {
      return auth.handler(request);
    }

    if (url.pathname === "/internal/session/verify" && request.method === "POST") {
      try {
        const session = await auth.api.getSession({ headers: request.headers });
        if (!session?.user) return json({ error: "unauthorized" }, 401);

        const plan = await readPlanFromSession(session, env);
        return json(buildVerifiedSessionSnapshot(session.user, plan), 200);
      } catch {
        return json({ error: "unauthorized" }, 401);
      }
    }

    return json({ error: "not_found" }, 404);
  }
};

export function rewritePolarWebhookRequest(request: Request) {
  const rewrittenUrl = new URL(request.url);
  rewrittenUrl.pathname = BETTER_AUTH_POLAR_WEBHOOK_PATH;
  return new Request(rewrittenUrl, request);
}

export function buildVerifiedSessionSnapshot(
  user: { id: string; email?: string | null; name?: string | null },
  plan: MiraiPlan
): VerifiedSession {
  return {
    userId: user.id,
    email: user.email || undefined,
    name: user.name || null,
    plan,
    entitlements: buildEntitlements(plan)
  };
}

async function readPlanFromSession(session: unknown, env: AuthEnv) {
  const explicitPlan = readExplicitPlanFromSession(session);
  if (explicitPlan) return normalizePlan(explicitPlan);

  const value = session as { user?: { id?: string } };
  return resolveBillingPlanFromD1(env.DB, value.user?.id || "", {
    proProductId: env.POLAR_PRO_PRODUCT_ID,
    studioProductId: env.POLAR_STUDIO_PRODUCT_ID
  });
}

function readExplicitPlanFromSession(session: unknown): string | null {
  const value = session as {
    user?: {
      plan?: unknown;
      subscription?: { slug?: unknown; plan?: unknown; product?: { slug?: unknown } };
    };
    subscription?: { slug?: unknown; plan?: unknown; product?: { slug?: unknown } };
    activeSubscription?: { slug?: unknown; plan?: unknown; product?: { slug?: unknown } };
  };

  return firstString(
    value.user?.plan,
    value.user?.subscription?.slug,
    value.user?.subscription?.plan,
    value.user?.subscription?.product?.slug,
    value.subscription?.slug,
    value.subscription?.plan,
    value.subscription?.product?.slug,
    value.activeSubscription?.slug,
    value.activeSubscription?.plan,
    value.activeSubscription?.product?.slug
  );
}

function firstString(...values: unknown[]) {
  for (const value of values) {
    if (typeof value === "string" && value.length > 0) return value;
  }
  return null;
}

function json(body: unknown, status: number) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" }
  });
}
