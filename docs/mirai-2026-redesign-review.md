# Mirai 2026 Redesign Review

Reviewed on 2026-05-06 against the current React/Cloudflare codebase, Context7 docs, current vendor docs, and subscription-app monetization research.

## Verdict

The plan is directionally right: mobile-first PWA, mandatory signup, starter credits, inspiration-driven onboarding, daily Blitz, and contextual paywalls are the correct product moves for aspiring IG/TikTok lifestyle creators. The biggest Phase 1 contract is: Instagram URL first, an agent harvests eligible public photos, and that source resolves into a usable Soul Character. Manual uploads and 10 preset Souls are the fallback/alternate paths. The automation script for creating Souls can come later, but each onboarding path must ultimately resolve to a `provider_config_json.customReferenceId` before Mirai can honestly show "me cloned." In the current app, uploaded or harvested reference assets are stored, but the Higgsfield generation provider only uses `provider_config_json.customReferenceId`. If onboarding creates `clone_reference_assets` but never creates or attaches a Soul-ID/custom reference, first Blitz cards will use the default character.

The other big adjustment is monetization posture. Do not make Free too generous. RevenueCat 2026 data strongly supports charging soon after the user understands the core value, annual-plan nudges, and cost-aware AI entitlements. Mirai should preserve a free path, but the main Day 0 goal should be a trial or paid top-up after the first impressive save/share moment.

## Codebase Fit

Current repo strengths:

- `src/server/index.ts` already has the Hono Worker shape, Better Auth routes, Polar webhook passthrough, asset fallback, and queue handler.
- `src/server/generation/provider.ts` already has `image | video` as a domain concept, so Phase 5 can remain provider-extension work.
- `src/server/routes/generations.ts` already materializes discovery items into R2 before queueing, which is the same pattern Blitz should reuse.
- `src/server/services/media.ts` and the D1 helpers are simple and reusable.
- `src/server/discovery/scrapecreators.ts` already normalizes ScrapeCreators feeds and caches by source params.

Current gaps that must be fixed before the roadmap works:

- Add a `provider_identity_jobs` or onboarding job concept that turns accepted IG/upload photos into a Higgsfield Soul Character and writes `clone_profiles.provider_config_json = { customReferenceId }`. This does not require the final automation script on day one; the first implementation can queue/manualize this step while preserving the same state model. Starter Characters can skip creation only if their seed clone already points at one of the 10 pre-created Souls.
- Add a route/service for starter reference asset reads. Current `/api/media/:id` requires ownership, so copied or system-owned starter assets need either duplicated user-owned rows or a read-only sharing policy.
- Block `mode: "video"` before queueing until a video provider exists. Today `generationRoutes` accepts video, but `HiggsfieldProvider.submit()` throws for non-image jobs.
- Add `generation_jobs.kind` or `request_json.kind` conventions before Blitz so the queue consumer can distinguish manual, onboarding, Blitz, retry, and video jobs.
- Replace independent `run()` calls with `env.DB.batch()` where atomicity matters, especially credit debit plus job insert. Cloudflare documents D1 batch statements as transactional and sequential.
- Add stale-job reconciliation. If a queue message exhausts retries or lands in the DLQ, the current job can remain `queued` or `processing`; a scheduled reconciler should mark stale jobs failed and refund idempotently.
- Tighten CORS. The current API CORS origin callback returns `origin || "*"`, while using credentials. Limit origins to `APP_URL`, local dev URLs, and any configured marketing/app host.

## Phase Adjustments

### Phase 0

Keep it first, but treat it as product infrastructure, not just UI cleanup.

- Use `vite-plugin-pwa` with `injectManifest` because Mirai needs custom push and notification-click handling later.
- Keep the legacy shell behind `?legacy=1`, but share API clients and types so the split does not fork business logic.
- Add `/api/ui-config` with server-evaluated flags and paywall copy. PostHog flags are good for experimentation, but server responses should be the client source of truth for gated features.
- Re-scope the install metric. Chrome Android can show install prompts, but iOS/iPadOS require manual Add to Home Screen; measure "eligible install prompt shown" separately from "install education shown."

### Phase 1

Add "Phase 1A.5 Soul Character Resolution" between source selection and bubbles:

1. User pastes an Instagram account URL as the primary path.
2. An onboarding agent harvests eligible public profile photos: clear face, single subject, good lighting, adequate resolution, not meme/screenshot/text-heavy, deduped.
3. If the agent finds enough eligible photos, create media rows, clone reference rows, and move the user into Soul creation/resolution.
4. If Instagram fails, returns too few usable photos, or the profile is private/blocked, send the user to manual uploads with any usable harvested photos pre-filled.
5. If the user wants the fastest path or does not want to upload, let them select one of the 10 preset clone/Soul options available to anyone on the site.
6. Create or attach the Soul Character for that source. Automated Soul creation from IG/manual uploads can be implemented later, but the state machine should already support `harvesting_instagram`, `needs_manual_upload`, `pending_soul`, `soul_ready`, and typed failure/fallback states.
7. Write `provider_config_json.customReferenceId`.
8. Only then enqueue first Blitz.

For manual uploads:

1. User uploads 5-15 photos.
2. Create media rows and clone reference rows.
3. Reuse the same eligibility filter and Soul creation/resolution state machine as Instagram.

Starter Character onboarding should be modeled as selecting from 10 pre-created Souls that you seed later. Each Starter should have a system-owned clone profile, reference assets for display, persona/style metadata, and a ready `customReferenceId`; user adoption copies or references that ready provider config.

For IG harvest, keep the typed failure reasons, but store partial accepted assets and let the user top up manually. Add R2 lifecycle cleanup for `tmp/ig-harvest/*`.

For persona bubbles, store both `selected` and `weight` in `inspiration_bubbles` from day one. Phase 3 taste then becomes an update, not a migration surprise.

For ScrapeCreators, prefer an endpoint registry generated or validated from their OpenAPI specs. Their 2026 changelog shows endpoint versions and pagination behavior changing, especially Instagram search/reels endpoints.

### Phase 2

Implement this before full Blitz scale, and introduce an entitlements contract immediately:

- `GET /api/account` should return `plan`, `balance`, `entitlements`, `limits`, `paywallTriggers`, and `dailyUsage`.
- The client should render plan state, but never calculate allowed actions from hard-coded plan slugs.
- Use Polar's granular webhook handlers (`onCustomerStateChanged`, `onOrderPaid`) for subscription/top-up reconciliation, while keeping the catch-all audit row.
- Add annual prices from launch. Suggested packaging:
  - Pro monthly: $14.99, annual: $99 or $119.
  - Studio monthly: $39.99, annual: $299 or $349.
  - Top-ups: 50, 250, 1000 credits with clear per-credit discount.
- Add a cost floor to credits. A credit should map to the true blended cost of generation, storage, ScrapeCreators, retries, and support margin.

### Phase 3

Do not generate maximum daily decks for every user by default. Daily Blitz is the biggest cost lever.

Use a hybrid deck policy:

- Free: assemble daily deck metadata overnight, generate first 1-2 cards only for users active in the last 7-14 days or with push enabled, and fill the rest on app open.
- Pro: pre-generate more aggressively, but recycle unopened cards for 48 hours.
- Studio: full pre-generation and priority queue.

Add fraud controls before referrals and streak rewards: referral code uniqueness, device/IP heuristics, reward caps, and first-save or first-paid thresholds.

### Phase 4

Move "share-ready export" earlier if possible. Monetization improves when the first saved output immediately hits a valuable gate:

- Free export: watermarked 9:16 with Mirai referral link.
- Pro export: no watermark, HD, brand kit.
- Studio export: batch, scheduler, video, priority variants.

Direct-posting to Instagram should remain out of scope. Clipboard plus Web Share plus deep-link is the right first product.

### Phase 5

Video latency means async-only UX. The existing queue/poll pattern can start here, but use longer backoff, progress copy, push completion, and a Studio-only entitlement gate before queue insert.

## Monetization Improvements

1. Add a Day 0 "first save" paywall. Let the user see the first strong Blitz card, then gate no-watermark export, HD, or "generate 20 more like this." This avoids a cold paywall while still monetizing the first session.
2. Push annual plans from the first paywall. RevenueCat 2026 reports annual plans producing roughly 2x monthly RPI, so the plan toggle should default to annual with monthly visible.
3. A/B test 7-day vs 14-day Pro trials. Longer trials convert better in the benchmark data, but AI generation cost may make 14 days expensive. Tie trial access to monthly credits, not unlimited use.
4. Add a paid "trend pack" surface. Weekly trend drops, seasonal presets, and creator niche packs can be included in Pro/Studio while also creating top-up demand.
5. Make Free viral but bounded: 5 Blitz/day, watermark, lower quality, no batch, no video, no all-starter access, limited regenerate-similar.
6. Create a "Creator Pass" intro offer: $4.99 for 100 credits, no subscription, one-time for users who hit balance zero but reject Pro. This captures payment intent and can later upsell.
7. Add "agency/manager" positioning under Studio later: multiple clones, brand kits, client folders, commercial export history, priority queue, invoice-friendly annual price.
8. Trigger contextual paywalls on high-intent actions, not generic account screens: remove watermark, quota exceeded, regenerate similar after a like, choose Studio-only video, schedule batch, and adopt second Starter.
9. Track refund/chargeback/renewal cohorts, not only paywall conversion. A paywall win that creates poor renewals will hide the real unit economics.

## Technical Recommendations

- Add `credits.reserve()` and `credits.refund()` rather than direct debit only. Reserve on queue insert, settle on successful provider submit or output persistence, refund on terminal failure.
- Use deterministic event keys everywhere: `signup:<userId>`, `job:<jobId>:reserve`, `job:<jobId>:refund`, `polar:<eventId>`, `referral:<refereeId>`.
- Add `credit_ledger.balance_after` but compute balance in the same transactional batch as the insert. Never let the client submit a balance.
- Add `generation_outputs.watermark_state` or store export variants separately, so Free outputs can be generated once and paid exports can be unlocked without rerunning the model.
- Store source consent metadata for IG harvest: handle, user-entered URL, public-only notice accepted timestamp, and deletion status.
- Add account deletion that removes face references, trained-provider IDs where possible, R2 objects, push subscriptions, and discovery materializations.
- Keep generated outputs private by default. Share URLs should be Mirai-controlled redirects or export assets, not only upstream provider share URLs.
- Use `ctx.waitUntil()` in scheduled handlers for cron work and split large cron runs into queue messages instead of doing all daily Blitz work inside a single scheduled event.
- Add tests for entitlement matrices, credit ledger idempotency, queue failure refunds, starter adoption limits, and upload/IG fallback state transitions.

## Research Notes

- Context7 confirmed the current Better Auth Hono pattern: mount `auth.handler` on `/api/auth/*`, configure credentialed CORS, and use `socialProviders` for Google/Apple. Apple also requires `https://appleid.apple.com` in trusted origins.
- Context7 and current docs confirm Polar's Better Auth plugin supports checkout, portal, customer creation on signup, usage, and webhook handlers. Use granular handlers for products and ledger grants, plus catch-all audit logging.
- Context7 and Cloudflare docs confirm Workers Assets SPA fallback and scheduled handlers fit this architecture. Cloudflare D1 `batch()` is the right primitive for transactional multi-statement credit/job updates.
- RevenueCat 2026 supports stronger Day 0 monetization: hard paywalls materially outperform freemium on D14/D60 RPI and D35 conversion, while trial starts are heavily Day 0.
- Web.dev confirms iOS/iPadOS do not provide an install prompt; use manual install education and Apple touch icons.
- PostHog docs support feature flags, remote config payloads, bootstrapping, experiments, and identifying users after login. Keep PII out of event props.
- Sentry docs support `@sentry/cloudflare` with Hono/Workers via `withSentry`.
- Claude API docs list `claude-haiku-4-5-20251001` as the current fastest Claude model with vision support, making it appropriate for persona/bubble analysis when cost matters.
- OpenAI docs list GPT Image 2 (`gpt-image-2`, snapshot `gpt-image-2-2026-04-21`) as a high-fidelity image generation/editing model. The local Higgsfield skill catalog also recommends GPT Image 2 for UI, graphic design, banners, and text-heavy image inspiration, but the Higgsfield MCP model search in this session did not expose a `gpt_image_2` model ID directly.

## Source Links

- RevenueCat State of Subscription Apps 2026: https://www.revenuecat.com/state-of-subscription-apps/
- Cloudflare D1 batch API: https://developers.cloudflare.com/d1/worker-api/d1-database/
- Cloudflare Queues retries and delays: https://developers.cloudflare.com/queues/configuration/batching-retries/
- Cloudflare Workers Vite plugin: https://developers.cloudflare.com/workers/vite-plugin/
- Better Auth Hono integration: https://better-auth.com/docs/integrations/hono
- Better Auth Polar plugin: https://better-auth.com/docs/plugins/polar
- Better Auth Apple provider: https://www.better-auth.com/docs/authentication/apple
- Vite PWA guide: https://vite-pwa-org.netlify.app/guide/
- Web.dev PWA installation: https://web.dev/learn/pwa/installation
- PostHog JavaScript SDK: https://posthog.com/docs/libraries/js
- PostHog feature flags: https://posthog.com/docs/feature-flags
- Sentry Cloudflare SDK: https://docs.sentry.dev/platforms/javascript/guides/cloudflare/
- ScrapeCreators homepage/changelog: https://scrapecreators.com/ and https://scrapecreators.com/changelog
- Higgsfield Soul 2.0: https://higgsfield.ai/soul-intro
- Claude model overview: https://platform.claude.com/docs/en/about-claude/models/overview
- OpenAI GPT Image 2 model: https://developers.openai.com/api/docs/models/gpt-image-2
