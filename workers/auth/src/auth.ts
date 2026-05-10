import { betterAuth } from "better-auth";
import type { D1Database } from "@cloudflare/workers-types";
import { polarPlugin } from "./polar";

export type AuthEnv = {
  DB: D1Database;
  APP_NAME?: string;
  APP_URL?: string;
  BETTER_AUTH_SECRET?: string;
  GOOGLE_CLIENT_ID?: string;
  GOOGLE_CLIENT_SECRET?: string;
  POLAR_ACCESS_TOKEN?: string;
  POLAR_WEBHOOK_SECRET?: string;
  POLAR_PRO_PRODUCT_ID?: string;
  POLAR_STUDIO_PRODUCT_ID?: string;
  POLAR_SERVER?: string;
};

export function createAuth(env: AuthEnv, requestOrigin?: string) {
  const appUrl = resolveAppUrl(env.APP_URL, requestOrigin);
  const secret = env.BETTER_AUTH_SECRET || resolveLocalDevSecret(appUrl, requestOrigin);

  return betterAuth({
    appName: env.APP_NAME || "Mirai",
    baseURL: appUrl,
    secret,
    database: env.DB,
    trustedOrigins: [appUrl, "http://localhost:5173", "http://localhost:8787"],
    emailAndPassword: { enabled: true },
    socialProviders: resolveSocialProviders(env, appUrl),
    plugins: polarPlugin(env)
  });
}

function resolveLocalDevSecret(appUrl: string, requestOrigin?: string) {
  if (isLocalUrl(appUrl) || isLocalUrl(requestOrigin)) return "mirai-local-dev-secret-change-me";
  throw new Error("BETTER_AUTH_SECRET must be configured outside local development.");
}

function resolveAppUrl(configuredUrl?: string, requestOrigin?: string) {
  const configured = configuredUrl || "http://localhost:5173";
  if (!requestOrigin) return configured;
  const configuredIsLocal = isLocalUrl(configured);
  const requestIsLocal = isLocalUrl(requestOrigin);
  return configuredIsLocal && !requestIsLocal ? requestOrigin : configured;
}

function isLocalUrl(value?: string) {
  if (!value) return false;
  try {
    const hostname = new URL(value).hostname;
    return hostname === "localhost" || hostname === "127.0.0.1";
  } catch {
    return value.includes("localhost") || value.includes("127.0.0.1");
  }
}

function resolveSocialProviders(env: AuthEnv, appUrl: string) {
  if (!env.GOOGLE_CLIENT_ID || !env.GOOGLE_CLIENT_SECRET) return {};
  return {
    google: {
      clientId: env.GOOGLE_CLIENT_ID,
      clientSecret: env.GOOGLE_CLIENT_SECRET,
      redirectURI: `${appUrl}/api/auth/callback/google`
    }
  };
}
