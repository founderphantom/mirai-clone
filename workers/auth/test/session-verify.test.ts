import { describe, expect, it } from "vitest";
import {
  buildEntitlements,
  normalizePlan,
  resolveBillingPlanFromD1,
  resolveBillingEventHistoryPlan,
  resolveBillingEventPlan
} from "../src/session-verify";

const paidProductIds = {
  proProductId: "prod_pro",
  studioProductId: "prod_studio"
};

describe("session verification plan mapping", () => {
  it("maps missing plan to free entitlements", () => {
    const plan = normalizePlan(null);
    expect(plan).toBe("free");
    expect(buildEntitlements(plan)).toEqual({
      maxActiveClones: 1,
      generationPriority: "standard",
      watermarkExports: true
    });
  });

  it("maps paid product slugs to paid entitlements", () => {
    for (const value of ["pro", "studio", "paid"]) {
      const plan = normalizePlan(value);
      expect(plan).toBe("paid");
      expect(buildEntitlements(plan).maxActiveClones).toBe(5);
    }
  });

  it("maps matching pro active subscription events to paid", () => {
    const plan = resolveBillingEventPlan(
      {
        eventType: "subscription.active",
        polarProductId: "prod_pro",
        payloadJson: "{}"
      },
      paidProductIds
    );

    expect(plan).toBe("paid");
  });

  it("maps matching studio active payload status to paid", () => {
    const plan = resolveBillingEventPlan(
      {
        eventType: "subscription.updated",
        polarProductId: "prod_studio",
        payloadJson: JSON.stringify({ data: { status: "active" } })
      },
      paidProductIds
    );

    expect(plan).toBe("paid");
  });

  it("keeps canceled subscriptions paid for matching products", () => {
    const plan = resolveBillingEventPlan(
      {
        eventType: "subscription.canceled",
        polarProductId: "prod_pro",
        payloadJson: "{}"
      },
      paidProductIds
    );

    expect(plan).toBe("paid");
  });

  it("keeps recoverable past due subscriptions paid from payload status", () => {
    const plan = resolveBillingEventPlan(
      {
        eventType: "subscription.updated",
        polarProductId: "prod_studio",
        payloadJson: JSON.stringify({ data: { subscription: { status: "past_due" } } })
      },
      paidProductIds
    );

    expect(plan).toBe("paid");
  });

  it("maps immediate-loss subscription events to free", () => {
    expect(
      resolveBillingEventPlan(
        {
          eventType: "subscription.revoked",
          polarProductId: "prod_pro",
          payloadJson: "{}"
        },
        paidProductIds
      )
    ).toBe("free");

    expect(
      resolveBillingEventPlan(
        {
          eventType: "subscription.updated",
          polarProductId: "prod_studio",
          payloadJson: JSON.stringify({ data: { status: "unpaid" } })
        },
        paidProductIds
      )
    ).toBe("free");

    expect(
      resolveBillingEventPlan(
        {
          eventType: "subscription.updated",
          polarProductId: "prod_studio",
          payloadJson: JSON.stringify({ data: { subscription: { status: "expired" } } })
        },
        paidProductIds
      )
    ).toBe("free");
  });

  it("maps unknown product ids to free", () => {
    const plan = resolveBillingEventPlan(
      {
        eventType: "subscription.active",
        polarProductId: "prod_unknown",
        payloadJson: JSON.stringify({ data: { status: "active" } })
      },
      paidProductIds
    );

    expect(plan).toBe("free");
  });

  it("keeps paid access when a neutral billing event is newer than an active subscription event", () => {
    const plan = resolveBillingEventHistoryPlan(
      [
        {
          eventType: "checkout.completed",
          polarProductId: "prod_pro",
          payloadJson: "{}"
        },
        {
          eventType: "subscription.active",
          polarProductId: "prod_pro",
          payloadJson: "{}"
        }
      ],
      paidProductIds
    );

    expect(plan).toBe("paid");
  });

  it("keeps paid access when more than 20 neutral events are newer than an active subscription event", () => {
    const neutralRows = Array.from({ length: 25 }, (_, index) => ({
      eventType: index % 2 === 0 ? "checkout.completed" : "customer.updated",
      polarProductId: "prod_pro",
      payloadJson: "{}"
    }));

    const plan = resolveBillingEventHistoryPlan(
      [
        ...neutralRows,
        {
          eventType: "subscription.active",
          polarProductId: "prod_pro",
          payloadJson: "{}"
        }
      ],
      paidProductIds
    );

    expect(plan).toBe("paid");
  });

  it("does not truncate D1 billing history before finding the latest meaningful event", async () => {
    const neutralRows = Array.from({ length: 25 }, (_, index) => ({
      eventType: index % 2 === 0 ? "checkout.completed" : "order.created",
      polarProductId: "prod_pro",
      payloadJson: "{}"
    }));
    const rows = [
      ...neutralRows,
      {
        eventType: "subscription.active",
        polarProductId: "prod_pro",
        payloadJson: "{}"
      }
    ];
    const db = {
      prepare(sql: string) {
        expect(sql).not.toMatch(/limit\s+20/i);
        return {
          bind() {
            return {
              async all() {
                return { results: rows };
              }
            };
          }
        };
      }
    };

    await expect(resolveBillingPlanFromD1(db as never, "user_123", paidProductIds)).resolves.toBe("paid");
  });

  it("keeps revoked access loss when a neutral billing event is newer than a revoked subscription event", () => {
    const plan = resolveBillingEventHistoryPlan(
      [
        {
          eventType: "customer.updated",
          polarProductId: "prod_studio",
          payloadJson: "{}"
        },
        {
          eventType: "subscription.revoked",
          polarProductId: "prod_studio",
          payloadJson: "{}"
        }
      ],
      paidProductIds
    );

    expect(plan).toBe("free");
  });

  it("maps all-neutral billing event history to free", () => {
    const plan = resolveBillingEventHistoryPlan(
      [
        {
          eventType: "checkout.completed",
          polarProductId: "prod_pro",
          payloadJson: "{}"
        },
        {
          eventType: "order.created",
          polarProductId: "prod_studio",
          payloadJson: "{}"
        }
      ],
      paidProductIds
    );

    expect(plan).toBe("free");
  });
});
