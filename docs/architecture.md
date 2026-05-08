# Mirai Target Architecture

Status: updated on 2026-05-08.

Mirai is a mobile-first creator app for generating trend-led images of a user's
AI clone. A clone profile is the primary user-owned entity: it holds identity
metadata, reference assets, Soul provider configuration, inspiration taste, and
generation history.

## Current Implementation Context

The current frontend implementation in `src/client` was reviewed to understand
how the app works today. It is a product/context reference for this backend
design, not a replacement for future product requirements.

The existing TypeScript/Hono Worker backend under `src/server` is a temporary
prototype. It is useful as evidence for route shapes, schema ideas, queue
messages, D1/R2 contracts, and Higgsfield experiments, but it is not the target
backend and should not receive major new product work. The production backend
will be rebuilt in Rust on Cloudflare Workers.

The legacy React shell in `src/client/App.tsx` is only reachable with
`?legacy=1` from `src/client/main.tsx`. It is not the active product shell and
can be removed when it no longer helps compare old screens.

## Current Frontend Behavior

The active app shell is `AppRouter` plus `MobileShell`.

Unauthenticated users see:

- Landing page sections from `src/client/screens/landing/*`.
- Auth page at `/signup` and `/login`.
- Google and email/password entry points through the frontend auth client.

Authenticated users see:

- `/onboarding`: Instagram URL, manual upload, Starter Souls, and inspiration
  bubbles.
- `/blitz`: swipe deck of completed generation jobs.
- `/create`: discovery or uploaded inspiration image, prompt override, quality,
  batch size, and generation submit.
- `/inbox`: queued and processing jobs.
- `/library`: generated job/output history.
- `/me`: account, plan, billing, usage, and support surfaces.
- `/clones`: manual clone creation and direct Soul ID entry.

The Rust backend should initially support these active UI paths where they
remain part of the product.

## Target Cloudflare Stack

The backend target is Rust on Cloudflare Workers with the `workers-rs` crate.

Cloudflare bindings needed by the Rust Worker:

- Workers Assets: serve the React SPA.
- D1: auth/account data, clone state, media metadata, inspiration data, jobs,
  credit ledger, billing snapshots, and provider account records.
- R2: uploaded references, accepted Instagram candidates, generated outputs,
  export variants, and niche research materializations.
- Queues: clone training, image generation, and niche research refresh.
- Workers AI: optional first-party model execution via an `AI` binding.
- Secrets/vars: auth secrets, billing secrets, ScrapeCreators key, AI provider
  keys, and provider account credentials/session references.

Workers AI binding is supported in Wrangler with:

```jsonc
"ai": {
  "binding": "AI"
}
```

Cloudflare docs show that Workers can call models through `env.AI.run(...)`.
The Rust docs list `Ai`, `D1Database`, R2 `Bucket`, and `Queue` as available
`workers-rs` bindings.

## Bounded Contexts

- Auth and account: user sessions, profile, account deletion, billing state,
  usage, limits, and support metadata.
- Clone management: clone identity, source, Soul status, provider config,
  reference assets, and plan limits.
- Onboarding: Instagram harvest, upload intake, Starter Soul adoption, bubble
  selection, and inspiration pool seeding.
- Instagram photo selection: candidate collection, AI scoring, accepted/rejected
  audit records, fallback states, and consent metadata.
- Niche research: dynamic research derived from `social-page`, bubble queries,
  visual references, and user inspiration pools.
- Media: R2 storage, signed/private reads, remote materialization, lifecycle
  cleanup, and metadata.
- Generation: provider-agnostic job records, provider account assignment,
  submission, polling, output persistence, retry, and refund/reconciliation.
- Billing and entitlements: Free/Paid limits, credits, top-ups, webhooks, and
  server-side action gates.
- Provider operations: Higgsfield account pool, session health, capacity,
  provider credentials, and migration from CLI/manual flows to direct API.

## API Shape

The Rust backend should expose frontend-oriented routes similar to the current
prototype routes:

- `GET /api/account`: user, plan, entitlements, limits, billing state, usage.
- `GET /api/account/usage`: usage buckets shown on `/me`.
- `/api/auth/*`: final auth implementation path, to be chosen during Rust build.
- `GET/POST /api/clones`: list and create clone profiles.
- `GET /api/onboarding/state`: active clone, latest harvest, bubbles, starters.
- `POST /api/onboarding/instagram`: enqueue public Instagram harvest.
- `GET /api/onboarding/harvest/:id`: harvest progress and accepted assets.
- `POST /api/onboarding/upload`: upload 5-15 reference photos.
- `POST /api/onboarding/starter`: adopt a preset Starter Soul.
- `POST /api/onboarding/bubbles/generate`: generate bubble options.
- `POST /api/onboarding/bubbles`: save selected bubbles and enqueue research.
- `GET /api/discovery/feed`: return cached discovery/inspiration items.
- `POST /api/discovery/refresh`: force refresh where allowed.
- `POST /api/generations`: enqueue image generation.
- `GET /api/generations`: list jobs for Blitz, Inbox, Library.
- `GET /api/generations/:id`: inspect job outputs.
- `POST /api/generations/:id/retry`: retry failed/canceled jobs when allowed.
- `/api/media/*`: upload/read private user media and materialized references.
- `/api/telemetry/*`: app events and server-evaluated config flags.

The exact auth library can change in Rust, but response contracts should remain
small, explicit, and frontend-driven.

## Data Model

Core target tables:

- `users` / auth tables: final auth-owned user/session records.
- `accounts`: plan, billing IDs, usage snapshot, app preferences.
- `clone_profiles`: user-owned clone identity, handle, persona, style prompt,
  source, Soul status, starter ID, provider config, and active/archived state.
- `media_assets`: R2 or remote media metadata for references, harvest
  candidates, discovery items, generated outputs, and exports.
- `clone_reference_assets`: ordered reference assets per clone with role,
  label, weight, and eligibility metadata.
- `instagram_harvest_jobs`: requested handle, candidate count, accepted count,
  status, failure reason, linked clone, raw snapshot, and consent timestamp.
- `instagram_candidate_assets`: per-candidate URL/media asset, AI score,
  accepted/rejected reason, face-quality metadata, and audit payload.
- `starter_characters`: preset personas, display assets, provider config, sort,
  and readiness status.
- `inspiration_bubbles`: generated bubble options, search queries, selected
  flag, weight, and sort order.
- `niche_research_queries`: dynamic research queries derived from selected
  bubbles and app-wide marketing niches.
- `niche_knowledge`: extracted niche insights, source platform, source URL,
  cluster, score, and freshness.
- `visual_references`: style archetypes, thumbnail/materialized image refs,
  descriptions, source URLs, and niche.
- `user_inspiration_pool`: user-specific discovery items linked to bubbles.
- `discovery_sources` and `discovery_items`: cached external discovery feeds.
- `soul_training_jobs`: clone-training job state, queue state, provider account,
  accepted reference assets, provider Soul ID, and error fields.
- `generation_jobs`: image/video job state, provider account, provider job IDs,
  input asset, request JSON, prompt, quality, batch size, and status.
- `generation_outputs`: generated media assets tied back to jobs.
- `provider_accounts`: Higgsfield account inventory, plan/subscription state,
  capacity, health, and secret/session reference.
- `provider_account_leases`: active assignment of provider accounts to clone or
  generation jobs.
- `credit_ledger`: reservations, debits, refunds, top-ups, and idempotency keys.
- `billing_events`: webhook snapshots and reconciliation audit trail.
- `ai_model_invocations`: provider/model, purpose, token/cost estimate, status,
  structured output, and trace ID.

Mirai should store app user IDs as text. Provider credentials and session
cookies must never be exposed to the client.

## Queue Design

### `clone_training_queue`

Purpose: turn accepted Instagram/upload photos into a reusable Higgsfield Soul
Character.

Responsibilities:

- Enforce clone limits before enqueueing.
- Free users may create 1 clone.
- Paid users may create up to 5 clones.
- Validate that enough accepted references exist.
- Select or lease a provider account.
- Submit Soul training through the current provider bridge.
- Poll or receive completion.
- Write `clone_profiles.soul_status = 'ready'` and provider config with
  `customReferenceId` when training succeeds.
- Mark terminal failures with typed reasons and release provider leases.

This is expected to be slower than generation and should have separate retry,
DLQ, capacity, and user-facing status semantics.

### `generation_queue`

Purpose: generate images from an input inspiration image plus a ready Soul.

Responsibilities:

- Enforce credits, plan limits, and clone readiness.
- Materialize remote inspiration images to R2 before provider submission.
- Select or lease a provider account.
- Submit provider jobs.
- Poll provider status using delayed queue messages.
- Persist generated outputs to R2 and D1.
- Retry transient failures and refund/settle credits idempotently.

Generation and clone training should stay separate because they have different
latency, cost, capacity, and failure behavior.

### `niche_research_queue`

Purpose: refresh dynamic inspiration pools from selected bubbles and global
marketing niches.

Responsibilities:

- Convert selected bubbles into search queries.
- Run ScrapeCreators API searches for TikTok, Reddit, Instagram, and YouTube
  where enabled.
- Extract queries, knowledge bits, clusters, visual style archetypes, and
  discovery items.
- Cache source results with TTLs.
- Fill `user_inspiration_pool` and app-wide marketing research tables.
- Avoid running expensive research for inactive users unless needed for Blitz or
  Create.

The `social-page/pipeline` project is the current working prototype for this
research process. Its stages map into production services:

- `seed.js`: scrape niche sources and extract queries/knowledge.
- `expand.js`: cluster knowledge and deepen research.
- `research-visuals.js`: identify high-performing visual styles.
- `produce.js` and `place-product.js`: remain marketing operations for TikTok
  carousel promotion, not core app runtime.

The Rust Worker cannot run the `scrapecreators` CLI directly. Production should
call ScrapeCreators HTTP APIs from Rust and reuse the pipeline's data model and
prompts as design input.

## Higgsfield Provider Strategy

Higgsfield is the main external provider for the MVP.

Current local assets:

- `.agents/skills/higgsfield-soul-id`: local skill for training a Soul Character
  with the Higgsfield CLI.
- `scripts/create_higgsfield_clone.mjs`: CLI wrapper that uploads 5-20 images,
  creates a Soul ID, waits, and emits Mirai provider config.
- `scripts/higgsfield_api.py`: direct HTTP example for image generation with an
  input image and a known Soul Character.

Important constraint: Cloudflare Workers run JavaScript/Wasm, not arbitrary
local binaries. A Rust Worker should not be designed to run the Higgsfield CLI
inside the Worker. CLI-based training is only viable through a separate
operator-controlled runner, such as a local machine, VM, container job, or other
service that can authenticate the CLI and report results back to Mirai.

### Provider Phases

Phase 1: manual or semi-manual CLI bridge.

- Rust Worker queues `soul_training_jobs`.
- An operator/runner reads pending jobs, runs the Higgsfield CLI with the chosen
  paid account, and writes the resulting Soul ID back through an admin endpoint
  or secure database path.
- This validates photo selection, job state, and user UX before deep provider
  automation.

Phase 2: managed provider runner.

- A dedicated runner owns Higgsfield CLI sessions for one or more paid accounts.
- Mirai assigns jobs to provider accounts through `provider_accounts` and
  `provider_account_leases`.
- The runner reports health, capacity, failures, and completed Soul IDs/outputs.
- The Rust Worker remains the public API and authoritative job/account state.

Phase 3: direct Higgsfield API integration.

- Replace CLI dependency with direct HTTP calls modeled after
  `scripts/higgsfield_api.py`.
- Image generation can likely be ported first because the Python script already
  captures upload, generation, polling, sharing, and download flows.
- Soul training direct API still needs discovery and validation.
- Keep the `GenerationProvider`/`SoulTrainingProvider` interface stable so the
  frontend and database do not care whether the provider path is CLI or API.

### Provider Account Pool

If Mirai grows, multiple paid Higgsfield accounts may be needed.

The backend should model provider accounts explicitly:

- `provider_accounts.id`
- `provider = 'higgsfield'`
- plan/subscription tier
- supported capabilities: `soul_training`, `image_generation`, `video`
- capacity settings: max active clone trainings, max active generations,
  cooldowns, daily limits
- health state: healthy, degraded, auth_required, quota_exhausted, disabled
- last auth check and last successful job timestamps
- secret/session reference, not raw credentials in D1

Job assignment should lease one provider account per external job. If an account
fails authentication or hits quota, the job can retry on another healthy account
where safe.

The biggest provider blocker is authenticating paid Higgsfield CLI accounts in a
non-interactive cloud runner. This needs to be tested before relying on CLI
automation for production clone training.

## Instagram Photo Selection Agent

Instagram onboarding should be public-profile only.

Flow:

1. User enters an Instagram URL/handle and accepts public-only scraping notice.
2. Backend queues a harvest job.
3. Harvester collects candidate image URLs and stores candidates in R2/D1.
4. AI photo selector scores each candidate.
5. Accepted photos must satisfy clear face, single subject, adequate lighting,
   enough resolution, low text/meme/screenshot content, and useful variety.
6. If at least 5 are accepted, create/attach clone references and enqueue Soul
   training.
7. If fewer than 5 are accepted, preserve partial accepted assets and send the
   user to manual upload fallback.

Each accepted/rejected decision should be stored with a reason and model trace.
This protects quality, debugging, and user trust.

## AI Model Routing

Do not hardcode one AI model into product logic.

Add a model router with config for:

- provider: `workers_ai`, `openrouter`, `opencode_go`, or future providers.
- endpoint/base URL where applicable.
- model ID/name.
- purpose: `photo_selection`, `persona_bubbles`, `niche_research`,
  `copywriting`, or `moderation`.
- supports vision, structured output, and tool/function calling.
- cost and timeout settings.

Candidate models:

- Workers AI Kimi K2.6: `@cf/moonshotai/kimi-k2.6`; useful for vision,
  structured output, and simple deployment through the `AI` binding.
- OpenRouter DeepSeek V4 Pro: useful when token price or quality is better for
  text-heavy niche research.
- OpenCode Go DeepSeek V4 Pro: treat as a configurable HTTP provider if it
  exposes an OpenAI-compatible or documented custom endpoint.

The Rust backend should expose a small internal interface such as
`AiProvider.run_structured(task, input, schema)` so model changes do not require
rewriting onboarding or research services.

## Discovery And Niche Research

Discovery has two roles:

1. User-facing inspiration in Create and Blitz.
2. Marketing/niche intelligence that helps Mirai produce TikTok content and
   improve trend matching.

The `social-page` project is strong at the second role and should influence the
first. Production should bring over the concepts, not the exact Node CLI
runtime:

- niche configuration
- search terms, hashtags, and subreddit/source lists
- query and knowledge extraction
- clustering and deeper query expansion
- high-engagement visual reference detection
- style archetype records

For app runtime, selected onboarding bubbles should feed `niche_research_queue`.
That queue refreshes the user's inspiration pool and gives Create/Blitz better
dynamic ideas over time.

For marketing operations, `produce.js` and `place-product.js` can keep running
manually from `social-page` until there is a separate marketing automation
backend.

## Entitlements

Initial clone limits:

- Free: 1 clone.
- Paid: 5 clones.

Other entitlement examples:

- Free: limited daily Blitz cards, watermark exports, lower priority queues.
- Pro: more generations, no watermark, higher queue priority, more bubbles and
  inspiration refreshes.
- Studio: larger batches, video when available, more aggressive pre-generation,
  priority support, and commercial/agency features later.

Allowed actions must be enforced on the server. The frontend can render plan
state, but it must not decide whether an action is allowed from hard-coded plan
slugs.

## Reliability And Privacy

- Use idempotency keys for signup, clone training, generation submission,
  credit reservation, refunds, billing events, and provider callbacks.
- Reserve credits before queue insert; settle or refund when the job reaches a
  terminal state.
- Mark stale jobs failed through a scheduled reconciler or DLQ consumer.
- Store generated outputs privately in R2 by default.
- Avoid exposing upstream Higgsfield share URLs as the only durable asset link.
- Store public-Instagram consent metadata and deletion status.
- Account deletion must remove or anonymize references, generated outputs,
  push subscriptions, provider IDs where possible, and R2 objects.
- Keep provider account credentials out of D1 and out of client responses.

## Docs Consulted

- Cloudflare Rust Workers: https://developers.cloudflare.com/workers/languages/rust/
- Cloudflare WebAssembly runtime: https://developers.cloudflare.com/workers/runtime-apis/webassembly/
- Cloudflare Workers AI bindings: https://developers.cloudflare.com/workers-ai/configuration/bindings/
- Cloudflare Kimi K2.6 model: https://developers.cloudflare.com/workers-ai/models/kimi-k2.6/
- Cloudflare Queues batching, retries, and delays: https://developers.cloudflare.com/queues/configuration/batching-retries/
- Higgsfield Soul-ID skill: `.agents/skills/higgsfield-soul-id/SKILL.md`
- Higgsfield direct generation script: `scripts/higgsfield_api.py`
- Local niche research prototype: `../social-page/pipeline`
