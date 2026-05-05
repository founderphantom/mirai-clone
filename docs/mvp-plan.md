# Mirai MVP Implementation Plan

## Phase 1: Foundation

- Install dependencies and run `npm run cf-typegen`.
- Create D1 database and R2 buckets, or let Wrangler auto-provision supported resources from `wrangler.jsonc`.
- Apply `config/d1/migrations/0001_initial_app_schema.sql`.
- Generate Better Auth tables with the current Better Auth migration flow for D1.
- Configure Worker secrets: `BETTER_AUTH_SECRET`, `SCRAPECREATORS_API_KEY`, `POLAR_ACCESS_TOKEN`, `POLAR_WEBHOOK_SECRET`, and temporary Higgsfield session/JWT secrets.
- Run `npm run higgsfield:session` from an admin environment to refresh the shared Higgsfield service session. Use `HIGGSFIELD_AUTO_OTP=1` plus `GAPI` when email-code verification is needed.
- Configure GitHub repository secrets for `.github/workflows/higgsfield-session-refresh.yml` so the shared Higgsfield session is refreshed automatically in production.

## Phase 2: Auth And Billing

- Validate Better Auth email/password signup and signin.
- Enable Polar checkout with `POLAR_PRO_PRODUCT_ID`.
- Confirm `/polar/webhooks` receives signed Polar events and stores `billing_events`.
- Add entitlement checks before generation once product limits are finalized.

## Phase 3: Clone Management

- Expand clone edit forms for persona, voice, style prompt, provider config, and reference roles.
- Add identity/reference upload slots with role labels.
- Add clone archive/restore controls.

## Phase 4: Discovery

- Confirm ScrapeCreators source endpoints in the account dashboard.
- Set endpoint overrides for TikTok/Instagram if their docs slug differs.
- Add source filters by region/query and cache observability.
- Add content attribution and source URL handling in the UI.

## Phase 5: Generation

- Test Queue submission with a configured Higgsfield session.
- Validate R2 materialization for uploaded and discovery inspiration images.
- Add output gallery rendering from `/api/generations/:id`.
- Add retry and cancellation policy.
- Add usage accounting before and after provider submission.

## Phase 6: Future Video Track

- Add `video` mode provider implementation behind the same job tables.
- Store input storyboard/shot metadata in `request_json`.
- Split long video orchestration into Workflows only when polling, branching, or recovery exceeds what delayed Queue messages should manage.

## Acceptance Criteria

- A user can sign up, create multiple clone profiles, upload reference/inspiration images, browse cached discovery cards, submit an image generation, and see job history.
- All media created by the user lands in R2 and is represented in D1.
- Higgsfield is isolated behind `GenerationProvider`.
- Billing has checkout, portal, and webhook integration points ready for product enforcement.
- The first app screen is the Mirai workbench, not a marketing page.
