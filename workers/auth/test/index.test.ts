import { afterEach, describe, expect, it, vi } from "vitest";
import authWorker, { buildVerifiedSessionSnapshot, rewritePolarWebhookRequest } from "../src/index";
import { createAuth } from "../src/auth";
import type { AuthEnv } from "../src/auth";

const db = {} as AuthEnv["DB"];

afterEach(() => {
  vi.doUnmock("../src/auth");
  vi.resetModules();
});

describe("auth worker routing", () => {
  it("rewrites external polar webhooks to the Better Auth polar endpoint", async () => {
    const body = JSON.stringify({ type: "subscription.active" });
    const request = new Request("https://auth.example.com/polar/webhooks?delivery=evt_123", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "polar-webhook-signature": "signature"
      },
      body
    });

    const rewritten = rewritePolarWebhookRequest(request);
    const rewrittenUrl = new URL(rewritten.url);

    expect(rewrittenUrl.origin).toBe("https://auth.example.com");
    expect(rewrittenUrl.pathname).toBe("/api/auth/polar/webhooks");
    expect(rewrittenUrl.search).toBe("?delivery=evt_123");
    expect(rewritten.method).toBe("POST");
    expect(rewritten.headers.get("polar-webhook-signature")).toBe("signature");
    expect(await rewritten.text()).toBe(body);
  });

  it("returns 401 for session verification without a valid session", async () => {
    const response = await authWorker.fetch(new Request("http://localhost/internal/session/verify", { method: "POST" }), {
      DB: db,
      APP_URL: "http://localhost:5173"
    });

    expect(response.status).toBe(401);
    expect(await response.json()).toEqual({ error: "unauthorized" });
  });

  it("builds stable paid identity snapshots from valid session users", () => {
    const body = buildVerifiedSessionSnapshot(
      {
        id: "user_123",
        email: "user@example.com",
        name: "Mirai User"
      },
      "paid"
    );

    expect(body).toEqual({
      userId: "user_123",
      email: "user@example.com",
      name: "Mirai User",
      plan: "paid",
      entitlements: {
        maxActiveClones: 5,
        generationPriority: "high",
        watermarkExports: false
      }
    });
  });

  it("builds stable free identity snapshots from valid session users", () => {
    const body = buildVerifiedSessionSnapshot(
      {
        id: "user_free",
        email: null,
        name: null
      },
      "free"
    );

    expect(body).toEqual({
      userId: "user_free",
      email: undefined,
      name: null,
      plan: "free",
      entitlements: {
        maxActiveClones: 1,
        generationPriority: "standard",
        watermarkExports: true
      }
    });
  });

  it("returns a stable identity snapshot for a valid session through fetch", async () => {
    vi.resetModules();
    vi.doMock("../src/auth", () => ({
      createAuth: () => ({
        handler: () => new Response("unexpected", { status: 500 }),
        api: {
          getSession: async () => ({
            user: {
              id: "user_fetch",
              email: "fetch@example.com",
              name: "Fetch User",
              plan: "pro"
            }
          })
        }
      })
    }));

    const { default: worker } = await import("../src/index");
    const response = await worker.fetch(
      new Request("https://auth.example.com/internal/session/verify", { method: "POST" }),
      {
        DB: db,
        APP_URL: "https://auth.example.com",
        BETTER_AUTH_SECRET: "test-secret"
      }
    );

    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({
      userId: "user_fetch",
      email: "fetch@example.com",
      name: "Fetch User",
      plan: "paid",
      entitlements: expect.objectContaining({
        maxActiveClones: 5
      })
    });
  });

  it("requires a configured Better Auth secret outside local development", () => {
    expect(() =>
      createAuth(
        {
          DB: db,
          APP_URL: "https://auth.example.com"
        },
        "https://auth.example.com"
      )
    ).toThrow("BETTER_AUTH_SECRET must be configured outside local development.");
  });
});
