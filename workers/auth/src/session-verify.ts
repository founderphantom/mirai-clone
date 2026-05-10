import type { D1Database } from "@cloudflare/workers-types";

export type MiraiPlan = "free" | "paid";

export type Entitlements = {
  maxActiveClones: number;
  generationPriority: "standard" | "high";
  watermarkExports: boolean;
};

export type VerifiedSession = {
  userId: string;
  email?: string;
  name?: string | null;
  plan: MiraiPlan;
  entitlements: Entitlements;
};

export type PaidProductIds = {
  proProductId?: string | null;
  studioProductId?: string | null;
};

export type BillingEventPlanInput = {
  eventType?: string | null;
  polarProductId?: string | null;
  payloadJson?: string | null;
  payload?: unknown;
};

export function normalizePlan(value: string | null | undefined): MiraiPlan {
  const normalized = String(value || "").toLowerCase();
  if (["paid", "pro", "studio"].includes(normalized)) return "paid";
  return "free";
}

export function buildEntitlements(plan: MiraiPlan): Entitlements {
  if (plan === "paid") {
    return {
      maxActiveClones: 5,
      generationPriority: "high",
      watermarkExports: false
    };
  }

  return {
    maxActiveClones: 1,
    generationPriority: "standard",
    watermarkExports: true
  };
}

export async function resolveBillingPlanFromD1(
  db: D1Database,
  userId: string,
  productIds: PaidProductIds
): Promise<MiraiPlan> {
  const matchingProductIds = paidProductIdValues(productIds);
  if (!userId || matchingProductIds.length === 0) return "free";

  const placeholders = matchingProductIds.map(() => "?").join(", ");
  const result = await db
    .prepare(
      `SELECT event_type AS eventType, polar_product_id AS polarProductId, payload_json AS payloadJson
       FROM billing_events
       WHERE user_id = ?
         AND polar_product_id IN (${placeholders})
       ORDER BY created_at DESC`
    )
    .bind(userId, ...matchingProductIds)
    .all<BillingEventPlanInput>();

  return resolveBillingEventHistoryPlan(result.results ?? [], productIds);
}

export function resolveBillingEventPlan(
  event: BillingEventPlanInput | null | undefined,
  productIds: PaidProductIds
): MiraiPlan {
  return resolveBillingEventPlanSignal(event, productIds) ?? "free";
}

export function resolveBillingEventHistoryPlan(
  events: BillingEventPlanInput[],
  productIds: PaidProductIds
): MiraiPlan {
  for (const event of events) {
    const plan = resolveBillingEventPlanSignal(event, productIds);
    if (plan) return plan;
  }
  return "free";
}

export function resolveBillingEventPlanSignal(
  event: BillingEventPlanInput | null | undefined,
  productIds: PaidProductIds
): MiraiPlan | null {
  if (!event || !matchesPaidProduct(event.polarProductId, productIds)) return null;

  const payloadStatus = normalizeSignal(readPayloadStatus(event));
  const eventType = normalizeSignal(event.eventType);

  if (hasInactiveSignal(payloadStatus) || hasInactiveSignal(eventType)) return "free";
  if (hasPaidSignal(payloadStatus) || hasPaidSignal(eventType)) return "paid";

  return null;
}

function matchesPaidProduct(productId: string | null | undefined, productIds: PaidProductIds) {
  if (!productId) return false;
  return paidProductIdValues(productIds).includes(productId);
}

function paidProductIdValues(productIds: PaidProductIds) {
  return [...new Set([productIds.proProductId, productIds.studioProductId].filter(isNonEmptyString))];
}

function readPayloadStatus(event: BillingEventPlanInput): string | null {
  const payload = asRecord(event.payload) ?? parseJsonRecord(event.payloadJson);
  const data = asRecord(payload?.data);
  const subscription = asRecord(payload?.subscription) ?? asRecord(data?.subscription);

  return firstString(payload?.status, data?.status, subscription?.status);
}

function parseJsonRecord(value: string | null | undefined) {
  if (!value) return null;
  try {
    return asRecord(JSON.parse(value));
  } catch {
    return null;
  }
}

function normalizeSignal(value: string | null | undefined) {
  return String(value || "")
    .trim()
    .toLowerCase();
}

function hasPaidSignal(value: string) {
  return hasSignal(value, ["active", "paid", "uncanceled", "uncancelled", "canceled", "cancelled", "past_due"]);
}

function hasInactiveSignal(value: string) {
  return hasSignal(value, ["expired", "inactive", "revoked", "unpaid"]);
}

function hasSignal(value: string, signals: string[]) {
  if (!value) return false;
  if (signals.includes(value)) return true;
  const parts = value.split(/[.:/-]+/).filter(Boolean);
  return parts.some((part) => signals.includes(part));
}

function firstString(...values: unknown[]) {
  for (const value of values) {
    if (typeof value === "string" && value.length > 0) return value;
  }
  return null;
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}
