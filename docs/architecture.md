# Mirai MVP Architecture

Mirai is the application brand. A clone profile is the primary user-owned entity: it holds identity text, voice/style guidance, reference assets, provider configuration, and generation history.

## Current Source Flow

The existing `scripts/higgsfield_api.py` flow is:

1. Refresh a Clerk session into a short-lived Higgsfield JWT.
2. Reserve a Higgsfield media upload slot with `POST /media/batch`.
3. Upload inspiration bytes to the presigned URL and confirm `POST /media/{id}/upload`.
4. Submit Soul v2 image jobs through `POST /jobs/v2/text2image_soul_v2`.
5. Poll `/jobs/{id}/status`, enable share links, fetch raw asset URLs, and download outputs.

Mirai ports that shape into a Worker-safe async pipeline. The API request only creates a `generation_jobs` row and writes a Queue message. The Queue consumer submits to Higgsfield, stores provider job IDs, then re-enqueues delayed poll messages until completion.

## Cloudflare Stack

- Workers: Hono API plus React SPA served with Workers Assets.
- D1: Better Auth tables plus Mirai app tables from `config/d1/migrations`.
- R2: uploaded references, discovery materializations, and generated outputs.
- Queues: generation submission and polling throttle. Per-message delays handle provider polling without blocking requests.
- Workflows: not in the initial scaffold. Add Workflows later if generation becomes a durable multi-branch process with video preparation, review gates, retries across days, or human approval.

The `wrangler.jsonc` bindings follow current Cloudflare guidance to use JSON config for new projects and bind D1, R2, Queues, and Assets from one source of truth.

## Bounded Contexts

- Auth and account: Better Auth email/password sessions with Polar plugin integration points.
- Clone management: clone identity, style, provider config, and reference assets.
- Discovery: ScrapeCreators ingestion, normalization, and TTL cache.
- Media: R2 object writes plus D1 metadata.
- Generation: provider-agnostic job records, queue messages, provider outputs.
- Billing: Polar checkout/portal/webhook hooks and local billing event snapshots.

## Data Model

Core app tables:

- `clone_profiles`: user-owned clone identity and provider config.
- `media_assets`: R2 or remote media metadata for uploaded, discovery, and generated images.
- `clone_reference_assets`: ordered role/weight references per clone.
- `discovery_sources`: normalized ScrapeCreators source cache keys and TTLs.
- `discovery_items`: browsable inspiration thumbnails/images and source metadata.
- `generation_jobs`: queued, processing, completed, failed job state.
- `generation_outputs`: generated assets tied back to jobs.
- `billing_events`: Polar webhook payload snapshots for account state reconciliation.

Better Auth owns its auth tables; Mirai stores user IDs as text to avoid coupling app migrations to auth migration timing.

## Discovery Cache Strategy

Discovery is powered by ScrapeCreators and normalized to image-like inspiration cards. The verified first source is YouTube Shorts trending thumbnails at `/v1/youtube/shorts/trending`. TikTok Trending Feed and Instagram Reels are configured as source adapters with environment-overridable endpoints because ScrapeCreators exposes those families but their docs can move route slugs.

Cache flow:

1. User requests `/api/discovery/feed?source=...`.
2. Worker checks `discovery_sources.expires_at`.
3. If fresh, D1 rows are returned without spending ScrapeCreators credits.
4. If stale, Worker calls ScrapeCreators with `x-api-key`, normalizes items, upserts by `(platform, external_id)`, and sets a default 30-minute TTL.
5. Choosing a discovery item materializes the image/thumbnail into R2 before generation.

## Generation Provider Abstraction

`GenerationProvider` exposes:

- `submit(input)`: provider payload creation and external job submission.
- `poll(providerJobIds)`: status checks and output URL collection.

`HiggsfieldProvider` is the temporary implementation. It accepts either `HIGGSFIELD_JWT` or `HIGGSFIELD_SESSION_ID` plus `HIGGSFIELD_CLIENT_COOKIE` for Clerk token refresh. Clone-level `provider_config_json` can override Higgsfield character/style IDs. The default FUFU IDs are kept in `wrangler.jsonc` vars to preserve the script behavior without hardcoding secrets.

`scripts/higgsfield_session.py` is the admin-owned session bootstrapper. It validates the existing `~/.higgsfield_session` cache, performs the Clerk email-code login only when the cache is missing or expired, and can write or publish the resulting `HIGGSFIELD_SESSION_ID` and `HIGGSFIELD_CLIENT_COOKIE` for all Worker generation jobs. This is a shared provider service session for Mirai, not an end-user credential flow.

Production refresh is handled by `.github/workflows/higgsfield-session-refresh.yml`. The workflow runs every 12 hours, restores the Higgsfield session cache, refreshes/publishes Worker secrets through Wrangler, then saves the cache for the next run. The Higgsfield account password and Gmail OTP access stay in GitHub Actions secrets and are not available to the Worker runtime.

The future Soul-v2-style in-house system should implement the same provider interface and can add a video provider next to image providers without changing user-facing job/history tables.

## Route Structure

- `/api/auth/*`: Better Auth login/signup/session routes.
- `/polar/webhooks`: Polar webhook endpoint handled through Better Auth Polar plugin when configured.
- `/api/account`: account and billing snapshot.
- `/api/clones`: clone CRUD and reference asset linking.
- `/api/media`: upload/list/read media assets.
- `/api/discovery`: source list, feed cache, forced refresh.
- `/api/generations`: submit jobs, list history, inspect outputs, retry failures.

Frontend routes are SPA tabs for Clones, Discovery, Generate, History, and Account.

## Docs Consulted

- Cloudflare Wrangler configuration: https://developers.cloudflare.com/workers/wrangler/configuration/
- Cloudflare Queues retries and delays: https://developers.cloudflare.com/queues/configuration/batching-retries/
- Cloudflare Hono guide: https://developers.cloudflare.com/workers/framework-guides/web-apps/more-web-frameworks/hono/
- Better Auth Hono integration: https://better-auth.com/docs/integrations/hono
- Better Auth Polar plugin: https://better-auth.com/docs/plugins/polar
- Polar webhooks: https://polar.sh/docs/integrate/webhooks/endpoints
- ScrapeCreators API docs: https://docs.scrapecreators.com/
- Higgsfield docs entrypoint: https://docs.higgsfield.ai/v1/image2video
