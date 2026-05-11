# Rust Product Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Mirai's first Rust Product Worker milestone: two-Worker auth/product topology, D1 schema, 5-20 manual clone onboarding, private R2 media, clone training queue, Higgsfield OAuth provider foundation, AI/niche research interfaces, and frontend updates that remove clone persona/style prompts.

**Architecture:** Keep Better Auth and Polar in a narrow TypeScript Auth Worker and move product APIs into a Rust Cloudflare Worker. The Rust Worker owns app state, private media, entitlements, queues, provider calls, and AI routing; it verifies sessions through an `AUTH_SERVICE` service binding. The first user-visible product path is manual clone creation from 5-20 images, with Instagram visible as disabled/coming-soon and generation/niche research interfaces ready for follow-on work.

**Tech Stack:** Cloudflare Workers, `workers-rs`, Rust/Wasm, D1, R2, Cloudflare Queues, Workers AI binding, TypeScript Better Auth Worker, Polar, React/Vite, Vitest, Rust unit tests.

---

## Scope And Execution Rules

This plan implements the approved design at `docs/superpowers/specs/2026-05-08-rust-product-backend-design.md`.

Working assumptions:

- Use an isolated worktree at execution time via `superpowers:using-git-worktrees`.
- Do not keep `src/server` as a source of product truth. Only reuse Better Auth/Polar mechanics where useful.
- Do not wire Instagram scraping as functional backend behavior in this plan.
- Do not add clone `persona`, `voice`, or `style_prompt` to the new schema.
- Do not store Higgsfield tokens in D1.
- Commit after each task.

## File Ownership Map

Use this map to avoid overlapping write sets during subagent-driven execution.

Sequential foundation tasks:

- Task 1 owns package/workspace config and Worker config files.
- Task 2 owns `workers/auth/**`.
- Task 3 owns D1 migration files under `config/d1/migrations/**`.
- Task 4 owns Rust Product Worker scaffold files under `workers/product/**`.

Parallelizable after Task 4:

- Task 5 owns Rust domain modules under `workers/product/src/domain/**`.
- Task 6 owns Rust auth/account modules under `workers/product/src/auth_client.rs`, `workers/product/src/routes/account.rs`, and account tests.
- Task 7 owns Rust media modules under `workers/product/src/services/media.rs`, `workers/product/src/routes/media.rs`, and media tests.
- Task 10 owns AI modules under `workers/product/src/ai/**`.

Sequential product path:

- Task 8 depends on Tasks 3, 5, 6, and 7; owns manual clone route/service.
- Task 9 depends on Task 8; owns clone training queue/provider leasing.
- Task 11 depends on Tasks 3, 5, and 10; owns bubbles/research route/queue scaffolding.
- Task 12 depends on API response contracts from Tasks 6, 8, and 11; owns frontend updates.
- Task 13 depends on Tasks 1-12; owns cleanup and final verification.

## Target File Structure

```text
workers/auth/
  src/index.ts
  src/auth.ts
  src/polar.ts
  src/session-verify.ts
  wrangler.auth.jsonc
  test/session-verify.test.ts

workers/product/
  Cargo.toml
  wrangler.product.jsonc
  src/lib.rs
  src/env.rs
  src/http/error.rs
  src/http/router.rs
  src/auth_client.rs
  src/db/mod.rs
  src/domain/entitlements.rs
  src/domain/idempotency.rs
  src/domain/media_validation.rs
  src/domain/status.rs
  src/routes/account.rs
  src/routes/clones.rs
  src/routes/media.rs
  src/routes/onboarding.rs
  src/routes/telemetry.rs
  src/services/accounts.rs
  src/services/clones.rs
  src/services/media.rs
  src/services/provider_accounts.rs
  src/queues/messages.rs
  src/queues/clone_training.rs
  src/queues/niche_research.rs
  src/providers/higgsfield_auth.rs
  src/providers/higgsfield_mcp.rs
  src/ai/model_router.rs
  src/ai/tasks.rs
  tests/domain_tests.rs

config/d1/migrations/
  1000_rust_product_core.sql
  1001_rust_product_indexes.sql

src/client/
  types.ts
  screens/ClonesScreen.tsx
  screens/OnboardingScreen.tsx
  screens/CreateScreen.tsx
  screens/onboarding/UploadReferencePanel.tsx
  screens/onboarding/upload-reference-guidance.ts
  lib/api.ts
```

---

## Task 1: Split Worker Configuration And Scripts

**Order:** Run first.

**Can parallelize:** No.

**Files:**
- Modify: `package.json`
- Modify: `wrangler.jsonc`
- Create: `workers/auth/wrangler.auth.jsonc`
- Create: `workers/product/wrangler.product.jsonc`
- Create: `workers/product/.cargo/config.toml`

**Acceptance Criteria:**
- `npm run build:client` builds the React client only.
- `npm run typecheck` still checks the existing frontend TypeScript.
- `npm run product:check` can run after Task 4 creates Rust files.
- `wrangler.jsonc` no longer points at `src/server/index.ts` as the active runtime.

- [ ] **Step 1: Add Worker split scripts to `package.json`**

Set the script block to include these commands while preserving existing frontend test/typecheck scripts:

```json
{
  "scripts": {
    "dev": "vite --host 0.0.0.0",
    "build:client": "vite build",
    "build": "npm run build:client && npm run product:build",
    "preview": "npm run build:client && wrangler dev -c workers/product/wrangler.product.jsonc",
    "deploy:auth": "wrangler deploy -c workers/auth/wrangler.auth.jsonc",
    "deploy:product": "npm run build:client && wrangler deploy -c workers/product/wrangler.product.jsonc",
    "typecheck": "tsc --noEmit",
    "test": "vitest run",
    "test:watch": "vitest",
    "product:check": "cd workers/product && cargo check --target wasm32-unknown-unknown",
    "product:test": "cd workers/product && cargo test",
    "product:build": "cd workers/product && worker-build --release",
    "auth:dev": "wrangler dev -c workers/auth/wrangler.auth.jsonc",
    "product:dev": "npm run build:client && wrangler dev -c workers/product/wrangler.product.jsonc",
    "db:migrate:local": "wrangler d1 migrations apply mirai-db --local -c workers/product/wrangler.product.jsonc",
    "db:migrate:remote": "wrangler d1 migrations apply mirai-db --remote -c workers/product/wrangler.product.jsonc",
    "cf-typegen": "wrangler types -c workers/product/wrangler.product.jsonc"
  }
}
```

If `worker-build` is not installed on the executor machine, install it once before running `npm run product:build`:

```bash
cargo install worker-build --locked
```

- [ ] **Step 2: Convert root `wrangler.jsonc` into a pointer file**

Replace the root file content with this message-bearing config so accidental deploys do not run `src/server`:

```jsonc
{
  "$schema": "./node_modules/wrangler/config-schema.json",
  "name": "mirai-config-moved",
  "main": "workers/product/build/worker/shim.mjs",
  "compatibility_date": "2026-05-04",
  "vars": {
    "CONFIG_NOTE": "Use workers/product/wrangler.product.jsonc or workers/auth/wrangler.auth.jsonc"
  }
}
```

- [ ] **Step 3: Create `workers/product/wrangler.product.jsonc`**

```jsonc
{
  "$schema": "../../node_modules/wrangler/config-schema.json",
  "name": "mirai-product",
  "main": "build/worker/shim.mjs",
  "compatibility_date": "2026-05-04",
  "assets": {
    "directory": "../../dist/client",
    "binding": "ASSETS",
    "not_found_handling": "single-page-application",
    "run_worker_first": ["/api/*", "/polar/webhooks"]
  },
  "observability": {
    "enabled": true
  },
  "vars": {
    "APP_NAME": "Mirai",
    "APP_URL": "https://mirai.founder-968.workers.dev",
    "ENVIRONMENT": "development",
    "MODERATION_LEVEL": "4",
    "SCRAPECREATORS_CACHE_TTL_SECONDS": "1800",
    "DISCOVERY_DEFAULT_REGION": "US"
  },
  "services": [
    {
      "binding": "AUTH_SERVICE",
      "service": "mirai-auth"
    }
  ],
  "ai": {
    "binding": "AI"
  },
  "d1_databases": [
    {
      "binding": "DB",
      "database_name": "mirai-db",
      "migrations_dir": "../../config/d1/migrations",
      "database_id": "d1430981-3129-47fc-8f93-4fc2696de239"
    }
  ],
  "r2_buckets": [
    {
      "binding": "MEDIA",
      "bucket_name": "mirai-media",
      "preview_bucket_name": "mirai-media-preview"
    }
  ],
  "queues": {
    "producers": [
      { "binding": "CLONE_TRAINING_QUEUE", "queue": "mirai-clone-training" },
      { "binding": "GENERATION_QUEUE", "queue": "mirai-generation" },
      { "binding": "NICHE_RESEARCH_QUEUE", "queue": "mirai-niche-research" }
    ],
    "consumers": [
      {
        "queue": "mirai-clone-training",
        "max_batch_size": 2,
        "max_batch_timeout": 10,
        "max_retries": 3,
        "dead_letter_queue": "mirai-clone-training-dlq"
      },
      {
        "queue": "mirai-generation",
        "max_batch_size": 4,
        "max_batch_timeout": 10,
        "max_retries": 3,
        "dead_letter_queue": "mirai-generation-dlq"
      },
      {
        "queue": "mirai-niche-research",
        "max_batch_size": 2,
        "max_batch_timeout": 30,
        "max_retries": 2,
        "dead_letter_queue": "mirai-niche-research-dlq"
      }
    ]
  }
}
```

- [ ] **Step 4: Create `workers/auth/wrangler.auth.jsonc`**

```jsonc
{
  "$schema": "../../node_modules/wrangler/config-schema.json",
  "name": "mirai-auth",
  "main": "src/index.ts",
  "compatibility_date": "2026-05-04",
  "compatibility_flags": ["nodejs_compat"],
  "observability": {
    "enabled": true
  },
  "vars": {
    "APP_NAME": "Mirai",
    "APP_URL": "https://mirai.founder-968.workers.dev",
    "POLAR_SERVER": "sandbox"
  },
  "d1_databases": [
    {
      "binding": "DB",
      "database_name": "mirai-db",
      "migrations_dir": "../../config/d1/migrations",
      "database_id": "d1430981-3129-47fc-8f93-4fc2696de239"
    }
  ]
}
```

- [ ] **Step 5: Create Rust target config**

Create `workers/product/.cargo/config.toml`:

```toml
[build]
target = "wasm32-unknown-unknown"
```

- [ ] **Step 6: Run config validation**

Run:

```bash
npm run typecheck
```

Expected: TypeScript typecheck either passes or reports only pre-existing frontend errors unrelated to Worker config. If it fails because scripts or JSON are invalid, fix `package.json` or Wrangler JSON before continuing.

- [ ] **Step 7: Commit**

```bash
git add package.json wrangler.jsonc workers/auth/wrangler.auth.jsonc workers/product/wrangler.product.jsonc workers/product/.cargo/config.toml
git commit -m "chore: split auth and product worker configs"
```

---

## Task 2: JS Auth Worker With Better Auth, Polar, And Session Verification

**Order:** Run after Task 1.

**Can parallelize:** No. Product Worker auth depends on this contract.

**Files:**
- Create: `workers/auth/src/auth.ts`
- Create: `workers/auth/src/polar.ts`
- Create: `workers/auth/src/session-verify.ts`
- Create: `workers/auth/src/index.ts`
- Create: `workers/auth/test/session-verify.test.ts`
- Modify: `config/secrets.example.md`

**Acceptance Criteria:**
- `/api/auth/*` is served by the auth Worker.
- `/polar/webhooks` is served by the auth Worker.
- `POST /internal/session/verify` returns `401` without a valid session.
- `POST /internal/session/verify` returns a stable identity snapshot with entitlements for a valid Better Auth session.
- Free plan maps to 1 clone; paid plan maps to 5 clones.

- [ ] **Step 1: Write session verification tests**

Create `workers/auth/test/session-verify.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { buildEntitlements, normalizePlan } from "../src/session-verify";

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
});
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
npx vitest run workers/auth/test/session-verify.test.ts
```

Expected: FAIL because `workers/auth/src/session-verify.ts` does not exist.

- [ ] **Step 3: Implement `workers/auth/src/session-verify.ts`**

```ts
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
```

- [ ] **Step 4: Implement Better Auth factory in `workers/auth/src/auth.ts`**

Use the existing `src/server/auth.ts` only as a mechanical reference for Better Auth and Polar setup. Do not import from `src/server`.

```ts
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
  const secret = env.BETTER_AUTH_SECRET || "mirai-local-dev-secret-change-me";

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

function resolveAppUrl(configuredUrl?: string, requestOrigin?: string) {
  const configured = configuredUrl || "http://localhost:5173";
  if (!requestOrigin) return configured;
  const configuredIsLocal = configured.includes("localhost") || configured.includes("127.0.0.1");
  const requestIsLocal = requestOrigin.includes("localhost") || requestOrigin.includes("127.0.0.1");
  return configuredIsLocal && !requestIsLocal ? requestOrigin : configured;
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
```

- [ ] **Step 5: Implement Polar plugin wrapper in `workers/auth/src/polar.ts`**

```ts
import { Polar } from "@polar-sh/sdk";
import { checkout, polar, portal, usage, webhooks } from "@polar-sh/better-auth";
import type { AuthEnv } from "./auth";

export function polarPlugin(env: AuthEnv) {
  if (!env.POLAR_ACCESS_TOKEN) return [];

  const client = new Polar({
    accessToken: env.POLAR_ACCESS_TOKEN,
    server: env.POLAR_SERVER === "production" ? "production" : "sandbox"
  });

  const use: ReturnType<typeof checkout | typeof portal | typeof usage | typeof webhooks>[] = [
    portal(),
    usage()
  ];

  if (env.POLAR_PRO_PRODUCT_ID) {
    use.unshift(
      checkout({
        products: [{ productId: env.POLAR_PRO_PRODUCT_ID, slug: "pro" }],
        successUrl: "/me?checkout=success&checkout_id={CHECKOUT_ID}",
        authenticatedUsersOnly: true
      })
    );
  }

  if (env.POLAR_STUDIO_PRODUCT_ID) {
    use.unshift(
      checkout({
        products: [{ productId: env.POLAR_STUDIO_PRODUCT_ID, slug: "studio" }],
        successUrl: "/me?checkout=success&checkout_id={CHECKOUT_ID}",
        authenticatedUsersOnly: true
      })
    );
  }

  if (env.POLAR_WEBHOOK_SECRET) {
    use.push(
      webhooks({
        secret: env.POLAR_WEBHOOK_SECRET,
        onPayload: async () => {
          return;
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
```

- [ ] **Step 6: Implement `workers/auth/src/index.ts`**

```ts
import { createAuth, type AuthEnv } from "./auth";
import { buildEntitlements, normalizePlan, type VerifiedSession } from "./session-verify";

export default {
  async fetch(request: Request, env: AuthEnv): Promise<Response> {
    const url = new URL(request.url);
    const auth = createAuth(env, url.origin);

    if (url.pathname.startsWith("/api/auth/") || url.pathname === "/polar/webhooks") {
      return auth.handler(request);
    }

    if (url.pathname === "/internal/session/verify" && request.method === "POST") {
      try {
        const session = await auth.api.getSession({ headers: request.headers });
        if (!session?.user) return json({ error: "unauthorized" }, 401);

        const plan = normalizePlan(readPlanFromSession(session));
        const body: VerifiedSession = {
          userId: session.user.id,
          email: session.user.email || undefined,
          name: session.user.name || null,
          plan,
          entitlements: buildEntitlements(plan)
        };
        return json(body, 200);
      } catch {
        return json({ error: "unauthorized" }, 401);
      }
    }

    return json({ error: "not_found" }, 404);
  }
};

function readPlanFromSession(session: unknown): string | null {
  const value = session as { user?: { plan?: unknown }; subscription?: { slug?: unknown } };
  if (typeof value.user?.plan === "string") return value.user.plan;
  if (typeof value.subscription?.slug === "string") return value.subscription.slug;
  return null;
}

function json(body: unknown, status: number) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" }
  });
}
```

- [ ] **Step 7: Update `config/secrets.example.md`**

Add these names:

```bash
POLAR_STUDIO_PRODUCT_ID=
HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FUFU=
OPENROUTER_API_KEY=
OPENCODE_GO_API_KEY=
```

State in the file: Higgsfield tokens are stored as Cloudflare Secrets only and raw tokens must not be written to D1.

- [ ] **Step 8: Run auth tests**

Run:

```bash
npx vitest run workers/auth/test/session-verify.test.ts
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add workers/auth config/secrets.example.md
git commit -m "feat: add auth worker session verification"
```

---

## Task 3: D1 Rust Product Schema Migrations

**Order:** Run after Task 1. Can run in parallel with Task 2 after Task 1.

**Can parallelize:** Yes, with Task 2.

**Files:**
- Create: `config/d1/migrations/1000_rust_product_core.sql`
- Create: `config/d1/migrations/1001_rust_product_indexes.sql`

**Acceptance Criteria:**
- Migrations create app-owned tables from the design.
- `clone_profiles` has no `persona`, `voice`, or `style_prompt`.
- `clone_profiles` supports Free/Paid entitlement counting.
- Tables needed by later tasks exist before Rust routes are implemented.

- [ ] **Step 1: Write migration structure check**

Run this command before creating the file:

```bash
test ! -f config/d1/migrations/1000_rust_product_core.sql
```

Expected: exit code `0`. If the file already exists, inspect it and merge this task into the existing file rather than creating a duplicate migration.

- [ ] **Step 2: Create `1000_rust_product_core.sql`**

```sql
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS accounts (
  user_id TEXT PRIMARY KEY,
  email TEXT,
  display_name TEXT,
  plan TEXT NOT NULL DEFAULT 'free',
  max_active_clones INTEGER NOT NULL DEFAULT 1,
  generation_priority TEXT NOT NULL DEFAULT 'standard',
  watermark_exports INTEGER NOT NULL DEFAULT 1,
  polar_customer_id TEXT,
  polar_subscription_id TEXT,
  deletion_status TEXT NOT NULL DEFAULT 'active',
  preferences_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS clone_profiles (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  handle TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'manual_upload',
  status TEXT NOT NULL DEFAULT 'active',
  soul_status TEXT NOT NULL DEFAULT 'queued',
  provider TEXT NOT NULL DEFAULT 'higgsfield',
  provider_soul_id TEXT,
  provider_config_json TEXT NOT NULL DEFAULT '{}',
  reference_count_total INTEGER NOT NULL DEFAULT 0,
  reference_count_training_selected INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  UNIQUE(user_id, handle)
);

CREATE TABLE IF NOT EXISTS media_assets (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT,
  kind TEXT NOT NULL,
  source TEXT NOT NULL,
  storage_key TEXT,
  content_type TEXT,
  bytes INTEGER,
  width INTEGER,
  height INTEGER,
  remote_url TEXT,
  sha256 TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  deleted_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS clone_reference_assets (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  media_asset_id TEXT NOT NULL,
  sort_order INTEGER NOT NULL,
  role TEXT NOT NULL DEFAULT 'identity',
  eligibility_status TEXT NOT NULL DEFAULT 'accepted',
  quality_score REAL,
  variety_tags_json TEXT NOT NULL DEFAULT '[]',
  training_selected INTEGER NOT NULL DEFAULT 1,
  rejection_reason TEXT,
  audit_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE CASCADE,
  UNIQUE(clone_id, media_asset_id)
);

CREATE TABLE IF NOT EXISTS soul_training_jobs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  provider TEXT NOT NULL DEFAULT 'higgsfield',
  provider_account_id TEXT,
  provider_job_id TEXT,
  status TEXT NOT NULL DEFAULT 'queued',
  idempotency_key TEXT NOT NULL UNIQUE,
  reference_count INTEGER NOT NULL,
  request_json TEXT NOT NULL DEFAULT '{}',
  response_json TEXT NOT NULL DEFAULT '{}',
  error_code TEXT,
  error_message TEXT,
  queued_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS provider_accounts (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  label TEXT NOT NULL,
  plan TEXT,
  capabilities_json TEXT NOT NULL DEFAULT '[]',
  health_state TEXT NOT NULL DEFAULT 'healthy',
  capacity_json TEXT NOT NULL DEFAULT '{}',
  secret_refs_json TEXT NOT NULL DEFAULT '{}',
  last_auth_check_at TEXT,
  last_successful_job_at TEXT,
  disabled_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS provider_account_leases (
  id TEXT PRIMARY KEY,
  provider_account_id TEXT NOT NULL,
  job_type TEXT NOT NULL,
  job_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  lease_expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  released_at TEXT,
  FOREIGN KEY (provider_account_id) REFERENCES provider_accounts(id) ON DELETE CASCADE,
  UNIQUE(job_type, job_id)
);

CREATE TABLE IF NOT EXISTS generation_jobs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  input_visual_reference_id TEXT,
  input_media_asset_id TEXT,
  provider TEXT NOT NULL DEFAULT 'higgsfield',
  provider_account_id TEXT,
  provider_job_ids_json TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL DEFAULT 'queued',
  prompt TEXT,
  aspect_ratio TEXT,
  quality TEXT,
  request_json TEXT NOT NULL DEFAULT '{}',
  response_json TEXT NOT NULL DEFAULT '{}',
  error_code TEXT,
  error_message TEXT,
  queued_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS generation_outputs (
  id TEXT PRIMARY KEY,
  job_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  media_asset_id TEXT,
  provider_asset_id TEXT,
  raw_url TEXT,
  output_index INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (job_id) REFERENCES generation_jobs(id) ON DELETE CASCADE,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS credit_ledger (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  entry_type TEXT NOT NULL,
  amount INTEGER NOT NULL,
  balance_after INTEGER,
  related_job_type TEXT,
  related_job_id TEXT,
  idempotency_key TEXT NOT NULL UNIQUE,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS billing_events (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  event_type TEXT NOT NULL,
  provider TEXT NOT NULL DEFAULT 'polar',
  external_event_id TEXT,
  polar_customer_id TEXT,
  polar_subscription_id TEXT,
  polar_product_id TEXT,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  UNIQUE(provider, external_event_id)
);

CREATE TABLE IF NOT EXISTS ai_model_invocations (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  task TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  input_hash TEXT,
  status TEXT NOT NULL,
  latency_ms INTEGER,
  cost_estimate_micros INTEGER,
  result_json TEXT NOT NULL DEFAULT '{}',
  error_message TEXT,
  trace_id TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS inspiration_bubbles (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT,
  slug TEXT NOT NULL,
  title TEXT NOT NULL,
  vibe_summary TEXT NOT NULL DEFAULT '',
  search_queries_json TEXT NOT NULL DEFAULT '[]',
  selected INTEGER NOT NULL DEFAULT 0,
  weight REAL NOT NULL DEFAULT 1,
  sort_order INTEGER NOT NULL DEFAULT 0,
  source TEXT NOT NULL DEFAULT 'default',
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS niche_research_queries (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  bubble_id TEXT,
  query TEXT NOT NULL,
  source TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'new',
  cluster TEXT,
  created_at TEXT NOT NULL,
  used_at TEXT,
  FOREIGN KEY (bubble_id) REFERENCES inspiration_bubbles(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS niche_knowledge (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  bit TEXT NOT NULL,
  cluster TEXT,
  source_platform TEXT,
  source_url TEXT,
  score REAL NOT NULL DEFAULT 1,
  raw_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS discovery_sources (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  source TEXT NOT NULL,
  params_json TEXT NOT NULL DEFAULT '{}',
  refreshed_at TEXT,
  expires_at TEXT,
  status TEXT NOT NULL DEFAULT 'stale',
  UNIQUE(provider, source, params_json)
);

CREATE TABLE IF NOT EXISTS discovery_items (
  id TEXT PRIMARY KEY,
  source_id TEXT NOT NULL,
  external_id TEXT NOT NULL,
  platform TEXT NOT NULL,
  media_type TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  author_handle TEXT NOT NULL DEFAULT '',
  thumbnail_url TEXT,
  image_url TEXT,
  source_url TEXT,
  metrics_json TEXT NOT NULL DEFAULT '{}',
  raw_json TEXT NOT NULL DEFAULT '{}',
  discovered_at TEXT NOT NULL,
  expires_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (source_id) REFERENCES discovery_sources(id) ON DELETE CASCADE,
  UNIQUE(platform, external_id)
);

CREATE TABLE IF NOT EXISTS visual_reference_candidates (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  discovery_item_id TEXT,
  source_platform TEXT NOT NULL,
  source_url TEXT,
  image_url TEXT,
  thumbnail_media_asset_id TEXT,
  human_presence_status TEXT NOT NULL DEFAULT 'unreviewed',
  human_presence_score REAL,
  rejection_reason TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  reviewed_at TEXT,
  FOREIGN KEY (discovery_item_id) REFERENCES discovery_items(id) ON DELETE SET NULL,
  FOREIGN KEY (thumbnail_media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS visual_references (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  candidate_id TEXT,
  media_asset_id TEXT,
  source_platform TEXT NOT NULL,
  source_url TEXT,
  aesthetic_tags_json TEXT NOT NULL DEFAULT '[]',
  human_presence_type TEXT NOT NULL,
  human_presence_score REAL NOT NULL,
  moderation_level INTEGER NOT NULL DEFAULT 4,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  FOREIGN KEY (candidate_id) REFERENCES visual_reference_candidates(id) ON DELETE SET NULL,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS user_inspiration_pool (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  bubble_id TEXT,
  visual_reference_id TEXT,
  discovery_item_id TEXT,
  score REAL NOT NULL DEFAULT 1,
  used_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (bubble_id) REFERENCES inspiration_bubbles(id) ON DELETE SET NULL,
  FOREIGN KEY (visual_reference_id) REFERENCES visual_references(id) ON DELETE CASCADE,
  FOREIGN KEY (discovery_item_id) REFERENCES discovery_items(id) ON DELETE CASCADE,
  UNIQUE(user_id, visual_reference_id),
  UNIQUE(user_id, discovery_item_id)
);

CREATE TABLE IF NOT EXISTS feature_flag_overrides (
  user_id TEXT NOT NULL,
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (user_id, key)
);

CREATE TABLE IF NOT EXISTS app_events (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  event TEXT NOT NULL,
  props_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS deletion_jobs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'queued',
  cursor_json TEXT NOT NULL DEFAULT '{}',
  error_message TEXT,
  queued_at TEXT NOT NULL,
  completed_at TEXT,
  updated_at TEXT NOT NULL
);
```

- [ ] **Step 3: Create `1001_rust_product_indexes.sql`**

```sql
PRAGMA foreign_keys = ON;

CREATE INDEX IF NOT EXISTS idx_accounts_plan ON accounts(plan);
CREATE INDEX IF NOT EXISTS idx_clone_profiles_user_status ON clone_profiles(user_id, status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_clone_profiles_soul_status ON clone_profiles(soul_status, updated_at);
CREATE INDEX IF NOT EXISTS idx_media_assets_user_kind ON media_assets(user_id, kind, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_media_assets_clone ON media_assets(clone_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_media_assets_sha ON media_assets(user_id, sha256);
CREATE INDEX IF NOT EXISTS idx_clone_reference_assets_clone ON clone_reference_assets(clone_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_soul_training_jobs_status ON soul_training_jobs(status, updated_at);
CREATE INDEX IF NOT EXISTS idx_soul_training_jobs_clone ON soul_training_jobs(clone_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_provider_accounts_provider_health ON provider_accounts(provider, health_state);
CREATE INDEX IF NOT EXISTS idx_provider_account_leases_active ON provider_account_leases(provider_account_id, status, lease_expires_at);
CREATE INDEX IF NOT EXISTS idx_generation_jobs_user ON generation_jobs(user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_generation_jobs_status ON generation_jobs(status, updated_at);
CREATE INDEX IF NOT EXISTS idx_generation_outputs_job ON generation_outputs(job_id, output_index);
CREATE INDEX IF NOT EXISTS idx_credit_ledger_user ON credit_ledger(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_billing_events_user ON billing_events(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ai_model_invocations_task ON ai_model_invocations(task, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_inspiration_bubbles_user_clone ON inspiration_bubbles(user_id, clone_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_niche_research_queries_status ON niche_research_queries(status, created_at);
CREATE INDEX IF NOT EXISTS idx_niche_knowledge_cluster ON niche_knowledge(user_id, cluster, score DESC);
CREATE INDEX IF NOT EXISTS idx_discovery_items_source ON discovery_items(source_id, discovered_at DESC);
CREATE INDEX IF NOT EXISTS idx_visual_reference_candidates_status ON visual_reference_candidates(human_presence_status, created_at);
CREATE INDEX IF NOT EXISTS idx_visual_references_user_status ON visual_references(user_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_user_inspiration_pool_user_unused ON user_inspiration_pool(user_id, used_at, score DESC);
CREATE INDEX IF NOT EXISTS idx_app_events_user_created ON app_events(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_deletion_jobs_status ON deletion_jobs(status, updated_at);
```

- [ ] **Step 4: Verify schema does not include clone persona/style fields**

Run:

```bash
rg -n "persona|voice|style_prompt" config/d1/migrations/1000_rust_product_core.sql config/d1/migrations/1001_rust_product_indexes.sql
```

Expected: no matches.

- [ ] **Step 5: Apply migrations locally**

Run:

```bash
npm run db:migrate:local
```

Expected: Wrangler applies migrations without SQL errors. If older migrations conflict because they already created prototype tables, create follow-up migration SQL that renames prototype-only columns or rebuilds the local DB before development. Do not modify existing user-owned migrations unless the user approves.

- [ ] **Step 6: Commit**

```bash
git add config/d1/migrations/1000_rust_product_core.sql config/d1/migrations/1001_rust_product_indexes.sql
git commit -m "feat: add rust product d1 schema"
```

---

## Task 4: Rust Product Worker Scaffold

**Order:** Run after Task 1. Can run in parallel with Task 2 and Task 3 after Task 1.

**Can parallelize:** Yes, with Tasks 2 and 3.

**Files:**
- Create: `workers/product/Cargo.toml`
- Create: `workers/product/src/lib.rs`
- Create: `workers/product/src/env.rs`
- Create: `workers/product/src/http/error.rs`
- Create: `workers/product/src/http/router.rs`
- Create: `workers/product/src/http/mod.rs`
- Create: `workers/product/src/db/mod.rs`

**Acceptance Criteria:**
- `npm run product:check` passes.
- `GET /api/health` returns JSON with binding booleans.
- Non-API routes fall back to `ASSETS`.

- [ ] **Step 1: Create `workers/product/Cargo.toml`**

```toml
[package]
name = "mirai-product-worker"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
worker = { version = "0.6", features = ["d1", "queue"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde-wasm-bindgen = "0.6"
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
web-sys = { version = "0.3", features = ["console"] }
thiserror = "2"
uuid = { version = "1", features = ["v4", "js"] }
sha2 = "0.10"
hex = "0.4"
time = { version = "0.3", features = ["formatting", "parsing", "wasm-bindgen"] }

[dev-dependencies]
pretty_assertions = "1"
```

- [ ] **Step 2: Create HTTP error module**

Create `workers/product/src/http/error.rs`:

```rust
use serde::Serialize;
use worker::{Response, Result as WorkerResult};

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn unauthorized() -> Self {
        Self { status: 401, code: "unauthorized", message: "Sign in to continue.".to_string() }
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self { status: 400, code, message: message.into() }
    }

    pub fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self { status: 404, code, message: message.into() }
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self { status: 409, code, message: message.into() }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self { status: 500, code: "internal_error", message: message.into() }
    }

    pub fn to_response(&self) -> WorkerResult<Response> {
        #[derive(Serialize)]
        struct Body<'a> {
            error: ErrorBody<'a>,
        }

        #[derive(Serialize)]
        struct ErrorBody<'a> {
            code: &'a str,
            message: &'a str,
        }

        let mut response = Response::from_json(&Body {
            error: ErrorBody { code: self.code, message: &self.message },
        })?;
        response = response.with_status(self.status);
        Ok(response)
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;
```

- [ ] **Step 3: Create environment helpers**

Create `workers/product/src/env.rs`:

```rust
use worker::{Bucket, D1Database, Env, Queue, Result as WorkerResult};

pub struct Bindings {
    pub db: D1Database,
    pub media: Bucket,
    pub clone_training_queue: Queue,
    pub generation_queue: Queue,
    pub niche_research_queue: Queue,
}

impl Bindings {
    pub fn from_env(env: &Env) -> WorkerResult<Self> {
        Ok(Self {
            db: env.d1("DB")?,
            media: env.bucket("MEDIA")?,
            clone_training_queue: env.queue("CLONE_TRAINING_QUEUE")?,
            generation_queue: env.queue("GENERATION_QUEUE")?,
            niche_research_queue: env.queue("NICHE_RESEARCH_QUEUE")?,
        })
    }
}
```

- [ ] **Step 4: Create D1 helper module**

Create `workers/product/src/db/mod.rs`:

```rust
use serde::de::DeserializeOwned;
use serde_json::Value;
use worker::{D1Database, Result as WorkerResult};

pub async fn first<T: DeserializeOwned>(
    db: &D1Database,
    sql: &str,
    params: Vec<Value>,
) -> WorkerResult<Option<T>> {
    let stmt = db.prepare(sql);
    let stmt = bind_values(stmt, params)?;
    stmt.first(None).await
}

pub async fn all<T: DeserializeOwned>(
    db: &D1Database,
    sql: &str,
    params: Vec<Value>,
) -> WorkerResult<Vec<T>> {
    let stmt = db.prepare(sql);
    let stmt = bind_values(stmt, params)?;
    let result = stmt.all().await?;
    Ok(result.results()?)
}

pub async fn exec(db: &D1Database, sql: &str, params: Vec<Value>) -> WorkerResult<()> {
    let stmt = db.prepare(sql);
    let stmt = bind_values(stmt, params)?;
    stmt.run().await?;
    Ok(())
}

fn bind_values(mut stmt: worker::D1PreparedStatement, params: Vec<Value>) -> WorkerResult<worker::D1PreparedStatement> {
    for value in params {
        stmt = match value {
            Value::Null => stmt.bind(&[wasm_bindgen::JsValue::NULL])?,
            Value::String(value) => stmt.bind(&[value.into()])?,
            Value::Number(value) => {
                if let Some(number) = value.as_f64() {
                    stmt.bind(&[number.into()])?
                } else {
                    stmt.bind(&[value.to_string().into()])?
                }
            }
            Value::Bool(value) => stmt.bind(&[(if value { 1 } else { 0 }).into()])?,
            other => stmt.bind(&[other.to_string().into()])?,
        };
    }
    Ok(stmt)
}
```

- [ ] **Step 5: Create router module**

Create `workers/product/src/http/router.rs`:

```rust
use serde::Serialize;
use worker::{Env, Request, Response, Result as WorkerResult, RouteContext, Router};

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
    app: String,
    bindings: HealthBindings,
}

#[derive(Serialize)]
struct HealthBindings {
    d1: bool,
    r2: bool,
    clone_training_queue: bool,
    generation_queue: bool,
    niche_research_queue: bool,
    ai: bool,
    auth_service: bool,
}

pub async fn run(req: Request, env: Env) -> WorkerResult<Response> {
    Router::new()
        .get_async("/api/health", |_req, ctx| async move { health(ctx).await })
        .run(req, env)
        .await
}

async fn health(ctx: RouteContext<()>) -> WorkerResult<Response> {
    let app = ctx
        .var("APP_NAME")
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "Mirai".to_string());

    Response::from_json(&HealthResponse {
        ok: true,
        app,
        bindings: HealthBindings {
            d1: ctx.env.d1("DB").is_ok(),
            r2: ctx.env.bucket("MEDIA").is_ok(),
            clone_training_queue: ctx.env.queue("CLONE_TRAINING_QUEUE").is_ok(),
            generation_queue: ctx.env.queue("GENERATION_QUEUE").is_ok(),
            niche_research_queue: ctx.env.queue("NICHE_RESEARCH_QUEUE").is_ok(),
            ai: ctx.env.ai("AI").is_ok(),
            auth_service: ctx.env.service("AUTH_SERVICE").is_ok(),
        },
    })
}
```

- [ ] **Step 6: Create module declarations**

Create `workers/product/src/http/mod.rs`:

```rust
pub mod error;
pub mod router;
```

Create `workers/product/src/lib.rs`:

```rust
mod db;
mod env;
mod http;

use worker::{event, Context, Env, Request, Response, Result as WorkerResult};

#[event(fetch, respond_with_errors)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> WorkerResult<Response> {
    let path = req.path();
    if path.starts_with("/api/") || path == "/polar/webhooks" {
        return http::router::run(req, env).await;
    }

    env.service("ASSETS")?.fetch(req).await
}
```

- [ ] **Step 7: Run Rust check**

Run:

```bash
npm run product:check
```

Expected: PASS. If `worker` crate APIs differ from the snippets, use current `workers-rs` docs and update only scaffold signatures. Preserve the module boundaries above.

- [ ] **Step 8: Commit**

```bash
git add workers/product
git commit -m "feat: scaffold rust product worker"
```

---

## Task 5: Rust Domain Modules For Entitlements, Idempotency, Media Validation, And Status

**Order:** Run after Task 4.

**Can parallelize:** Yes, with Tasks 6, 7, and 10 after Task 4.

**Files:**
- Create: `workers/product/src/domain/mod.rs`
- Create: `workers/product/src/domain/entitlements.rs`
- Create: `workers/product/src/domain/idempotency.rs`
- Create: `workers/product/src/domain/media_validation.rs`
- Create: `workers/product/src/domain/status.rs`
- Modify: `workers/product/src/lib.rs`
- Test: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- Domain tests pass with `cargo test`.
- Clone upload count is exactly 5-20.
- Free/Paid clone entitlements are enforced by pure functions.
- Status transitions for clone training are explicit.

- [ ] **Step 1: Write domain tests**

Create `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::domain::entitlements::{can_create_clone, Entitlements};
use mirai_product_worker::domain::idempotency::clone_upload_key;
use mirai_product_worker::domain::media_validation::{validate_reference_count, ReferenceCountError};
use mirai_product_worker::domain::status::{can_transition_soul_status, SoulStatus};

#[test]
fn free_users_can_create_only_one_active_clone() {
    let free = Entitlements { max_active_clones: 1 };
    assert!(can_create_clone(&free, 0).is_ok());
    assert_eq!(can_create_clone(&free, 1).unwrap_err(), "clone_limit_reached");
}

#[test]
fn paid_users_can_create_up_to_five_active_clones() {
    let paid = Entitlements { max_active_clones: 5 };
    assert!(can_create_clone(&paid, 4).is_ok());
    assert_eq!(can_create_clone(&paid, 5).unwrap_err(), "clone_limit_reached");
}

#[test]
fn reference_count_must_match_higgsfield_range() {
    assert_eq!(validate_reference_count(4), Err(ReferenceCountError::TooFew));
    assert_eq!(validate_reference_count(5), Ok(()));
    assert_eq!(validate_reference_count(20), Ok(()));
    assert_eq!(validate_reference_count(21), Err(ReferenceCountError::TooMany));
}

#[test]
fn clone_upload_idempotency_key_is_stable() {
    let a = clone_upload_key("user_1", "My Soul", &["hash_b".to_string(), "hash_a".to_string()]);
    let b = clone_upload_key("user_1", "My Soul", &["hash_a".to_string(), "hash_b".to_string()]);
    assert_eq!(a, b);
    assert!(a.starts_with("clone_upload:user_1:"));
}

#[test]
fn soul_status_transitions_are_explicit() {
    assert!(can_transition_soul_status(SoulStatus::Queued, SoulStatus::Training));
    assert!(can_transition_soul_status(SoulStatus::Training, SoulStatus::Ready));
    assert!(can_transition_soul_status(SoulStatus::Training, SoulStatus::Failed));
    assert!(!can_transition_soul_status(SoulStatus::Ready, SoulStatus::Training));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cd workers/product && cargo test domain_tests
```

Expected: FAIL because domain modules do not exist.

- [ ] **Step 3: Implement entitlement logic**

Create `workers/product/src/domain/entitlements.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entitlements {
    pub max_active_clones: u32,
}

pub fn can_create_clone(entitlements: &Entitlements, active_clone_count: u32) -> Result<(), &'static str> {
    if active_clone_count >= entitlements.max_active_clones {
        return Err("clone_limit_reached");
    }
    Ok(())
}
```

- [ ] **Step 4: Implement idempotency helper**

Create `workers/product/src/domain/idempotency.rs`:

```rust
use sha2::{Digest, Sha256};

pub fn clone_upload_key(user_id: &str, display_name: &str, file_hashes: &[String]) -> String {
    let mut sorted = file_hashes.to_vec();
    sorted.sort();

    let mut hasher = Sha256::new();
    hasher.update(user_id.as_bytes());
    hasher.update(b":");
    hasher.update(display_name.trim().to_lowercase().as_bytes());
    hasher.update(b":");
    for hash in sorted {
        hasher.update(hash.as_bytes());
        hasher.update(b";");
    }

    format!("clone_upload:{}:{}", user_id, hex::encode(hasher.finalize()))
}
```

- [ ] **Step 5: Implement media validation**

Create `workers/product/src/domain/media_validation.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceCountError {
    TooFew,
    TooMany,
}

pub fn validate_reference_count(count: usize) -> Result<(), ReferenceCountError> {
    if count < 5 {
        return Err(ReferenceCountError::TooFew);
    }
    if count > 20 {
        return Err(ReferenceCountError::TooMany);
    }
    Ok(())
}

pub fn is_supported_reference_content_type(content_type: &str) -> bool {
    matches!(
        content_type.to_ascii_lowercase().as_str(),
        "image/jpeg" | "image/jpg" | "image/png" | "image/webp" | "image/heic" | "image/heif"
    )
}
```

- [ ] **Step 6: Implement status transition module**

Create `workers/product/src/domain/status.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoulStatus {
    Draft,
    Queued,
    Training,
    Ready,
    Failed,
    ProviderActionRequired,
}

pub fn can_transition_soul_status(from: SoulStatus, to: SoulStatus) -> bool {
    matches!(
        (from, to),
        (SoulStatus::Draft, SoulStatus::Queued)
            | (SoulStatus::Queued, SoulStatus::Training)
            | (SoulStatus::Queued, SoulStatus::ProviderActionRequired)
            | (SoulStatus::Training, SoulStatus::Ready)
            | (SoulStatus::Training, SoulStatus::Failed)
            | (SoulStatus::ProviderActionRequired, SoulStatus::Queued)
            | (SoulStatus::Failed, SoulStatus::Queued)
    )
}
```

- [ ] **Step 7: Export modules**

Create `workers/product/src/domain/mod.rs`:

```rust
pub mod entitlements;
pub mod idempotency;
pub mod media_validation;
pub mod status;
```

Modify `workers/product/src/lib.rs` so the module is public for tests:

```rust
pub mod domain;
mod db;
mod env;
mod http;
```

- [ ] **Step 8: Run tests**

Run:

```bash
cd workers/product && cargo test domain_tests
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add workers/product/src/domain workers/product/src/lib.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add product domain rules"
```

---

## Task 6: Product Auth Client And Account Routes

**Order:** Run after Task 4 and Task 2.

**Can parallelize:** Yes, after Task 4 with Tasks 5, 7, and 10, but final account route validation depends on Task 2's auth contract.

**Files:**
- Create: `workers/product/src/auth_client.rs`
- Create: `workers/product/src/services/accounts.rs`
- Create: `workers/product/src/routes/account.rs`
- Modify: `workers/product/src/http/router.rs`
- Modify: `workers/product/src/lib.rs`
- Test: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- Account response shape contains `user`, `plan`, `entitlements`, `billing`, and `usage`.
- Missing auth maps to `401`.
- Account upsert logic creates an `accounts` row from verified auth snapshot.

- [ ] **Step 1: Add account mapping test**

Append to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::services::accounts::{account_usage_limits, VerifiedIdentity};

#[test]
fn account_usage_limits_come_from_verified_identity() {
    let identity = VerifiedIdentity {
        user_id: "user_1".to_string(),
        email: Some("creator@example.com".to_string()),
        name: Some("Creator".to_string()),
        plan: "paid".to_string(),
        max_active_clones: 5,
    };

    let limits = account_usage_limits(&identity, 3);
    assert_eq!(limits.active_clones, 3);
    assert_eq!(limits.max_active_clones, 5);
    assert_eq!(limits.plan, "paid");
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cd workers/product && cargo test account_usage_limits_come_from_verified_identity
```

Expected: FAIL because `services::accounts` is not defined.

- [ ] **Step 3: Implement auth client types**

Create `workers/product/src/auth_client.rs`:

```rust
use serde::{Deserialize, Serialize};
use worker::{Headers, Request, RequestInit, Response, Result as WorkerResult, RouteContext};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthVerifyResponse {
    pub user_id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub plan: String,
    pub entitlements: AuthEntitlements,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthEntitlements {
    pub max_active_clones: u32,
    pub generation_priority: String,
    pub watermark_exports: bool,
}

pub async fn verify_session(ctx: &RouteContext<()>, original_headers: &Headers) -> WorkerResult<Option<AuthVerifyResponse>> {
    let service = ctx.env.service("AUTH_SERVICE")?;
    let mut headers = Headers::new();
    if let Some(cookie) = original_headers.get("cookie")? {
        headers.set("cookie", &cookie)?;
    }
    headers.set("content-type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(worker::Method::Post);
    init.with_headers(headers);

    let request = Request::new_with_init("https://auth.internal/internal/session/verify", &init)?;
    let mut response: Response = service.fetch_request(request).await?;
    if response.status_code() == 401 {
        return Ok(None);
    }
    if response.status_code() >= 400 {
        return Err(worker::Error::RustError(format!("auth service returned {}", response.status_code())));
    }
    response.json::<AuthVerifyResponse>().await.map(Some)
}
```

- [ ] **Step 4: Implement account service**

Create `workers/product/src/services/accounts.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    pub user_id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub plan: String,
    pub max_active_clones: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimits {
    pub plan: String,
    pub active_clones: u32,
    pub max_active_clones: u32,
}

pub fn account_usage_limits(identity: &VerifiedIdentity, active_clone_count: u32) -> UsageLimits {
    UsageLimits {
        plan: identity.plan.clone(),
        active_clones: active_clone_count,
        max_active_clones: identity.max_active_clones,
    }
}
```

- [ ] **Step 5: Export service module**

Create `workers/product/src/services/mod.rs`:

```rust
pub mod accounts;
```

Modify `workers/product/src/lib.rs`:

```rust
pub mod domain;
pub mod services;
mod auth_client;
mod db;
mod env;
mod http;
```

- [ ] **Step 6: Create account routes**

Create `workers/product/src/routes/account.rs`:

```rust
use serde::Serialize;
use serde_json::json;
use worker::{Response, Result as WorkerResult, RouteContext};

use crate::auth_client::verify_session;
use crate::db;
use crate::http::error::ApiError;
use crate::services::accounts::{account_usage_limits, VerifiedIdentity};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountResponse {
    user: UserResponse,
    plan: String,
    entitlements: EntitlementResponse,
    usage: UsageResponse,
    billing: BillingResponse,
}

#[derive(Serialize)]
struct UserResponse {
    id: String,
    email: Option<String>,
    name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EntitlementResponse {
    max_active_clones: u32,
    generation_priority: String,
    watermark_exports: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageResponse {
    active_clones: u32,
    max_active_clones: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BillingResponse {
    checkout_enabled: bool,
    portal_enabled: bool,
    server: String,
}

#[derive(serde::Deserialize)]
struct CountRow {
    count: u32,
}

pub async fn get_account(req: worker::Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let verified = match verify_session(&ctx, req.headers()).await? {
        Some(value) => value,
        None => return ApiError::unauthorized().to_response(),
    };

    let active_count = db::first::<CountRow>(
        &ctx.env.d1("DB")?,
        "SELECT COUNT(*) AS count FROM clone_profiles WHERE user_id = ?1 AND status = 'active'",
        vec![json!(verified.user_id)],
    )
    .await?
    .map(|row| row.count)
    .unwrap_or(0);

    let identity = VerifiedIdentity {
        user_id: verified.user_id.clone(),
        email: verified.email.clone(),
        name: verified.name.clone(),
        plan: verified.plan.clone(),
        max_active_clones: verified.entitlements.max_active_clones,
    };
    let usage_limits = account_usage_limits(&identity, active_count);

    let body = AccountResponse {
        user: UserResponse { id: verified.user_id, email: verified.email, name: verified.name },
        plan: verified.plan,
        entitlements: EntitlementResponse {
            max_active_clones: verified.entitlements.max_active_clones,
            generation_priority: verified.entitlements.generation_priority,
            watermark_exports: verified.entitlements.watermark_exports,
        },
        usage: UsageResponse {
            active_clones: usage_limits.active_clones,
            max_active_clones: usage_limits.max_active_clones,
        },
        billing: BillingResponse {
            checkout_enabled: true,
            portal_enabled: true,
            server: ctx.var("POLAR_SERVER").map(|v| v.to_string()).unwrap_or_else(|_| "sandbox".to_string()),
        },
    };

    Response::from_json(&body)
}
```

- [ ] **Step 7: Wire account route**

Create `workers/product/src/routes/mod.rs`:

```rust
pub mod account;
```

Modify `workers/product/src/lib.rs` by adding these module declarations and preserving any declarations created by other tasks:

```rust
pub mod services;
mod auth_client;
mod routes;
```

Modify `workers/product/src/http/router.rs` to register:

```rust
.get_async("/api/account", crate::routes::account::get_account)
```

Place it before `.run(req, env)`.

- [ ] **Step 8: Run tests/checks**

Run:

```bash
cd workers/product && cargo test account_usage_limits_come_from_verified_identity
npm run product:check
```

Expected: both PASS.

- [ ] **Step 9: Commit**

```bash
git add workers/product/src/auth_client.rs workers/product/src/services workers/product/src/routes workers/product/src/http/router.rs workers/product/src/lib.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add product auth and account route"
```

---

## Task 7: Private Media Service And Media Route

**Order:** Run after Task 4.

**Can parallelize:** Yes, after Task 4 with Tasks 5, 6, and 10.

**Files:**
- Create: `workers/product/src/services/media.rs`
- Create: `workers/product/src/routes/media.rs`
- Modify: `workers/product/src/services/mod.rs`
- Modify: `workers/product/src/routes/mod.rs`
- Modify: `workers/product/src/http/router.rs`
- Test: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- Media storage keys are user-scoped and deterministic by asset ID.
- `/api/media/:id` only returns media owned by the verified user.
- R2 responses set private cache headers.

- [ ] **Step 1: Add media path tests**

Append to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::services::media::{media_storage_key, normalize_extension};

#[test]
fn media_storage_key_is_user_scoped() {
    let key = media_storage_key("user/one", "clone:two", "media_abc", "image/png");
    assert_eq!(key, "users/user-one/clones/clone-two/media_abc.png");
}

#[test]
fn normalize_extension_uses_content_type() {
    assert_eq!(normalize_extension("image/jpeg"), "jpg");
    assert_eq!(normalize_extension("image/png"), "png");
    assert_eq!(normalize_extension("image/webp"), "webp");
    assert_eq!(normalize_extension("image/heic"), "heic");
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cd workers/product && cargo test media_storage_key_is_user_scoped
```

Expected: FAIL because `services::media` is not defined.

- [ ] **Step 3: Implement media service helpers**

Create `workers/product/src/services/media.rs`:

```rust
pub fn media_storage_key(user_id: &str, clone_id: &str, media_id: &str, content_type: &str) -> String {
    format!(
        "users/{}/clones/{}/{}.{}",
        safe_segment(user_id),
        safe_segment(clone_id),
        safe_segment(media_id),
        normalize_extension(content_type)
    )
}

pub fn normalize_extension(content_type: &str) -> &'static str {
    match content_type.to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/heic" | "image/heif" => "heic",
        _ => "jpg",
    }
}

pub fn safe_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    out.chars().take(96).collect()
}
```

- [ ] **Step 4: Export media service**

Modify `workers/product/src/services/mod.rs`:

```rust
pub mod accounts;
pub mod media;
```

- [ ] **Step 5: Implement media route**

Create `workers/product/src/routes/media.rs`:

```rust
use serde::Deserialize;
use serde_json::json;
use worker::{Response, Result as WorkerResult, RouteContext};

use crate::auth_client::verify_session;
use crate::db;
use crate::http::error::ApiError;

#[derive(Deserialize)]
struct MediaRow {
    storage_key: Option<String>,
    content_type: Option<String>,
}

pub async fn get_media(req: worker::Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let verified = match verify_session(&ctx, req.headers()).await? {
        Some(value) => value,
        None => return ApiError::unauthorized().to_response(),
    };

    let media_id = match ctx.param("id") {
        Some(value) => value.to_string(),
        None => return ApiError::bad_request("missing_media_id", "Missing media id.").to_response(),
    };

    let row = db::first::<MediaRow>(
        &ctx.env.d1("DB")?,
        "SELECT storage_key, content_type FROM media_assets WHERE id = ?1 AND user_id = ?2 AND deleted_at IS NULL",
        vec![json!(media_id), json!(verified.user_id)],
    )
    .await?;

    let Some(row) = row else {
        return ApiError::not_found("media_not_found", "Media asset was not found.").to_response();
    };

    let Some(storage_key) = row.storage_key else {
        return ApiError::not_found("media_unavailable", "Media asset has no stored object.").to_response();
    };

    let object = ctx.env.bucket("MEDIA")?.get(storage_key).execute().await?;
    let Some(object) = object else {
        return ApiError::not_found("media_object_missing", "Media object is missing.").to_response();
    };

    let content_type = row.content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    let mut response = Response::from_body(object.body().unwrap())?;
    response.headers_mut().set("content-type", &content_type)?;
    response.headers_mut().set("cache-control", "private, max-age=300")?;
    Ok(response)
}
```

- [ ] **Step 6: Wire media route**

Modify `workers/product/src/routes/mod.rs`:

```rust
pub mod account;
pub mod media;
```

Modify `workers/product/src/http/router.rs`:

```rust
.get_async("/api/media/:id", crate::routes::media::get_media)
```

- [ ] **Step 7: Run tests/checks**

Run:

```bash
cd workers/product && cargo test media_storage_key_is_user_scoped normalize_extension_uses_content_type
npm run product:check
```

Expected: both PASS.

- [ ] **Step 8: Commit**

```bash
git add workers/product/src/services/media.rs workers/product/src/routes/media.rs workers/product/src/services/mod.rs workers/product/src/routes/mod.rs workers/product/src/http/router.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add private media route"
```

---

## Task 8: Manual Clone Upload Route And Clone Service

**Order:** Run after Tasks 3, 5, 6, and 7.

**Can parallelize:** No. This is the main integration task.

**Files:**
- Create: `workers/product/src/services/clones.rs`
- Create: `workers/product/src/routes/clones.rs`
- Modify: `workers/product/src/services/mod.rs`
- Modify: `workers/product/src/routes/mod.rs`
- Modify: `workers/product/src/http/router.rs`
- Modify: `workers/product/src/queues/messages.rs`
- Test: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- `POST /api/clones/manual-upload` accepts 5-20 image files.
- The route rejects fewer than 5 and more than 20 references.
- The route rejects unsupported image content types.
- The route creates clone/media/reference/job rows and sends a clone training queue message.
- Response contains the created clone and training job status.

- [ ] **Step 1: Add clone handle tests**

Append to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::services::clones::slugify_handle;

#[test]
fn clone_handle_slug_is_stable() {
    assert_eq!(slugify_handle("My New Soul!!"), "my-new-soul");
    assert_eq!(slugify_handle("   "), "my-soul");
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cd workers/product && cargo test clone_handle_slug_is_stable
```

Expected: FAIL because `services::clones` does not exist.

- [ ] **Step 3: Implement clone service helper**

Create `workers/product/src/services/clones.rs`:

```rust
pub fn slugify_handle(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;

    for ch in value.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }

    while out.ends_with('-') {
        out.pop();
    }

    let out: String = out.chars().take(48).collect();
    if out.is_empty() { "my-soul".to_string() } else { out }
}
```

- [ ] **Step 4: Define queue message type**

Create `workers/product/src/queues/messages.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CloneTrainingMessage {
    SubmitCloneTraining {
        job_id: String,
        clone_id: String,
        user_id: String,
        idempotency_key: String,
    },
}
```

Create `workers/product/src/queues/mod.rs`:

```rust
pub mod messages;
```

Modify `workers/product/src/lib.rs` by adding the queue module and preserving existing declarations:

```rust
mod queues;
```

- [ ] **Step 5: Export clone service**

Modify `workers/product/src/services/mod.rs`:

```rust
pub mod accounts;
pub mod clones;
pub mod media;
```

- [ ] **Step 6: Implement manual clone route**

Create `workers/product/src/routes/clones.rs` with this structure:

```rust
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use worker::{FormEntry, Response, Result as WorkerResult, RouteContext};

use crate::auth_client::verify_session;
use crate::db;
use crate::domain::entitlements::{can_create_clone, Entitlements};
use crate::domain::idempotency::clone_upload_key;
use crate::domain::media_validation::{is_supported_reference_content_type, validate_reference_count, ReferenceCountError};
use crate::http::error::ApiError;
use crate::queues::messages::CloneTrainingMessage;
use crate::services::clones::slugify_handle;
use crate::services::media::media_storage_key;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManualCloneResponse {
    clone: CloneResponse,
    training_job: TrainingJobResponse,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CloneResponse {
    id: String,
    display_name: String,
    handle: String,
    source: String,
    status: String,
    soul_status: String,
    reference_count_total: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrainingJobResponse {
    id: String,
    status: String,
    reference_count: u32,
}

#[derive(serde::Deserialize)]
struct CountRow {
    count: u32,
}

pub async fn manual_upload(mut req: worker::Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let verified = match verify_session(&ctx, req.headers()).await? {
        Some(value) => value,
        None => return ApiError::unauthorized().to_response(),
    };

    let form = req.form_data().await?;
    let display_name = match form.get("displayName").or_else(|| form.get("name")) {
        Some(FormEntry::Field(value)) if !value.trim().is_empty() => value.trim().to_string(),
        _ => "My Soul".to_string(),
    };

    let mut files = Vec::new();
    for key in ["photos", "files", "file"] {
        for entry in form.get_all(key) {
            if let FormEntry::File(file) = entry {
                files.push(file);
            }
        }
    }

    if let Err(error) = validate_reference_count(files.len()) {
        let message = match error {
            ReferenceCountError::TooFew => "Upload at least 5 reference photos.",
            ReferenceCountError::TooMany => "Upload no more than 20 reference photos.",
        };
        return ApiError::bad_request("invalid_reference_count", message).to_response();
    }

    let active_count = db::first::<CountRow>(
        &ctx.env.d1("DB")?,
        "SELECT COUNT(*) AS count FROM clone_profiles WHERE user_id = ?1 AND status = 'active'",
        vec![json!(verified.user_id)],
    )
    .await?
    .map(|row| row.count)
    .unwrap_or(0);

    if can_create_clone(&Entitlements { max_active_clones: verified.entitlements.max_active_clones }, active_count).is_err() {
        return ApiError::conflict("clone_limit_reached", "Your current plan has reached its clone limit.").to_response();
    }

    let clone_id = format!("clone_{}", Uuid::new_v4().simple());
    let training_job_id = format!("train_{}", Uuid::new_v4().simple());
    let handle = slugify_handle(&display_name);
    let now = js_sys::Date::new_0().toISOString().as_string().unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string());

    let mut media_rows = Vec::new();
    let mut hashes = Vec::new();

    for (index, file) in files.into_iter().enumerate() {
        let content_type = file.type_();
        if !is_supported_reference_content_type(&content_type) {
            return ApiError::bad_request("invalid_reference_media", "Reference uploads must be JPEG, PNG, WebP, or HEIC images.").to_response();
        }

        let bytes = file.bytes().await?;
        if bytes.len() > 15 * 1024 * 1024 {
            return ApiError::bad_request("reference_too_large", "Each reference photo must be 15 MB or smaller.").to_response();
        }

        let hash = hex::encode(Sha256::digest(&bytes));
        hashes.push(hash.clone());
        let media_id = format!("media_{}", Uuid::new_v4().simple());
        let storage_key = media_storage_key(&verified.user_id, &clone_id, &media_id, &content_type);

        ctx.env.bucket("MEDIA")?.put(&storage_key, bytes).http_metadata(worker::HttpMetadata {
            content_type: Some(content_type.clone()),
            ..Default::default()
        }).execute().await?;

        media_rows.push((index as u32, media_id, storage_key, content_type, hash));
    }

    let idempotency_key = clone_upload_key(&verified.user_id, &display_name, &hashes);

    db::exec(
        &ctx.env.d1("DB")?,
        "INSERT INTO clone_profiles (id, user_id, display_name, handle, source, status, soul_status, provider, provider_config_json, reference_count_total, reference_count_training_selected, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'manual_upload', 'active', 'queued', 'higgsfield', '{}', ?5, ?5, ?6, ?6)",
        vec![json!(clone_id), json!(verified.user_id), json!(display_name), json!(handle), json!(media_rows.len() as u32), json!(now)],
    ).await?;

    for (sort_order, media_id, storage_key, content_type, hash) in &media_rows {
        db::exec(
            &ctx.env.d1("DB")?,
            "INSERT INTO media_assets (id, user_id, clone_id, kind, source, storage_key, content_type, sha256, metadata_json, created_at) VALUES (?1, ?2, ?3, 'reference', 'manual_upload', ?4, ?5, ?6, '{}', ?7)",
            vec![json!(media_id), json!(verified.user_id), json!(clone_id), json!(storage_key), json!(content_type), json!(hash), json!(now)],
        ).await?;

        db::exec(
            &ctx.env.d1("DB")?,
            "INSERT INTO clone_reference_assets (id, user_id, clone_id, media_asset_id, sort_order, role, eligibility_status, training_selected, audit_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'identity', 'accepted', 1, ?6, ?7)",
            vec![
                json!(format!("ref_{}", Uuid::new_v4().simple())),
                json!(verified.user_id),
                json!(clone_id),
                json!(media_id),
                json!(sort_order),
                json!(json!({ "sha256": hash, "contentType": content_type }).to_string()),
                json!(now),
            ],
        ).await?;
    }

    db::exec(
        &ctx.env.d1("DB")?,
        "INSERT INTO soul_training_jobs (id, user_id, clone_id, provider, status, idempotency_key, reference_count, request_json, queued_at, updated_at) VALUES (?1, ?2, ?3, 'higgsfield', 'queued', ?4, ?5, '{}', ?6, ?6)",
        vec![json!(training_job_id), json!(verified.user_id), json!(clone_id), json!(idempotency_key), json!(media_rows.len() as u32), json!(now)],
    ).await?;

    ctx.env.queue("CLONE_TRAINING_QUEUE")?.send(&CloneTrainingMessage::SubmitCloneTraining {
        job_id: training_job_id.clone(),
        clone_id: clone_id.clone(),
        user_id: verified.user_id.clone(),
        idempotency_key,
    }).await?;

    Response::from_json(&ManualCloneResponse {
        clone: CloneResponse {
            id: clone_id,
            display_name,
            handle,
            source: "manual_upload".to_string(),
            status: "active".to_string(),
            soul_status: "queued".to_string(),
            reference_count_total: media_rows.len() as u32,
        },
        training_job: TrainingJobResponse {
            id: training_job_id,
            status: "queued".to_string(),
            reference_count: media_rows.len() as u32,
        },
    })
}
```

- [ ] **Step 7: Wire route**

Modify `workers/product/src/routes/mod.rs`:

```rust
pub mod account;
pub mod clones;
pub mod media;
```

Modify `workers/product/src/http/router.rs`:

```rust
.post_async("/api/clones/manual-upload", crate::routes::clones::manual_upload)
```

- [ ] **Step 8: Run tests/checks**

Run:

```bash
cd workers/product && cargo test clone_handle_slug_is_stable
npm run product:check
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add workers/product/src/services/clones.rs workers/product/src/routes/clones.rs workers/product/src/services/mod.rs workers/product/src/routes/mod.rs workers/product/src/http/router.rs workers/product/src/queues workers/product/src/lib.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add manual clone upload route"
```

---

## Task 9: Clone Training Queue, Provider Leases, And Higgsfield Auth Foundation

**Order:** Run after Task 8.

**Can parallelize:** No. It consumes clone upload output.

**Files:**
- Create: `workers/product/src/services/provider_accounts.rs`
- Create: `workers/product/src/providers/higgsfield_auth.rs`
- Create: `workers/product/src/providers/higgsfield_mcp.rs`
- Create: `workers/product/src/providers/mod.rs`
- Create: `workers/product/src/queues/clone_training.rs`
- Modify: `workers/product/src/queues/mod.rs`
- Modify: `workers/product/src/lib.rs`
- Test: `workers/product/tests/domain_tests.rs`
- Create: `scripts/higgsfield_device_auth.py`

**Acceptance Criteria:**
- Provider lease selection is testable without network.
- Higgsfield token refresh and validate endpoints are encapsulated.
- Queue consumer moves clone training jobs through `queued -> training`.
- If Higgsfield MCP tool submission is not available in local secrets, queue marks `provider_action_required` with a typed error instead of losing the job.
- Raw tokens are never inserted into D1.

- [ ] **Step 1: Add provider lease pure test**

Append to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::services::provider_accounts::{choose_provider_account, ProviderAccountCandidate};

#[test]
fn provider_selection_skips_unhealthy_accounts() {
    let candidates = vec![
        ProviderAccountCandidate { id: "bad".to_string(), health_state: "auth_required".to_string(), active_leases: 0, max_leases: 2 },
        ProviderAccountCandidate { id: "good".to_string(), health_state: "healthy".to_string(), active_leases: 1, max_leases: 2 },
    ];
    assert_eq!(choose_provider_account(&candidates).unwrap().id, "good");
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cd workers/product && cargo test provider_selection_skips_unhealthy_accounts
```

Expected: FAIL because `services::provider_accounts` is not defined.

- [ ] **Step 3: Implement provider account selection**

Create `workers/product/src/services/provider_accounts.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAccountCandidate {
    pub id: String,
    pub health_state: String,
    pub active_leases: u32,
    pub max_leases: u32,
}

pub fn choose_provider_account(candidates: &[ProviderAccountCandidate]) -> Option<ProviderAccountCandidate> {
    candidates
        .iter()
        .filter(|candidate| candidate.health_state == "healthy")
        .filter(|candidate| candidate.active_leases < candidate.max_leases)
        .min_by_key(|candidate| candidate.active_leases)
        .cloned()
}
```

Modify `workers/product/src/services/mod.rs`:

```rust
pub mod accounts;
pub mod clones;
pub mod media;
pub mod provider_accounts;
```

- [ ] **Step 4: Create Higgsfield auth client**

Create `workers/product/src/providers/higgsfield_auth.rs`:

```rust
use serde::{Deserialize, Serialize};
use worker::{console_log, Env, Headers, Request, RequestInit, Result as WorkerResult};

const DEVICE_AUTH_BASE: &str = "https://fnf-device-auth.higgsfield.ai";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HiggsfieldTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u32,
    pub refresh_token: String,
    pub refresh_expires_in: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HiggsfieldValidateResponse {
    pub user_id: String,
}

pub async fn refresh_access_token(env: &Env, refresh_secret_name: &str) -> WorkerResult<HiggsfieldTokenResponse> {
    let refresh_token = env.secret(refresh_secret_name)?.to_string();
    let mut headers = Headers::new();
    headers.set("content-type", "application/json")?;

    let body = serde_json::json!({ "refresh_token": refresh_token }).to_string();
    let mut init = RequestInit::new();
    init.with_method(worker::Method::Post);
    init.with_headers(headers);
    init.with_body(Some(body.into()));

    let request = Request::new_with_init(&format!("{}/refresh", DEVICE_AUTH_BASE), &init)?;
    let mut response = worker::Fetch::Request(request).send().await?;
    if response.status_code() >= 400 {
        console_log!("higgsfield refresh failed with status {}", response.status_code());
        return Err(worker::Error::RustError("higgsfield_refresh_failed".to_string()));
    }
    response.json().await
}

pub async fn validate_access_token(access_token: &str) -> WorkerResult<HiggsfieldValidateResponse> {
    let mut headers = Headers::new();
    headers.set("content-type", "application/json")?;
    let body = serde_json::json!({ "token": access_token }).to_string();
    let mut init = RequestInit::new();
    init.with_method(worker::Method::Post);
    init.with_headers(headers);
    init.with_body(Some(body.into()));
    let request = Request::new_with_init(&format!("{}/validate", DEVICE_AUTH_BASE), &init)?;
    let mut response = worker::Fetch::Request(request).send().await?;
    if response.status_code() >= 400 {
        return Err(worker::Error::RustError("higgsfield_validate_failed".to_string()));
    }
    response.json().await
}
```

- [ ] **Step 5: Create MCP client wrapper**

Create `workers/product/src/providers/higgsfield_mcp.rs`:

```rust
use serde::{Deserialize, Serialize};
use worker::{Headers, Request, RequestInit, Result as WorkerResult};

const MCP_URL: &str = "https://mcp.higgsfield.ai/mcp";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    pub raw_json: serde_json::Value,
}

pub async fn call_tool(access_token: &str, call: McpToolCall) -> WorkerResult<McpToolResult> {
    let mut headers = Headers::new();
    headers.set("authorization", &format!("Bearer {}", access_token))?;
    headers.set("content-type", "application/json")?;
    headers.set("accept", "application/json, text/event-stream")?;

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "mirai-provider-call",
        "method": "tools/call",
        "params": {
            "name": call.name,
            "arguments": call.arguments
        }
    })
    .to_string();

    let mut init = RequestInit::new();
    init.with_method(worker::Method::Post);
    init.with_headers(headers);
    init.with_body(Some(body.into()));

    let request = Request::new_with_init(MCP_URL, &init)?;
    let mut response = worker::Fetch::Request(request).send().await?;
    if response.status_code() >= 400 {
        return Err(worker::Error::RustError(format!("higgsfield_mcp_failed_{}", response.status_code())));
    }
    let raw_json = response.json::<serde_json::Value>().await?;
    Ok(McpToolResult { raw_json })
}
```

- [ ] **Step 6: Create provider module export**

Create `workers/product/src/providers/mod.rs`:

```rust
pub mod higgsfield_auth;
pub mod higgsfield_mcp;
```

Modify `workers/product/src/lib.rs` by adding the provider module and preserving existing declarations:

```rust
mod providers;
```

- [ ] **Step 7: Create clone training queue consumer**

Create `workers/product/src/queues/clone_training.rs`:

```rust
use serde::Deserialize;
use serde_json::json;
use worker::{console_error, MessageBatch, Result as WorkerResult};

use crate::db;
use crate::queues::messages::CloneTrainingMessage;

#[derive(Deserialize)]
struct TrainingJobRow {
    id: String,
    user_id: String,
    clone_id: String,
    status: String,
}

pub async fn handle_batch(batch: MessageBatch<CloneTrainingMessage>, env: worker::Env) -> WorkerResult<()> {
    for message in batch.messages()? {
        let result = handle_message(message.body(), &env).await;
        match result {
            Ok(()) => message.ack(),
            Err(error) => {
                console_error!("clone training message failed: {:?}", error);
                message.retry();
            }
        }
    }
    Ok(())
}

async fn handle_message(message: CloneTrainingMessage, env: &worker::Env) -> WorkerResult<()> {
    match message {
        CloneTrainingMessage::SubmitCloneTraining { job_id, clone_id: _, user_id: _, idempotency_key: _ } => {
            submit_training(job_id, env).await
        }
    }
}

async fn submit_training(job_id: String, env: &worker::Env) -> WorkerResult<()> {
    let db_handle = env.d1("DB")?;
    let job = db::first::<TrainingJobRow>(
        &db_handle,
        "SELECT id, user_id, clone_id, status FROM soul_training_jobs WHERE id = ?1",
        vec![json!(job_id)],
    ).await?;

    let Some(job) = job else {
        return Ok(());
    };

    if job.status != "queued" {
        return Ok(());
    }

    let now = js_sys::Date::new_0().toISOString().as_string().unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string());
    db::exec(
        &db_handle,
        "UPDATE soul_training_jobs SET status = 'training', started_at = ?1, updated_at = ?1 WHERE id = ?2",
        vec![json!(now), json!(job.id)],
    ).await?;

    db::exec(
        &db_handle,
        "UPDATE clone_profiles SET soul_status = 'training', updated_at = ?1 WHERE id = ?2 AND user_id = ?3",
        vec![json!(now), json!(job.clone_id), json!(job.user_id)],
    ).await?;

    if env.secret("HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FUFU").is_err() {
        db::exec(
            &db_handle,
            "UPDATE soul_training_jobs SET status = 'provider_action_required', error_code = 'higgsfield_secret_missing', updated_at = ?1 WHERE id = ?2",
            vec![json!(now), json!(job.id)],
        ).await?;
        db::exec(
            &db_handle,
            "UPDATE clone_profiles SET soul_status = 'provider_action_required', updated_at = ?1 WHERE id = ?2 AND user_id = ?3",
            vec![json!(now), json!(job.clone_id), json!(job.user_id)],
        ).await?;
        return Ok(());
    }

    Ok(())
}
```

- [ ] **Step 8: Wire queue event**

Modify `workers/product/src/queues/mod.rs`:

```rust
pub mod clone_training;
pub mod messages;
```

Modify `workers/product/src/lib.rs` to include queue event:

```rust
use queues::messages::CloneTrainingMessage;
use worker::{event, Context, Env, MessageBatch, Request, Response, Result as WorkerResult};

#[event(queue)]
pub async fn queue(batch: MessageBatch<CloneTrainingMessage>, env: Env, _ctx: Context) -> WorkerResult<()> {
    queues::clone_training::handle_batch(batch, env).await
}
```

- [ ] **Step 9: Add token bootstrap script**

Create `scripts/higgsfield_device_auth.py`:

```python
#!/usr/bin/env python3
import json
import sys
import time
import urllib.request

BASE = "https://fnf-device-auth.higgsfield.ai"

def post_json(path, body=None):
    data = json.dumps(body or {}).encode()
    req = urllib.request.Request(
        f"{BASE}{path}",
        data=data,
        headers={"content-type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req) as response:
        return json.loads(response.read().decode())

def main():
    if "--help" in sys.argv or "-h" in sys.argv:
        print("Usage: python scripts/higgsfield_device_auth.py [--print-token]")
        print("Starts Higgsfield device OAuth and prints the wrangler secret command.")
        return

    auth = post_json("/authorize")
    print(f"Authorize here: {auth['verification_uri']}", flush=True)
    deadline = time.time() + int(auth["expires_in"])
    interval = int(auth.get("interval", 3))

    while time.time() < deadline:
        time.sleep(interval)
        try:
            token = post_json("/token", {"device_code": auth["device_code"]})
        except urllib.error.HTTPError as exc:
            body = json.loads(exc.read().decode())
            if body.get("detail") == "authorization_pending":
                continue
            raise

        refresh_token = token["refresh_token"]
        print("Authorization succeeded.")
        print("Run this command to store the refresh token as a Cloudflare secret:")
        print("wrangler secret put HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FUFU -c workers/product/wrangler.product.jsonc")
        if "--print-token" in sys.argv:
            print(refresh_token)
        return

    raise SystemExit("Authorization timed out.")

if __name__ == "__main__":
    main()
```

- [ ] **Step 10: Run tests/checks**

Run:

```bash
cd workers/product && cargo test provider_selection_skips_unhealthy_accounts
npm run product:check
python scripts/higgsfield_device_auth.py --help
```

Expected: Rust tests/check pass. Python command prints usage text and does not start the device flow. Do not authorize during automated test runs.

- [ ] **Step 11: Commit**

```bash
git add workers/product/src/services/provider_accounts.rs workers/product/src/providers workers/product/src/queues workers/product/src/lib.rs workers/product/tests/domain_tests.rs scripts/higgsfield_device_auth.py
git commit -m "feat: add clone training queue foundation"
```

---

## Task 10: AI Model Router And Moderation Config

**Order:** Run after Task 4.

**Can parallelize:** Yes, after Task 4 with Tasks 5, 6, and 7.

**Files:**
- Create: `workers/product/src/ai/tasks.rs`
- Create: `workers/product/src/ai/model_router.rs`
- Create: `workers/product/src/ai/mod.rs`
- Modify: `workers/product/src/lib.rs`
- Test: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- Vision-only tasks never route to text-only models.
- DeepSeek V4 Pro is allowed for text tasks and rejected for image review tasks.
- Moderation level clamps to 0-10.

- [ ] **Step 1: Add AI routing tests**

Append to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::ai::model_router::{choose_model, clamp_moderation_level, ModelConfig};
use mirai_product_worker::ai::tasks::AiTask;

#[test]
fn text_only_models_are_not_chosen_for_vision_tasks() {
    let models = vec![
        ModelConfig { provider: "openrouter".to_string(), model: "deepseek/deepseek-v4-pro".to_string(), supports_vision: false, supports_structured_json: true },
        ModelConfig { provider: "workers_ai".to_string(), model: "@cf/moonshotai/kimi-k2.6".to_string(), supports_vision: true, supports_structured_json: true },
    ];
    let selected = choose_model(AiTask::HumanPresenceDetection, &models).unwrap();
    assert_eq!(selected.provider, "workers_ai");
}

#[test]
fn deepseek_can_handle_text_tasks() {
    let models = vec![
        ModelConfig { provider: "openrouter".to_string(), model: "deepseek/deepseek-v4-pro".to_string(), supports_vision: false, supports_structured_json: true },
    ];
    let selected = choose_model(AiTask::NicheSeedExtraction, &models).unwrap();
    assert_eq!(selected.model, "deepseek/deepseek-v4-pro");
}

#[test]
fn moderation_level_is_bounded() {
    assert_eq!(clamp_moderation_level(-1), 0);
    assert_eq!(clamp_moderation_level(4), 4);
    assert_eq!(clamp_moderation_level(99), 10);
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cd workers/product && cargo test text_only_models_are_not_chosen_for_vision_tasks
```

Expected: FAIL because `ai` modules do not exist.

- [ ] **Step 3: Define AI tasks**

Create `workers/product/src/ai/tasks.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiTask {
    PhotoQualityReview,
    HumanPresenceDetection,
    BubbleGeneration,
    NicheSeedExtraction,
    NicheClusterExpansion,
    VisualReferenceSelection,
    Moderation,
}

impl AiTask {
    pub fn requires_vision(self) -> bool {
        matches!(
            self,
            AiTask::PhotoQualityReview | AiTask::HumanPresenceDetection
        )
    }
}
```

- [ ] **Step 4: Implement model router**

Create `workers/product/src/ai/model_router.rs`:

```rust
use super::tasks::AiTask;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub supports_vision: bool,
    pub supports_structured_json: bool,
}

pub fn choose_model(task: AiTask, models: &[ModelConfig]) -> Option<ModelConfig> {
    models
        .iter()
        .find(|model| {
            if task.requires_vision() && !model.supports_vision {
                return false;
            }
            if !model.supports_structured_json {
                return false;
            }
            true
        })
        .cloned()
}

pub fn clamp_moderation_level(value: i32) -> u8 {
    value.clamp(0, 10) as u8
}
```

- [ ] **Step 5: Export AI modules**

Create `workers/product/src/ai/mod.rs`:

```rust
pub mod model_router;
pub mod tasks;
```

Modify `workers/product/src/lib.rs` by adding the AI module and preserving existing declarations:

```rust
pub mod ai;
```

- [ ] **Step 6: Run tests**

Run:

```bash
cd workers/product && cargo test text_only_models_are_not_chosen_for_vision_tasks deepseek_can_handle_text_tasks moderation_level_is_bounded
npm run product:check
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add workers/product/src/ai workers/product/src/lib.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add ai model routing rules"
```

---

## Task 11: Bubbles, Niche Research Queue Skeleton, And Discovery Feed Contract

**Order:** Run after Tasks 3, 5, and 10.

**Can parallelize:** No if Task 8 is active, because this touches router/routes. It can run after Task 8 or coordinate carefully.

**Files:**
- Create: `workers/product/src/routes/onboarding.rs`
- Create: `workers/product/src/queues/niche_research.rs`
- Modify: `workers/product/src/queues/mod.rs`
- Modify: `workers/product/src/routes/mod.rs`
- Modify: `workers/product/src/http/router.rs`
- Test: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- `/api/onboarding/state` returns clones, activeClone, bubbles, and inspirationPoolCount.
- `/api/onboarding/bubbles/generate` creates deterministic default bubbles if none exist.
- `/api/onboarding/bubbles` saves selected bubbles and enqueues niche research.
- Niche research message type includes user, clone, selected bubble ids, and moderation level.

- [ ] **Step 1: Add default bubble test**

Append to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::routes::onboarding::default_bubbles;

#[test]
fn default_bubbles_include_visual_queries() {
    let bubbles = default_bubbles();
    assert!(bubbles.len() >= 8);
    assert!(bubbles.iter().all(|bubble| !bubble.search_queries.is_empty()));
    assert!(bubbles.iter().any(|bubble| bubble.slug == "y2k-cafe"));
}
```

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cd workers/product && cargo test default_bubbles_include_visual_queries
```

Expected: FAIL because onboarding route is not defined.

- [ ] **Step 3: Implement default bubble data**

Create `workers/product/src/routes/onboarding.rs`:

```rust
use serde::Serialize;
use worker::{Response, Result as WorkerResult, RouteContext};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BubbleSeed {
    pub slug: &'static str,
    pub title: &'static str,
    pub vibe_summary: &'static str,
    pub search_queries: Vec<&'static str>,
}

pub fn default_bubbles() -> Vec<BubbleSeed> {
    vec![
        BubbleSeed { slug: "y2k-cafe", title: "Y2K Cafe", vibe_summary: "Pastel cafe scenes, playful accessories, soft flash energy.", search_queries: vec!["y2k cafe outfit", "pastel cafe aesthetic", "creator cafe photoshoot"] },
        BubbleSeed { slug: "tokyo-neon", title: "Tokyo Neon", vibe_summary: "Night city color, glossy styling, and cinematic street light.", search_queries: vec!["tokyo neon fashion", "night street outfit reels", "neon city portrait"] },
        BubbleSeed { slug: "streetwear-fit", title: "Streetwear Fit", vibe_summary: "Layered outfits, sneakers, sidewalks, and confident movement.", search_queries: vec!["streetwear fit check", "sneaker outfit inspiration", "urban creator streetwear"] },
        BubbleSeed { slug: "clean-girl", title: "Clean Girl Errands", vibe_summary: "Minimal errands, matcha stops, athleisure, and bright mornings.", search_queries: vec!["clean girl errands outfit", "matcha morning aesthetic", "minimal lifestyle creator"] },
        BubbleSeed { slug: "coastal-weekend", title: "Coastal Weekend", vibe_summary: "Linen, sunglasses, ocean light, and relaxed luxury.", search_queries: vec!["coastal outfit creator", "linen beach aesthetic", "coastal weekend photoshoot"] },
        BubbleSeed { slug: "golden-hour", title: "Golden Hour", vibe_summary: "Warm city light, rooftops, polished lifestyle poses.", search_queries: vec!["golden hour portrait outfit", "rooftop creator aesthetic", "sunset city fashion"] },
        BubbleSeed { slug: "editorial-flash", title: "Editorial Flash", vibe_summary: "Direct flash, strong silhouettes, beauty editorial energy.", search_queries: vec!["editorial flash portrait", "direct flash fashion", "beauty creator editorial"] },
        BubbleSeed { slug: "pilates-morning", title: "Pilates Morning", vibe_summary: "Wellness studio energy, activewear sets, calm routine content.", search_queries: vec!["pilates morning aesthetic", "activewear creator shoot", "wellness routine outfit"] },
    ]
}

pub async fn onboarding_state(_req: worker::Request, _ctx: RouteContext<()>) -> WorkerResult<Response> {
    Response::from_json(&serde_json::json!({
        "clones": [],
        "activeClone": null,
        "bubbles": [],
        "inspirationPoolCount": 0,
        "starters": [],
        "instagram": { "enabled": false, "status": "coming_soon" }
    }))
}
```

- [ ] **Step 4: Add niche research message type**

Create `workers/product/src/queues/niche_research.rs`:

```rust
use serde::{Deserialize, Serialize};
use worker::{MessageBatch, Result as WorkerResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NicheResearchMessage {
    SeedFromBubbles {
        user_id: String,
        clone_id: String,
        bubble_ids: Vec<String>,
        moderation_level: u8,
    },
}

pub async fn handle_batch(batch: MessageBatch<NicheResearchMessage>, _env: worker::Env) -> WorkerResult<()> {
    for message in batch.messages()? {
        message.ack();
    }
    Ok(())
}
```

- [ ] **Step 5: Wire onboarding route**

Modify `workers/product/src/routes/mod.rs`:

```rust
pub mod account;
pub mod clones;
pub mod media;
pub mod onboarding;
```

Modify `workers/product/src/queues/mod.rs`:

```rust
pub mod clone_training;
pub mod messages;
pub mod niche_research;
```

Modify `workers/product/src/http/router.rs`:

```rust
.get_async("/api/onboarding/state", crate::routes::onboarding::onboarding_state)
```

- [ ] **Step 6: Run tests/checks**

Run:

```bash
cd workers/product && cargo test default_bubbles_include_visual_queries
npm run product:check
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add workers/product/src/routes/onboarding.rs workers/product/src/queues/niche_research.rs workers/product/src/routes/mod.rs workers/product/src/queues/mod.rs workers/product/src/http/router.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add onboarding bubble foundation"
```

---

## Task 12: Frontend Clone Onboarding Updates

**Order:** Run after Tasks 6, 8, and 11 define response contracts.

**Can parallelize:** No. It depends on API shapes.

**Files:**
- Modify: `src/client/types.ts`
- Modify: `src/client/screens/ClonesScreen.tsx`
- Modify: `src/client/screens/OnboardingScreen.tsx`
- Modify: `src/client/screens/onboarding/UploadReferencePanel.tsx`
- Modify: `src/client/screens/onboarding/upload-reference-guidance.ts`
- Modify: `src/client/screens/CreateScreen.tsx`
- Modify: `src/client/screens/BlitzScreen.tsx`

**Acceptance Criteria:**
- Frontend clone types have no `persona` or `style_prompt`.
- Clone creation UI uses 5-20 upload path.
- Instagram tab/section is visible but disabled or marked coming soon.
- Create flow does not require a text prompt for future generation.
- `npm run typecheck` passes.

- [ ] **Step 1: Update `src/client/types.ts`**

Use this clone type:

```ts
export type Clone = {
  id: string;
  display_name: string;
  handle: string;
  source?: "manual_upload" | "starter" | "future_instagram";
  status?: "active" | "archived" | "deleting";
  soul_status?: "draft" | "queued" | "training" | "ready" | "failed" | "provider_action_required";
  provider?: "higgsfield";
  provider_soul_id?: string | null;
  reference_count_total?: number;
  reference_count_training_selected?: number;
  generation_count?: number;
};
```

Keep the existing `Job` type `prompt` field optional:

```ts
prompt?: string | null;
```

- [ ] **Step 2: Update upload constants**

In `src/client/screens/onboarding/upload-reference-guidance.ts`, set:

```ts
export const MIN_REFERENCE_PHOTOS = 5;
export const MAX_REFERENCE_PHOTOS = 20;
```

Ensure `validateReferenceFiles(files)` returns invalid below 5 and above 20 with these exact messages:

```ts
if (files.length < MIN_REFERENCE_PHOTOS) {
  return { valid: false, message: "Choose at least 5 reference photos." };
}
if (files.length > MAX_REFERENCE_PHOTOS) {
  return { valid: false, message: "Choose no more than 20 reference photos." };
}
```

- [ ] **Step 3: Update upload panel copy**

In `UploadReferencePanel.tsx`, replace copy with:

```tsx
<h2 id="upload-reference-title">Upload reference photos</h2>
<p>Share 5-20 clear images. 8-12 varied photos usually trains the best first Soul.</p>
```

Use this dropzone small text:

```tsx
<small>JPG, PNG, WebP or HEIC. Max 15 MB each.</small>
```

- [ ] **Step 4: Replace clone creation form**

In `ClonesScreen.tsx`, remove persona/style/Soul ID fields. Submit a `FormData` payload to `/api/clones/manual-upload` with `displayName` and `photos`.

Required submit body shape:

```ts
const payload = new FormData(event.currentTarget);
await api("/api/clones/manual-upload", {
  method: "POST",
  body: payload
});
```

The form must include:

```tsx
<input name="displayName" placeholder="Soul name" required />
<input name="photos" type="file" accept="image/*" multiple required />
```

- [ ] **Step 5: Keep Instagram visible but disabled in onboarding**

In `OnboardingScreen.tsx`, render the Instagram tab as disabled:

```tsx
<button type="button" className={sourceTabClass(mode, "instagram", "source-tab-instagram")} disabled>
  <AtSign size={18} />
  <strong>Instagram</strong>
  <span>Coming soon</span>
</button>
```

If the Instagram panel still exists, replace its form button with:

```tsx
<button className="primary" disabled type="button">
  Coming soon
</button>
```

- [ ] **Step 6: Update display names**

Replace usages of `clone.name` with `clone.display_name` in:

```text
src/client/screens/ClonesScreen.tsx
src/client/screens/OnboardingScreen.tsx
src/client/screens/CreateScreen.tsx
src/client/screens/BlitzScreen.tsx
src/client/layout/MobileShell.tsx
```

Use fallback:

```ts
clone.display_name || clone.handle || "Mirai Soul"
```

- [ ] **Step 7: Run frontend checks**

Run:

```bash
npm run typecheck
npm run test
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/client
git commit -m "feat: update frontend for manual soul onboarding"
```

---

## Task 13: Cleanup Prototype Server And Final Verification

**Order:** Run after Tasks 1-12 pass.

**Can parallelize:** No.

**Files:**
- Delete: `src/server/**`
- Modify: `tsconfig.json`
- Modify: `package.json`
- Modify: `config/secrets.example.md`

**Acceptance Criteria:**
- No build/runtime path references `src/server`.
- Rust product check passes.
- Frontend typecheck and tests pass.
- Local migrations apply.
- Final `git status --short` shows only intended files.

- [ ] **Step 1: Find prototype server references**

Run:

```bash
rg -n "src/server|server/index|Hono|hono" package.json tsconfig.json vite.config.* wrangler*.jsonc workers src config || true
```

Expected: matches only in deleted files, package dependencies removed in Step 3, or historical docs. Runtime config has zero `src/server` references before Step 2.

- [ ] **Step 2: Delete prototype backend**

Run:

```bash
rm -rf src/server
```

- [ ] **Step 3: Remove unused backend dependencies**

In `package.json`, remove these dependency keys:

```text
@hono/zod-validator
hono
zod
```

Do not remove Better Auth, Polar, React, Vite, Vitest, or TypeScript dependencies.

- [ ] **Step 4: Run complete verification**

Run:

```bash
npm run typecheck
npm run test
npm run product:test
npm run product:check
npm run db:migrate:local
```

Expected: all commands PASS. If any command fails because Cloudflare local resources are missing, record the exact failure and confirm the code-level checks still pass.

- [ ] **Step 5: Run repo status check**

Run:

```bash
git status --short
```

Expected: only planned changes from this task are listed.

- [ ] **Step 6: Commit**

```bash
git add package.json tsconfig.json config/secrets.example.md workers src/client config/d1/migrations
git add -u src/server
git commit -m "chore: remove prototype server after rust split"
```

---

## Parallelization Plan

Run tasks in this order:

1. Task 1.
2. In parallel: Task 2, Task 3, Task 4.
3. After Task 4, in parallel: Task 5, Task 6, Task 7, Task 10.
4. Task 8 after Tasks 3, 5, 6, and 7.
5. Task 9 after Task 8.
6. Task 11 after Tasks 3, 5, and 10.
7. Task 12 after Tasks 6, 8, and 11.
8. Task 13 last.

Disjoint write sets for safe parallel workers:

- Auth Worker: `workers/auth/**`
- D1 schema: `config/d1/migrations/1000_*`, `config/d1/migrations/1001_*`
- Rust domain: `workers/product/src/domain/**`, `workers/product/tests/domain_tests.rs`
- Rust media: `workers/product/src/services/media.rs`, `workers/product/src/routes/media.rs`
- Rust AI: `workers/product/src/ai/**`

If two workers both need `workers/product/src/lib.rs`, `workers/product/src/http/router.rs`, `workers/product/src/routes/mod.rs`, or `workers/product/src/services/mod.rs`, merge through the parent agent after both workers finish.

## Final Verification Checklist

- [ ] `npm run typecheck` passes.
- [ ] `npm run test` passes.
- [ ] `npm run product:test` passes.
- [ ] `npm run product:check` passes.
- [ ] `npm run db:migrate:local` passes or has a documented local D1 setup blocker.
- [ ] `rg -n "persona|voice|style_prompt|stylePrompt" workers/product src/client` has no clone schema/form matches.
- [ ] `rg -n "src/server|server/index" package.json wrangler.jsonc workers src/client` has no runtime references.
- [ ] Manual Higgsfield device auth bootstrap script can produce an authorization URL and instruct the operator to store the refresh token as a Cloudflare Secret.
- [ ] `/api/health` works in `wrangler dev`.
- [ ] `/api/account` returns `401` when signed out.
- [ ] Manual clone upload rejects 4 files, rejects 21 files, and accepts 5 valid image files in local dev.
