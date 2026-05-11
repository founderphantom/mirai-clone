# Mirai Rust Product Backend Design

Date: 2026-05-08

## Status

Design approved in brainstorming. This document defines the backend target for
the first Rust implementation plan. It is intentionally focused on manual clone
onboarding and the product backend core. It is not an implementation plan.

## Product Scope

The first backend milestone is the manual-upload clone onboarding core:

1. An authenticated user uploads 5-20 reference images.
2. The Rust Product Worker stores the images privately in R2.
3. The Rust Product Worker creates a clone profile that represents identity and
   provider state only.
4. The backend enforces clone entitlements server-side.
5. The backend queues Soul training through Higgsfield.
6. The user chooses visual direction bubbles after clone creation.
7. The niche research system begins building visual reference pools for future
   image-guided Soul generation.

Instagram scraping and Instagram photo selection are a later feature. The
frontend may keep an Instagram section visible as a disabled or coming-soon
path, but it must not be wired as a working backend flow in this milestone.

## Runtime Topology

Mirai should run as two Workers plus the existing React client:

- JS Auth Worker: Better Auth, Google/email auth, Polar checkout, Polar portal,
  Polar webhooks, and internal session verification.
- Rust Product Worker: all product APIs, private media, D1 app state, queues,
  AI routing, provider calls, credits, entitlements, and cleanup.
- React client: served as Workers Assets by the Rust Product Worker.

The Rust Product Worker should be the public gateway for the app. Requests to
`/api/auth/*` and `/polar/webhooks` are passed to the JS Auth Worker through a
Cloudflare service binding so the frontend can keep same-origin auth URLs.

The Rust Worker verifies sessions by calling the JS Auth Worker over the
`AUTH_SERVICE` service binding. `workers-rs` exposes service bindings through
`Env::service(...)` / `Fetcher`, so the Rust-friendly contract is HTTP:

```text
POST /internal/session/verify
Cookie: <original user cookies>

200 { "userId": "...", "email": "...", "name": "...", "plan": "...", "entitlements": {...} }
401 { "error": "unauthorized" }
```

The Rust Worker must not depend on Better Auth internals. Better Auth tables are
owned by the JS Auth Worker.

## Project Structure

```text
src/client/
  React app, updated for 5-20 upload onboarding and no clone persona/style fields

workers/auth/
  src/index.ts
  wrangler.auth.jsonc
  Better Auth, Polar, and session verification

workers/product/
  Cargo.toml
  wrangler.product.jsonc
  src/lib.rs
  src/http/
  src/auth_client.rs
  src/db/
  src/routes/
  src/services/
  src/queues/
  src/providers/
  src/ai/
  tests/

config/d1/migrations/
  Shared D1 migrations, Rust-owned for app tables going forward

docs/superpowers/specs/
  Design and planning artifacts
```

The current TypeScript backend under `src/server` is disposable. The
implementation may carry forward only proven Better Auth and Polar mechanics.
Do not port its product routes, schema, queue design, or service logic as source
of truth. Once the two-Worker setup is active, delete or fully disconnect
`src/server`.

## Wrangler Bindings

Product Worker bindings:

- Assets: `ASSETS`, `directory = ./dist/client`, SPA fallback, Worker-first for
  `/api/*` and `/polar/webhooks`.
- D1: `DB`.
- R2: `MEDIA`.
- Queues:
  - producer/consumer `CLONE_TRAINING_QUEUE`
  - producer/consumer `GENERATION_QUEUE`
  - producer/consumer `NICHE_RESEARCH_QUEUE`
  - DLQs for each queue.
- Workers AI: `AI`.
- Service binding: `AUTH_SERVICE` to the JS Auth Worker.
- Vars: app URL/name, environment, moderation level, feature flags, default AI
  routing config, ScrapeCreators defaults.
- Secrets: ScrapeCreators API key and Higgsfield provider token secrets.

Auth Worker bindings:

- D1: same `DB`, limited to Better Auth and billing/auth-owned tables.
- Vars: app URL/name, Polar server, product/plan IDs.
- Secrets: Better Auth secret, Google OAuth credentials, Polar access token,
  Polar webhook secret.

## Data Ownership

Auth-owned tables:

- Better Auth tables: `user`, `session`, `account`, `verification`, and any
  Better Auth plugin tables.
- Auth routes and session issuance are controlled by the JS Auth Worker.
- Polar webhook handling is controlled by the JS Auth Worker.

Rust-owned product tables:

- `accounts`
- `clone_profiles`
- `media_assets`
- `clone_reference_assets`
- `soul_training_jobs`
- `provider_accounts`
- `provider_account_leases`
- `generation_jobs`
- `generation_outputs`
- `credit_ledger`
- `ai_model_invocations`
- `inspiration_bubbles`
- `niche_research_queries`
- `niche_knowledge`
- `visual_reference_candidates`
- `visual_references`
- `user_inspiration_pool`
- `discovery_sources`
- `discovery_items`
- `app_events`
- `feature_flag_overrides`
- `deletion_jobs`

Shared billing audit table:

- `billing_events`: schema lives with app migrations, writes come from JS Auth
  Worker Polar webhooks, and reads/summaries are used by the Rust Product
  Worker through `accounts`.

Instagram-specific tables such as `instagram_harvest_jobs` and
`instagram_candidate_assets` are deferred to a later feature spec.

## Core Schema Direction

`clone_profiles` represents identity and provider state only. It must not have
`persona`, `voice`, or `style_prompt` fields. Soul training does not need these.

Important `clone_profiles` fields:

- `id`
- `user_id`
- `display_name`
- `handle`
- `source`: `manual_upload`, `starter`, `future_instagram`
- `status`: `active`, `archived`, `deleting`
- `soul_status`: `draft`, `queued`, `training`, `ready`, `failed`,
  `provider_action_required`
- `provider`: initially `higgsfield`
- `provider_soul_id`
- `provider_config_json`
- `reference_count_total`
- `reference_count_training_selected`
- `created_at`, `updated_at`, `deleted_at`

`clone_reference_assets` stores the training set:

- `id`, `user_id`, `clone_id`, `media_asset_id`
- `sort_order`
- `role`: initially `identity`
- `eligibility_status`: `accepted`, `rejected`, `needs_review`
- `quality_score`
- `variety_tags_json`
- `training_selected`
- `rejection_reason`
- `audit_json`
- timestamps

Reference upload constraints:

- Exactly the same provider range as Higgsfield Soul training: 5-20 images.
- Guidance should recommend 8-12 varied photos when possible.
- Clear face, eyes visible, single person, no heavy filters or sunglasses.
- Varied angles, lighting, expressions, and distances.
- Sharp images, ideally at least 1024 by 1024.
- JPEG, PNG, WebP, and HEIC can be accepted by the frontend if the backend can
  validate/store them safely.

Visual direction is separate from clone identity:

- `inspiration_bubbles` stores user-selected taste/niche directions.
- `visual_reference_candidates` stores scraped public visual candidates.
- `visual_references` stores accepted public images with verified human
  presence and aesthetic tags.
- `generation_jobs.input_visual_reference_id` points to the image that provides
  aesthetic direction for image-guided Soul generation.
- `generation_jobs.prompt` is nullable/legacy. It is not the core product input.

## API Routes

Rust Product Worker first milestone routes:

```text
GET  /api/account
GET  /api/account/usage
GET  /api/clones
POST /api/clones/manual-upload
GET  /api/clones/:id
DELETE /api/clones/:id
GET  /api/media/:id
GET  /api/onboarding/state
POST /api/onboarding/bubbles/generate
POST /api/onboarding/bubbles
GET  /api/discovery/feed
POST /api/discovery/refresh
POST /api/telemetry/events
GET  /api/telemetry/config
```

Auth/Polar routes are proxied to the JS Auth Worker:

```text
/api/auth/*
/polar/webhooks
/internal/session/verify
```

Future generation routes:

```text
POST /api/generations
GET  /api/generations
GET  /api/generations/:id
POST /api/generations/:id/retry
```

Generation requests should be image-guided: selected clone plus selected visual
reference/input image. Prompt is not required.

## Manual Clone Workflow

`POST /api/clones/manual-upload`:

1. Verify the user through `AUTH_SERVICE`.
2. Parse multipart form with `displayName` and 5-20 `photos`.
3. Enforce entitlements:
   - Free users: 1 active clone.
   - Paid users: 5 active clones.
4. Validate uploads: image-only, size limit, supported media type, hash dedupe.
5. Store each accepted file in private R2.
6. Insert `media_assets`.
7. Insert `clone_profiles` with `soul_status = queued`.
8. Insert `clone_reference_assets` in order with audit metadata.
9. Insert `soul_training_jobs`.
10. Send `clone_training_queue` message.
11. Return clone and training job status.

Frontend changes:

- Remove persona/style prompt fields from clone creation and TypeScript types.
- Upload copy says 5-20 photos, with 8-12 varied references recommended.
- Onboarding may show Instagram as a disabled/coming-soon option.
- Clone status UI shows queued, training, ready, failed, or provider action
  required.

## Queues

Use three queues:

- `clone_training_queue`
- `generation_queue`
- `niche_research_queue`

Each queue should have producer binding, consumer config, bounded retries, and a
DLQ. The first milestone wires `clone_training_queue`. The other queues can be
schema/config ready and implemented in later tasks.

`clone_training_queue` message:

```json
{
  "type": "submit_clone_training",
  "job_id": "train_...",
  "clone_id": "clone_...",
  "user_id": "user_...",
  "idempotency_key": "..."
}
```

The consumer loads the training job, clone, selected references, provider
account lease, and provider credentials. It submits the Soul training request to
Higgsfield. On success it stores the provider Soul ID on `clone_profiles` and
marks the training job complete. On failure it stores a typed failure reason,
releases the provider lease, and either retries or marks terminal failure.

## Higgsfield Provider Strategy

No external CLI runner should be required for the planned backend. Higgsfield
supports an MCP endpoint and OAuth device flow. The provider layer should call
Higgsfield over HTTP with bearer tokens stored in Cloudflare Secrets.

Verified endpoints:

- MCP resource: `https://mcp.higgsfield.ai/mcp`
- Protected-resource metadata:
  `https://mcp.higgsfield.ai/.well-known/oauth-protected-resource`
- Device auth server: `https://fnf-device-auth.higgsfield.ai`
- Device auth OpenAPI: `https://fnf-device-auth.higgsfield.ai/openapi.json`
- Device auth endpoints:
  - `POST /authorize`
  - `POST /token`
  - `POST /refresh`
  - `POST /validate`

The OAuth device flow was verified during design. Tokens were not printed or
persisted. Returned lifetimes were 3600 seconds for access tokens and 604800
seconds for refresh tokens.

Credential rules:

- Store Higgsfield access/refresh material in Cloudflare Secrets only.
- D1 stores provider account metadata and secret reference names, not raw
  tokens.
- Prefer refresh-token secrets as the durable provider credential.
- The Worker can call `POST /refresh` when a short-lived access token is needed.
- If refresh tokens rotate, an admin/maintenance script must update Cloudflare
  Secrets with `wrangler secret put`. The Worker must not write raw tokens to
  D1.

Provider account tables should support multiple paid Higgsfield accounts:

- provider
- account label
- plan/subscription state
- supported capabilities: `soul_training`, `image_generation`, future `video`
- capacity limits
- health state
- cooldowns
- secret reference names
- last auth check
- last successful job

The local CLI was tested successfully for image-guided Soul generation:

```text
higgsfield generate create text2image_soul_v2
  --prompt ""
  --image <input image>
  --soul-id <provider_soul_id>
```

The design treats this behavior as the MVP generation model. The backend should
call Higgsfield over HTTP/MCP with the same semantics: empty prompt, input image
as visual direction, and Soul ID as identity.

## Workers AI Routing

All app-owned model tasks use Workers AI through the `AI` binding. The default
model is Kimi K2.6: `@cf/moonshotai/kimi-k2.6`. Model config must include
capability flags:

- `supports_vision`
- `supports_structured_json`
- `supports_large_context`
- `supports_function_calling`
- `supports_moderation`

Tasks:

- `photo_quality_review`: vision-capable models only.
- `human_presence_detection`: vision-capable models only.
- `bubble_generation`: text model is acceptable.
- `niche_seed_extraction`: text model is acceptable.
- `niche_cluster_expansion`: text model is acceptable.
- `visual_reference_selection`: metadata ranking can use text models, but final
  image verification requires a vision model.
- `moderation`: configurable strictness.

Provider policy:

- Workers AI Kimi K2.6 through the `AI` binding for text, vision, structured
  extraction, moderation, and niche research.
- Do not add OpenRouter, OpenCode, or other external model providers for
  app-owned model calls.
- Higgsfield MCP is separate from app-owned model calls and remains the
  provider path for Soul clone training and image generation.

Every model call inserts `ai_model_invocations` with task, provider, model,
input hash/reference, status, latency, structured result, and error summary.

Moderation:

- `moderation_level` is configurable from 0 to 10.
- 0 means disabled except hard legal/provider constraints.
- 1-3 is light filtering.
- 4-7 is normal app safety/default.
- 8-10 is stricter brand-safe filtering.
- Apply to uploaded references, scraped visual references, and generated outputs
  according to config.

## Niche Research And Visual References

The `social-page` project is concept input, not runtime code. Do not run its
Node CLI pipeline inside the Rust Worker.

Mirai should recreate the useful concepts:

1. Seed from selected user bubbles and inferred niche.
2. Scrape TikTok and Instagram where enabled.
3. Extract queries and knowledge bits with an LLM.
4. Cluster knowledge and generate deeper searches.
5. Research top visual posts in the niche.
6. Require a human in the visual reference image at any angle, crop, or zoom.
7. Select accepted visual references for future image-guided Soul generation.
8. Later, use right-swiped generated images to adjust bubble/niche weights.

Human presence flow:

1. Store scraped candidates in `visual_reference_candidates`.
2. Run cheap metadata/dimension filters first.
3. Run required vision model pass for `human_presence_detection`.
4. Accept only candidates with score above threshold and typed result:
   `human_full_body`, `human_upper_body`, `human_face`, `human_partial`.
5. Reject candidates as `rejected_no_human`, `rejected_low_quality`, or other
   typed reasons.
6. Store accepted rows in `visual_references`.

The visual image provides aesthetic direction. Mirai does not copy the source
image; clothes, background, body shape, and composition can change. The Soul ID
provides identity.

## Entitlements And Credits

Clone entitlements:

- Free users: 1 active clone.
- Paid users: 5 active clones.

Server routes must enforce entitlements before clone creation and before queue
submission. `/api/account` returns plan, limits, and usage so the frontend does
not hard-code plan logic.

Polar plan mapping is owned by the JS Auth Worker and summarized into
`accounts`. It should be updated to match the current frontend plan direction,
not blindly copied from the prototype.

Credits:

- Clone training can be plan-gated, not credit-metered in the first milestone.
- Generation reserves credits before enqueue.
- Generation settles credits on success.
- Generation refunds on failure/cancel.
- `credit_ledger` entries require idempotency keys.

## Reliability And Privacy

Required reliability behavior:

- Idempotency keys for clone upload, queue messages, provider submissions,
  provider callbacks, credit ledger entries, and billing webhook events.
- DLQs for each queue.
- Scheduled stale-job reconciliation for queued/training/generation jobs.
- Provider lease release on terminal success/failure.
- Typed failure reasons for user-visible and operator-visible states.

Required privacy behavior:

- R2 media is private by default.
- Client media reads go through authenticated `/api/media/:id`.
- Provider credentials and tokens are never exposed to the client.
- Provider credentials and tokens are never stored raw in D1.
- Account deletion queues R2 cleanup and deletes/anonymizes app data.
- Public visual reference metadata stores source URLs and audit data; private
  user media remains scoped to the owner.

## Testing Strategy

Design the implementation so the less-capable execution model can test each
piece independently:

- Rust pure unit tests for entitlement logic, idempotency keys, status
  transitions, media path helpers, model routing, and provider lease selection.
- Rust route tests for account, clone upload validation, media authorization,
  and onboarding state.
- Queue tests for clone training state transitions and retry/terminal failure.
- Auth Worker tests for session verification and Polar plan mapping.
- Frontend tests/typecheck for clone type changes and 5-20 upload UI behavior.
- Manual provider verification for Higgsfield token bootstrap and a test
  `text2image_soul_v2` image-guided generation.

## Documentation Sources

- Cloudflare Rust Workers:
  https://developers.cloudflare.com/workers/languages/rust/
- `workers-rs`: https://github.com/cloudflare/workers-rs
- Cloudflare service bindings:
  https://developers.cloudflare.com/workers/runtime-apis/bindings/service-bindings/
- Cloudflare D1 bindings:
  https://developers.cloudflare.com/d1/worker-api/
- Cloudflare R2 Worker API:
  https://developers.cloudflare.com/r2/api/workers/workers-api-usage/
- Cloudflare Queues:
  https://developers.cloudflare.com/queues/configuration/batching-retries/
- Cloudflare Workers AI bindings:
  https://developers.cloudflare.com/workers-ai/configuration/bindings/
- Cloudflare Kimi K2.6:
  https://developers.cloudflare.com/workers-ai/models/kimi-k2.6/
- Cloudflare Worker secrets:
  https://developers.cloudflare.com/workers/configuration/secrets/
- Higgsfield MCP metadata:
  https://mcp.higgsfield.ai/.well-known/oauth-protected-resource
- Higgsfield device auth OpenAPI:
  https://fnf-device-auth.higgsfield.ai/openapi.json
- Local Soul photo guide:
  `.agents/skills/higgsfield-soul-id/references/photo-guide.md`
- Local Higgsfield API experiment:
  `scripts/higgsfield_api.py`
- Local niche research prototype:
  `../social-page/pipeline`
