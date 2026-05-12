# Mirai 2026 Redesign Review

Reviewed on 2026-05-08 against the current frontend in `src/client`, the
temporary TypeScript/Hono backend prototype, the local `social-page` research
tool, local Higgsfield scripts/skills, Context7 Cloudflare docs results, and
current official Cloudflare docs.

## Verdict

The current frontend is the clearest implemented reference for how the app
works today. Mirai is a Soul-first creator app with a strong mobile shell,
onboarding around Instagram/uploads/Starter Souls/moodboards, a Blitz deck, a
Create surface, active job inbox, library, and account screen.

The existing TypeScript/Hono backend should be treated as a disposable
prototype. It proves useful contracts, but it is not tested as the production
path and should be replaced by Rust on Cloudflare Workers. The next backend
work should port the useful route/schema/queue ideas into Rust rather than
hardening the TypeScript server.

The largest product and technical risk is still provider automation. Higgsfield
CLI is useful locally, but a Cloudflare Worker cannot run a CLI binary or keep a
normal interactive CLI session. The production design needs a provider bridge
or direct API integration, plus an account pool for scaling across multiple paid
Higgsfield accounts.

## Current Frontend Reality

The active app is `AppRouter`, not the legacy shell. `src/client/App.tsx` is
only enabled by `?legacy=1` and can be removed when no longer useful.

Current user-facing surfaces:

- Landing page: product narrative, social proof, how it works, gallery,
  features, testimonials, pricing, and CTA.
- Auth: email/password and Google entry, with onboarding as the post-signup
  destination.
- Onboarding: Instagram harvest, manual upload of 5-15 photos, Starter Soul
  adoption, and exactly 5 inspiration moodboards.
- Blitz: swipe deck of completed generation jobs.
- Create: discovery feed or uploaded inspiration image plus prompt override,
  quality, batch size, and generation submit.
- Inbox: active queued/processing jobs.
- Library: job/output history.
- Me: account identity, usage, plan/billing actions, support, and Pro upsell.
- Clones: manual clone creation and direct Soul ID input.

Frontend types already expect clone Soul source/status, Starter Characters,
Instagram harvest jobs, inspiration moodboards, and app routes for Blitz/Create/
Inbox/Library/Me/Clones/Onboarding.

## Prototype Findings To Carry Forward

The TypeScript backend is not the target, but these ideas are useful:

- D1 tables for clone profiles, media assets, reference assets, discovery,
  generation jobs/outputs, onboarding, moodboards, and starters.
- R2 as the private media store.
- Separate onboarding and generation queues.
- Discovery cache through ScrapeCreators.
- Materializing remote discovery images before generation.
- Higgsfield generation flow: upload input image, submit Soul v2 job, poll,
  fetch raw outputs, persist outputs.
- Current onboarding state machine: Instagram harvest can become
  `ready_for_soul_script`; manual upload and starters also create clones with
  `pending_script` when no provider Soul ID exists.

Do not assume the current Hono CORS/auth/routes are production ready. They are
implementation notes for the Rust rewrite.

## Rust Backend Direction

Build the replacement backend in Rust on Cloudflare Workers using `workers-rs`.

Confirmed Cloudflare fit:

- Rust Workers are supported through `workers-rs`.
- `workers-rs` exposes bindings for D1, R2 Bucket, Queues, and Workers AI.
- Workers AI can be bound with `ai.binding = "AI"` / `"ai": { "binding": "AI" }`.
- Kimi K2.6 is available as `@cf/moonshotai/kimi-k2.6` and supports vision,
  structured outputs, function calling, and a large context window.

The Rust backend should keep the frontend API stable and implement server-side
entitlements from day one.

## Onboarding Adjustments

The onboarding product contract is now:

1. User signs up.
2. User chooses a Soul source:
    - Instagram public profile.
    - Manual upload of 5-15 reference photos.
    - Preset Starter Soul.
3. Mirai generates or loads inspiration moodboards.
4. User selects exactly 5 moodboards.
5. Mirai seeds a dynamic inspiration pool.
6. Clone training runs separately and updates the clone when a Soul ID is ready.

Instagram should not go straight from scraped URLs to clone training. It needs
an AI photo-selection agent that scores public candidate images for clear face,
single subject, adequate resolution, lighting, low screenshot/text/meme content,
and useful variety. Accepted and rejected decisions should be stored with typed
reasons.

Manual uploads should use the same eligibility rules as Instagram candidates.
If Instagram yields fewer than 5 good photos, keep the accepted assets and send
the user to manual upload to top up.

Starter Souls should remain a fast path. A Starter is truly ready only when its
provider config has a valid Soul/custom reference ID. Until then, the UI can
show it as setup pending.

## Clone Limits

Initial clone limits:

- Free users: 1 clone.
- Paid users: 5 clones.

These limits must be enforced before enqueueing clone training, not only in the
frontend. Paid-plan details can evolve, but the backend should return limits and
usage through `/api/account` so the client does not hard-code plan logic.

## Higgsfield Provider Risks

Higgsfield is currently the provider of record, but the integration path needs
careful staging.

Known local tools:

- `.agents/skills/higgsfield-soul-id`: trains Soul Characters through the
  Higgsfield CLI.
- `scripts/create_higgsfield_clone.mjs`: CLI wrapper that emits Mirai
  provider config.
- `scripts/higgsfield_api.py`: direct HTTP generation example using a known
  Soul Character and input image.

Key risk: Cloudflare Workers cannot run the Higgsfield CLI inside the Worker.
Workers are JavaScript/Wasm request/queue handlers; they do not provide a normal
shell, child processes, or a persistent CLI auth environment. CLI automation
therefore needs a separate service runner, or it should be replaced by direct
Higgsfield API calls.

Recommended provider phases:

1. Keep clone training semi-manual or runner-assisted while validating the app
   flow.
2. Add a managed provider runner if CLI remains necessary.
3. Replace CLI with direct HTTP API calls, starting with image generation since
   `scripts/higgsfield_api.py` already demonstrates that path.
4. Discover and port the Soul training API separately.

If user growth requires scale, create multiple paid Higgsfield accounts and
model them as a provider account pool. Track each account's plan, capabilities,
auth state, health, capacity, cooldowns, active leases, and failures. The queue
consumer should assign jobs to healthy accounts and retry safely when an account
is auth-blocked or quota-limited.

The biggest blocker to CLI-based scale is non-interactive authentication for
paid Higgsfield accounts. Test this before relying on CLI automation.

## Queue Model

The Rust backend should use three primary queues.

`clone_training_queue`:

- Long-running Soul creation.
- Separate from image generation because it has different latency and capacity.
- Enforces clone limits.
- Uses accepted references from Instagram/manual uploads.
- Assigns provider accounts.
- Updates `clone_profiles.soul_status` and provider config on completion.

`generation_queue`:

- Image-guided Soul generation from a ready clone and selected visual
  references.
- Produces Blitz images in batches of 10.
- Keeps the next Blitz batch hidden until the 10-image batch is complete.
- Handles submit, delayed poll, output persistence, retries, and credit
  settlement/refunds.
- Uses Higgsfield API/runner initially and should preserve a provider interface
  for later replacement.

`niche_research_queue`:

- Dynamic inspiration-pool refresh.
- Uses selected moodboards and app-wide niches to run research.
- Builds per-user visual reference pools for future image-guided Soul
  generation.
- Feeds Create, Blitz, and eventually marketing automation.
- Should avoid expensive refreshes for inactive users unless needed.

## Blitz Batch Loop

The next backend slice should make Blitz batch-oriented instead of an infinite
single-card stream.

Target behavior:

1. Niche research builds a per-user/per-clone visual reference pool.
2. The generation system selects 10 visual references for the next Blitz batch.
3. The generation queue creates 10 image-guided Soul outputs.
4. The user waits while that batch is generating.
5. Blitz shows the new images only after the batch of 10 is complete.
6. The user swipes through the batch.
7. Right-swiped/saved image metadata influences the next batch of 10.

This turns swipes into taste feedback. The system should store which visual
reference, niche cluster, aesthetic tags, and generation output produced each
right swipe. The next selector should lean toward liked metadata while still
preserving enough variety to avoid repetitive decks.

## Niche Research Integration

The `social-page/pipeline` project is valuable and should be incorporated as
product intelligence.

What it already proves:

- Scraping Reddit and TikTok for niche questions and advice.
- Extracting searchable queries and short knowledge bits with an LLM.
- Clustering knowledge and deepening subtopics.
- Researching high-engagement TikTok visuals.
- Generating carousel assets and product-placement marketing posts.

How it should map into Mirai:

- Onboarding moodboards become dynamic search seeds.
- `niche_research_queue` refreshes user inspiration pools from those seeds.
- Create uses the refreshed pool as better trend/inspiration input.
- Blitz can learn from saved cards and selected moodboards.
- Marketing can keep using `produce.js` and `place-product.js` manually until a
  separate marketing automation path is needed.

Production should not run the Node CLI pipeline inside the Rust Worker. Port the
concepts and prompts, and call ScrapeCreators HTTP APIs from Rust.

## AI Provider Routing

The backend should not hardcode one model or one AI vendor.

Use a small model router by task:

- `photo_selection`
- `persona_moodboards`
- `niche_research`
- `copywriting`
- `moderation`

Supported provider options should be config-driven:

- Workers AI Kimi K2.6 for vision, structured output, and easy Cloudflare
  binding.
- OpenRouter DeepSeek V4 Pro where token cost or text-heavy quality is better.
- OpenCode Go DeepSeek V4 Pro if it exposes a stable HTTP API.

The model router should make endpoint, model ID, API key, timeout, and feature
support configurable. Each call should store model, provider, purpose, status,
and structured result for audit/debugging.

## Monetization Notes

Keep Free useful but bounded:

- 1 clone.
- 10 Soul image generations per day, delivered as one 10-image Blitz batch.
- Watermarked exports.
- Lower generation priority.
- Limited inspiration refresh.

Paid plans should unlock:

- 5 clones.
- Pro: 30 Soul image generations per day at launch, delivered as three 10-image
  Blitz batches.
- Pro target after testing: increase to 50 per day if provider capacity and unit
  economics hold.
- No watermark.
- Higher queue priority.
- More aggressive Blitz/inspiration refresh.
- Video or batch workflows later.

Contextual paywalls should trigger on high-intent actions: remove watermark,
quota exceeded, create another clone, generate more like this, premium export,
or future video.

## Rust Build Priorities

1. Use the current frontend routes and types as API inputs, then adjust them
   intentionally as product requirements evolve.
2. Scaffold Rust Worker with Assets, D1, R2, Queues, and AI binding.
3. Implement account/session/entitlement responses needed by the frontend.
4. Implement clone list/create, media upload/read, and onboarding state.
5. Implement Instagram harvest and AI photo selection.
6. Implement `clone_training_queue` with manual/runner callback first.
7. Implement `generation_queue` using a provider interface.
8. Implement `niche_research_queue` from the `social-page` concepts.
9. Add billing/credit ledger and server-enforced limits.
10. Add stale-job reconciliation, DLQ handling, and deletion/privacy flows.

## Technical Recommendations

- Do not add more product logic to the TypeScript/Hono backend unless it is a
  throwaway experiment.
- Remove the legacy shell when the active mobile/desktop app no longer needs a
  comparison path.
- Keep route responses small and frontend-specific.
- Keep provider automation behind interfaces: `SoulTrainingProvider`,
  `GenerationProvider`, and `AiProvider`.
- Treat Higgsfield account/session health as first-class data.
- Use idempotency keys for queue jobs, provider submissions, credit entries, and
  billing webhooks.
- Store R2 media privately by default.
- Store Instagram consent and photo-selection audit results.
- Make model selection configurable by purpose.
- Add tests around entitlements, clone limits, queue state transitions, credit
  refunds, model-router fallback, and provider-account leasing.

## Source Links

- Cloudflare Rust Workers: https://developers.cloudflare.com/workers/languages/rust/
- Cloudflare WebAssembly runtime: https://developers.cloudflare.com/workers/runtime-apis/webassembly/
- Cloudflare Workers AI bindings: https://developers.cloudflare.com/workers-ai/configuration/bindings/
- Cloudflare Kimi K2.6 model: https://developers.cloudflare.com/workers-ai/models/kimi-k2.6/
- Cloudflare Queues batching, retries, and delays: https://developers.cloudflare.com/queues/configuration/batching-retries/
- Local frontend implementation reviewed: `src/client`
- Local Higgsfield Soul skill: `.agents/skills/higgsfield-soul-id/SKILL.md`
- Local Higgsfield direct generation example: `scripts/higgsfield_api.py`
- Local niche research prototype: `../social-page/pipeline`
