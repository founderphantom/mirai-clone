import { Polar } from "@polar-sh/sdk";
import { checkout, polar, portal, usage, webhooks } from "@polar-sh/better-auth";
import type { AuthEnv } from "./auth";

type PolarPluginUse = Parameters<typeof polar>[0]["use"];
type UnknownRecord = Record<string, unknown>;

export function polarPlugin(env: AuthEnv) {
  if (!env.POLAR_ACCESS_TOKEN) return [];

  const client = new Polar({
    accessToken: env.POLAR_ACCESS_TOKEN,
    server: env.POLAR_SERVER === "production" ? "production" : "sandbox"
  });

  const use: PolarPluginUse = [portal(), usage()];
  const checkoutProducts: { productId: string; slug: string }[] = [];

  if (env.POLAR_PRO_PRODUCT_ID) {
    checkoutProducts.push({ productId: env.POLAR_PRO_PRODUCT_ID, slug: "pro" });
  }

  if (env.POLAR_STUDIO_PRODUCT_ID) {
    checkoutProducts.push({ productId: env.POLAR_STUDIO_PRODUCT_ID, slug: "studio" });
  }

  if (checkoutProducts.length > 0) {
    use.unshift(
      checkout({
        products: checkoutProducts,
        successUrl: "/me?checkout=success&checkout_id={CHECKOUT_ID}",
        authenticatedUsersOnly: true
      })
    );
  }

  if (env.POLAR_WEBHOOK_SECRET) {
    use.push(
      webhooks({
        secret: env.POLAR_WEBHOOK_SECRET,
        onPayload: async (payload) => {
          await insertBillingEvent(env, payload);
        }
      })
    );
  }

  return [
    polar({
      client,
      createCustomerOnSignUp: false,
      use
    })
  ];
}

async function insertBillingEvent(env: AuthEnv, payload: { type?: unknown; data?: unknown }) {
  const data = asRecord(payload.data);

  await env.DB.prepare(
    `INSERT INTO billing_events
      (id, user_id, event_type, polar_customer_id, polar_subscription_id,
       polar_product_id, payload_json, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
  )
    .bind(
      createId("bevt"),
      extractPolarUserId(payload),
      readEventType(payload),
      extractPolarCustomerId(data),
      extractPolarSubscriptionId(data),
      extractPolarProductId(data),
      toJson(payload),
      nowIso()
    )
    .run();
}

function extractPolarUserId(payload: { data?: unknown }): string | null {
  const data = asRecord(payload.data);
  const customer = asRecord(data?.customer);
  const metadata = asRecord(data?.metadata);

  return firstString(
    customer?.external_id,
    customer?.externalId,
    data?.customer_external_id,
    data?.customerExternalId,
    data?.external_id,
    data?.externalId,
    metadata?.user_id,
    metadata?.userId
  );
}

function extractPolarCustomerId(data: UnknownRecord | null) {
  return extractPolarEntityId(data, ["customer_id", "customerId"], ["customer"]);
}

function extractPolarSubscriptionId(data: UnknownRecord | null) {
  return (
    extractPolarEntityId(data, ["subscription_id", "subscriptionId"], ["subscription"]) ??
    extractPolarEntityId(asRecord(data?.checkout), ["subscription_id", "subscriptionId"], ["subscription"])
  );
}

function extractPolarProductId(data: UnknownRecord | null) {
  return (
    extractPolarEntityId(data, ["product_id", "productId"], ["product"]) ??
    extractPolarEntityId(asRecord(data?.subscription), ["product_id", "productId"], ["product"]) ??
    extractPolarEntityId(asRecord(data?.checkout), ["product_id", "productId"], ["product"]) ??
    extractPolarEntityId(asRecord(data?.order), ["product_id", "productId"], ["product"])
  );
}

function extractPolarEntityId(
  data: UnknownRecord | null,
  directKeys: string[],
  nestedKeys: string[]
): string | null {
  if (!data) return null;

  for (const key of directKeys) {
    const direct = data[key];
    if (typeof direct === "string" && direct.length > 0) return direct;
  }

  for (const key of nestedKeys) {
    const nestedId = asRecord(data[key])?.id;
    if (typeof nestedId === "string" && nestedId.length > 0) return nestedId;
  }

  return null;
}

function readEventType(payload: { type?: unknown }) {
  return typeof payload.type === "string" && payload.type.length > 0 ? payload.type : "polar.event";
}

function createId(prefix: string) {
  return `${prefix}_${crypto.randomUUID().replaceAll("-", "")}`;
}

function nowIso() {
  return new Date().toISOString();
}

function toJson(value: unknown) {
  return JSON.stringify(value ?? {});
}

function firstString(...values: unknown[]) {
  for (const value of values) {
    if (typeof value === "string" && value.length > 0) return value;
  }
  return null;
}

function asRecord(value: unknown): UnknownRecord | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as UnknownRecord) : null;
}
