import { Polar } from "@polar-sh/sdk";
import { checkout, polar, portal, usage, webhooks } from "@polar-sh/better-auth";
import { betterAuth } from "better-auth";
import type { Env } from "./env";
import { createId, nowIso, run, toJson } from "./db";

type PolarPluginUse = Parameters<typeof polar>[0]["use"];
type UnknownRecord = Record<string, unknown>;

export function createAuth(env: Env, requestOrigin?: string) {
  const plugins: any[] = [];
  const appUrl = resolveAppUrl(env.APP_URL, requestOrigin);
  const isLocal = appUrl.includes("localhost") || appUrl.includes("127.0.0.1");

  if (env.POLAR_ACCESS_TOKEN) {
    const polarClient = new Polar({
      accessToken: env.POLAR_ACCESS_TOKEN,
      server: env.POLAR_SERVER === "production" ? "production" : "sandbox"
    });

    const polarUse: PolarPluginUse = [portal(), usage()];
    if (env.POLAR_PRO_PRODUCT_ID) {
      polarUse.unshift(
        checkout({
          products: [{ productId: env.POLAR_PRO_PRODUCT_ID, slug: "pro" }],
          successUrl: "/account?checkout=success&checkout_id={CHECKOUT_ID}",
          authenticatedUsersOnly: true
        })
      );
    }

    if (env.POLAR_WEBHOOK_SECRET) {
      polarUse.push(
        webhooks({
          secret: env.POLAR_WEBHOOK_SECRET,
          onPayload: async (payload) => {
            await run(
              env.DB,
              `INSERT INTO billing_events
                (id, user_id, event_type, polar_customer_id, polar_subscription_id,
                 polar_product_id, payload_json, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
              [
                createId("bevt"),
                extractPolarUserId(payload),
                payload.type ?? "polar.event",
                extractPolarEntityId(payload.data, "customer_id", "customer"),
                extractPolarEntityId(payload.data, "subscription_id", "subscription"),
                extractPolarEntityId(payload.data, "product_id", "product"),
                toJson(payload),
                nowIso()
              ]
            );
          }
        })
      );
    }

    plugins.push(
      polar({
        client: polarClient,
        // Polar's built-in hook hard-fails auth if a customer with the same
        // email already has a different externalId. Phase 2 will replace this
        // with an idempotent, non-blocking customer sync tied to credits.
        createCustomerOnSignUp: false,
        use: polarUse
      })
    );
  }

  const secret = env.BETTER_AUTH_SECRET ?? (isLocal ? "mirai-local-dev-secret-change-me" : undefined);
  if (!secret) {
    throw new Error("BETTER_AUTH_SECRET must be configured outside local development.");
  }

  return betterAuth({
    appName: env.APP_NAME || "Mirai",
    baseURL: appUrl,
    secret,
    database: env.DB,
    trustedOrigins: [appUrl, "http://localhost:5173", "http://localhost:8787"],
    emailAndPassword: {
      enabled: true
    },
    socialProviders: resolveSocialProviders(env, appUrl),
    plugins
  });
}

function resolveAppUrl(configuredUrl?: string, requestOrigin?: string) {
  const fallback = "http://localhost:5173";
  const configured = configuredUrl || fallback;
  if (!requestOrigin) return configured;

  const configuredIsLocal = configured.includes("localhost") || configured.includes("127.0.0.1");
  const requestIsLocal = requestOrigin.includes("localhost") || requestOrigin.includes("127.0.0.1");
  return configuredIsLocal && !requestIsLocal ? requestOrigin : configured;
}

function resolveSocialProviders(env: Env, appUrl: string) {
  const socialProviders: Record<string, unknown> = {};
  if (env.GOOGLE_CLIENT_ID && env.GOOGLE_CLIENT_SECRET) {
    socialProviders.google = {
      clientId: env.GOOGLE_CLIENT_ID,
      clientSecret: env.GOOGLE_CLIENT_SECRET,
      redirectURI: `${appUrl}/api/auth/callback/google`
    };
  }
  return socialProviders;
}

function extractPolarUserId(payload: { data?: unknown }): string | null {
  const data = asRecord(payload.data);
  const customer = asRecord(data?.customer);
  const externalId = customer?.external_id ?? data?.customer_external_id;
  return typeof externalId === "string" ? externalId : null;
}

function extractPolarEntityId(data: unknown, directKey: string, nestedKey: string): string | null {
  const record = asRecord(data);
  const direct = record?.[directKey];
  if (typeof direct === "string") return direct;

  const nested = asRecord(record?.[nestedKey]);
  const nestedId = nested?.id;
  return typeof nestedId === "string" ? nestedId : null;
}

function asRecord(value: unknown): UnknownRecord | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as UnknownRecord) : null;
}
