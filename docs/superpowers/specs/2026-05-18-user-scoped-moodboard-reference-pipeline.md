# Global Moodboard Reference Pipeline

Date: 2026-05-18

Status: Draft for implementation planning.

Supersedes/amends:

- `docs/superpowers/specs/2026-05-14-visual-reference-pipeline-design.md`
- `docs/superpowers/specs/2026-05-17-pipeline-v2-visual-reference-migration-design.md`
- `docs/superpowers/plans/2026-05-17-pipeline-v2-visual-reference-migration.md`

## Product Goal

Moodboard reference discovery should build a shared global supply of high-taste
reference images per moodboard. Users select moodboards from that global system.
Clones consume compatible references from those selected moodboards only after
Soul training is ready.

This separates the pipeline into three scopes:

1. Global moodboard supply discovers, validates, cleans, stores, and refreshes
   reusable images for each app moodboard.
2. User moodboard selection stores a user's persistent visual preference layer.
3. Clone pool build checks whether global references are physically compatible
   with a specific clone, then creates clone-scoped Blitz references.

A failed Soul clone retry must not lose moodboard selections, discovered source
progress, cleaned global images, or the ability to reuse already discovered
references for a new clone.

## Target Model

1. Discovery is global and moodboard-scoped.
   - ScrapeCreators work is not tied to a user or clone.
   - The system discovers new Instagram handles and images whenever global
     supply for a moodboard is below target or stale.
   - Reels search remains the primary handle discovery path.
   - Global discovery rotates search terms, pages, date windows, and handles so
     it does not repeatedly use the same sources.
2. Moodboard selection is user-scoped.
   - Users can select 1 to 10 moodboards.
   - Selection is saved once per user.
   - Saving moodboards does not require a usable active clone.
   - Failed clone retries do not lose selected moodboards.
3. Compatibility and Blitz readiness are clone-scoped.
   - Clone-specific work starts only when a ready clone needs a Blitz pool.
   - The clone pool builder loads global references for the user's selected
     moodboards.
   - It runs clone compatibility for that clone.
   - It stores accepted references in clone-scoped `visual_references`.
   - Blitz continues to read from `visual_references` by `clone_id`.

## Current Logic Being Replaced

The current implementation couples moodboards and niche research to an active
clone:

- `moodboards.clone_id` is `NOT NULL`.
- onboarding loads moodboards for `activeClone.id`.
- saving moodboards requires a usable active clone.
- `NicheResearchMessage` variants all carry `clone_id`.
- ScrapeCreators discovery source reservations include `cloneId` and `runId`.
- `visual_reference_candidates` and `visual_references` are clone-owned.
- candidate uniqueness is scoped by `clone_id`.
- `user_inspiration_pool` is clone-owned and depends on clone-scoped references.
- cleanup and clone compatibility both run before a reference becomes reusable.
- a failed clone can own selected moodboards and partially researched candidates
  that the next clone cannot reuse.

This is the root cause of the retry and scale problem: each clone can trigger
the same ScrapeCreators searches and review the same images again, while a
failed clone can strand useful research artifacts.

## Product Decisions

- Moodboard and niche still mean the same product concept for this pipeline.
- Users can select 1 to 10 moodboards.
- Moodboard IDs should be deterministic by `user_id + slug`, not by
  `user_id + clone_id + slug`.
- App moodboard definitions have stable global slugs and search queries.
- User moodboard rows store selection state only; they do not own discovery.
- Global moodboard references are keyed by moodboard slug.
- Default moodboards are created once per user for selection state.
- `GET /api/onboarding/state` ensures default user moodboard rows exist before
  returning state, so no-clone onboarding can show moodboards immediately.
- Saving moodboards does not require a clone.
- Saving moodboards may enqueue global discovery for underfilled selected
  moodboard slugs.
- Public profile pictures are never production references.
- Static Instagram photos and carousel child images are preferred.
- Videos and reels are skipped as image sources by default. Reels search is
  used to discover owner handles.
- Kimi K2.6 through Cloudflare Workers AI is the only visual analysis model.
- Seedream 5.0 Lite cleanup removes only visible text from images.
- Only cleaned images are stored for reusable reference generation.
- Captions and source text are untrusted metadata. They may be stored for audit
  and filtering, but must not be sent to generation.
- Soul 2.0 generation uses image references only. Text prompts are not part of
  this app's clone generation contract.

## Non-Goals

- Do not copy failed clone moodboard rows onto each retry clone.
- Do not run full Instagram discovery separately for every user or clone.
- Do not make Blitz read directly from global references; Blitz remains
  clone-scoped through `visual_references`.
- Do not use profile pictures, source captions, handle names, or identity claims
  as generation references.
- Do not add the secondary gender compatibility signal yet.
- Do not add manual admin curation as a required v1 dependency.

## High-Level Flow

```text
Background global supply loop or user demand finds underfilled moodboard slugs
  -> Enqueue global moodboard discovery

Global moodboard discovery
  -> Rotate moodboard search terms, Reels pages, and date windows
  -> Discover Instagram owner handles from Reels search
  -> Rotate handles by freshness, usage, yield, and cooldown state
  -> Fetch profile metadata and static posts/carousel children
  -> Normalize globally unique image candidates
  -> Kimi K2.6 visual guardrail, moodboard assignment, and Soul2 reference scoring
  -> Seedream 5.0 Lite text-only cleanup
  -> Cache only cleaned images to R2/media_assets as global reference assets
  -> Store active global moodboard references

User opens onboarding
  -> GET /api/onboarding/state ensures default moodboard selection rows for user
  -> Load user moodboards and current usable active clone independently

User selects 1-10 moodboards
  -> Save selected moodboards for user
  -> Check global active reference counts for selected moodboard slugs
  -> Enqueue global discovery for underfilled selected slugs

Clone becomes ready or Blitz needs a pool
  -> Load current selected user moodboard slugs
  -> If global library cannot supply enough candidate refs, enqueue global discovery and mark clone pool waiting
  -> When global discovery finishes, re-check global references and resume pool build
  -> Run clone compatibility for the specific clone
  -> Insert compatible clone-scoped visual_references
  -> Create or refresh Blitz batch for the clone
```

## Moodboard Selection

The onboarding API should treat moodboards as user-owned selection state:

- `GET /api/onboarding/state` ensures default moodboard rows exist for the user,
  then returns user moodboards regardless of clone state.
- `POST /api/onboarding/moodboards/generate` creates default moodboard selection
  rows for the authenticated user and does not require `cloneId`.
- `POST /api/onboarding/moodboards` saves selected moodboard IDs for the user and
  does not require `cloneId`.
- The existing `cloneId` request field can remain temporarily for frontend
  compatibility, but the backend must ignore it for moodboard persistence.

Validation:

- minimum selected moodboards: `1`
- maximum selected moodboards: `10`
- duplicates are removed after trimming IDs
- selected moodboards must belong to the authenticated user

After saving selected moodboards, the route should:

1. Resolve selected moodboard slugs.
2. Count active global references for each selected slug.
3. Enqueue `EnsureGlobalMoodboardLibrary` for every selected slug below
   `global_refs_per_moodboard_target`.
4. If a ready clone exists, enqueue or refresh clone pool build.

Being below `global_refs_per_moodboard_target` is a top-up trigger, not always a
blocking condition. Clone pool build should continue when the selected
moodboards can provide at least `global_refs_for_pool_min` active global
references in aggregate that have not already been rejected for that clone.
Moodboard diversity is still enforced later by Blitz selection caps when enough
compatible references exist.

Frontend scope:

- The onboarding UI must allow moodboard selection when `activeClone` is `null`.
- The UI must accept 1 to 10 selected moodboards, not exactly 5.
- A failed clone retry banner must not hide or disable moodboard preferences.
- Upload and moodboard selection are independent onboarding surfaces; upload
  creates or retries the Soul clone, while moodboards persist at user scope.

## Global Discovery Inputs

Each app moodboard has global search inputs:

- slug
- title
- vibe summary
- search queries
- optional previously successful handles discovered for that moodboard

Configured manual handle lists are not required for v1. Reels search remains the
primary source of fresh handles because it avoids repeatedly relying on the same
manually curated accounts.

Handle discovery rules:

- Start from selected moodboard search queries.
- Use ScrapeCreators `/v2/instagram/reels/search` to discover owner handles.
- Use Reels results only for owner-handle discovery; do not store reel
  thumbnails as production reference images.
- Rotate search terms per moodboard so repeated runs do not always start from
  the same query.
- Rotate Reels pages and `date_posted` windows where ScrapeCreators supports
  them.
- Add previously successful global handles for the same Kimi-assigned
  moodboard, weighted by acceptance yield and freshness.
- Optionally add one-hop related profiles from `/v1/instagram/profile`, capped
  tightly and only when the profile is public.
- Do not discover from profile pictures.
- Do not expand beyond one related-profile hop in v1.

Default global discovery caps should remain configurable:

- `global_refs_per_moodboard_target`: `25`
- `global_refs_per_moodboard_min_ready`: `5`
- `global_refs_for_pool_min`: `5`
- `global_library_stale_after_hours`: `168`
- `global_discovery_run_stale_after_minutes`: `60`
- `global_insufficient_retry_after_hours`: `24`
- `global_source_failure_retry_after_hours`: `12`
- `clone_pool_stale_after_hours`: `24`
- `clone_pool_run_stale_after_minutes`: `30`
- `instagram_search_terms_per_moodboard`: `2`
- `instagram_reels_pages_per_term`: `1`
- `instagram_reels_date_windows_json`: `["last-month", "last-year"]`
- `instagram_max_handles_per_moodboard_run`: `20`
- `instagram_profiles_per_moodboard_run`: `8`
- `instagram_related_profiles_per_seed`: `2`
- `instagram_max_profiles_per_global_run`: `40`
- `instagram_posts_per_profile`: `12`
- `instagram_pages_per_profile`: `1`
- `instagram_images_per_post`: `3`
- `instagram_candidate_review_limit`: `80`
- `instagram_min_image_width`: `512`
- `instagram_min_image_height`: `512`
- `accepted_refs_per_profile_cap`: `3`
- `max_accepted_refs_per_global_run`: `50`
- `visual_reference_cleanup_retry_limit`: `3`
- `visual_reference_compatibility_retry_limit`: `2`
- `clone_compatibility_reference_limit`: `4`
- `batch_size`: `5`

`clone_compatibility_reference_limit` controls how many clone identity/reference
images are sent into a compatibility review. It is not the number of global
references required for a Blitz batch. `global_refs_for_pool_min` controls
whether a clone pool build has enough global candidates in aggregate across all
selected moodboard slugs to attempt compatibility. It is not a per-slug minimum.
Blitz pool readiness uses `batch_size`.

If one selected moodboard has all available references and other selected
moodboards have none, clone pool build can still proceed when the aggregate
candidate count meets `global_refs_for_pool_min`. The empty or underfilled slugs
should still enqueue global top-up discovery. Diversity caps later prevent
overusing one moodboard when other compatible references exist, but they must not
block a usable partial pool when global supply is uneven.

Stale thresholds:

- A global moodboard library is stale when its
  `global_moodboard_reference_state.last_successful_refresh_at` is older than
  `global_library_stale_after_hours`.
- A global discovery run is stale when it is still in `queued`, `scraping`,
  `reviewing`, or `cleaning` and `updated_at` is older than
  `global_discovery_run_stale_after_minutes`.
- A clone pool is stale when `clone_reference_state.last_usable_pool_at` is
  older than `clone_pool_stale_after_hours`, the selected moodboard hash has
  changed, or the active pool is depleted.
- A clone pool run is stale when it is still in `queued`,
  `waiting_for_global_library`, or `compatibility_reviewing` and `updated_at` is
  older than `clone_pool_run_stale_after_minutes`.

Scheduler triggers:

- A periodic background worker should enqueue `EnsureGlobalMoodboardLibrary` for
  active moodboard definitions whose global library is under target or stale and
  whose `global_moodboard_reference_state.next_retry_at` is null or in the past.
- Opening Blitz or saving moodboards should enqueue clone pool build when a ready
  clone has no pool, a depleted pool, a stale pool, or a selected moodboard hash
  mismatch.

## Source Rotation and De-Dupe

Global discovery must not keep fetching the same images or overusing the same
handle.

Required source tracking:

- `global_moodboard_source_runs`
  - one row per discovery run
  - stores moodboard slug, reason, status, counts, `created_at`, `updated_at`,
    `started_at`, and `completed_at`
- `global_moodboard_search_state`
  - one row per moodboard slug plus search term/date window/page
  - stores last run time, next eligible time, seen result count, and failure
    state
- `global_moodboard_handles`
  - one row per moodboard slug plus normalized handle
  - stores discovery source, related depth, last fetched time, next cursor,
    accepted count, rejected count, fetch count, failure count, cooldown until,
    and status

Handle rotation:

- Prefer handles that have never been fetched.
- Then prefer handles with older `last_fetched_at`.
- Penalize handles with high recent fetch count.
- Penalize handles with low accepted/reference yield.
- Put handles into cooldown after repeated failures or repeated zero-acceptance
  fetches.
- Allow overused handles only when the moodboard is below
  `global_refs_per_moodboard_min_ready` and no fresher handles are available.

Exhaustion gating:

- `EnsureGlobalMoodboardLibrary` must check for eligible source work before
  creating a new run after `insufficient_refs`.
- Eligible work exists when at least one selected search term/page/date-window
  row in `global_moodboard_search_state` has `next_eligible_at IS NULL OR
  next_eligible_at <= now`, or at least one handle row in
  `global_moodboard_handles` has `cooldown_until IS NULL OR cooldown_until <=
  now`.
- When a global run reaches `insufficient_refs`, set
  `global_moodboard_reference_state.next_retry_at` to the earliest eligible
  search/handle time. If no source can produce another attempt, set it to
  `now + global_insufficient_retry_after_hours`.
- Scheduler and demand-triggered `EnsureGlobalMoodboardLibrary` must no-op while
  `status = 'insufficient_refs'`, the library is still below target, and
  `next_retry_at` is in the future.
- Source/provider failures should set source-specific retry times using
  `global_source_failure_retry_after_hours` unless the provider supplies a more
  specific retry-after signal.

Image de-dupe:

- Every candidate must have `source_image_key TEXT NOT NULL`.
- Required identity shape:

```text
instagram:<post-id-or-shortcode>:<image-index-or-child-media-id>
```

- Prefer stable Instagram media IDs and child media IDs when ScrapeCreators
  returns them.
- Fall back to shortcode plus image index only when stable media IDs are
  unavailable.
- Do not include `source_handle` in `source_image_key`; handles can change and a
  post can be rediscovered through another account path.
- Required uniqueness: `UNIQUE(platform, source_image_key)`.
- If the same source image is later rediscovered through another moodboard, do
  not insert a duplicate raw candidate. Append the later discovery to
  `metadata_json.discoveredMoodboardSlugs`.
- One cleaned global reference can be assigned to exactly one primary
  `assigned_moodboard_slug` from Kimi. If later review strongly routes it
  elsewhere, update assignment only through an explicit audit transition.

Do not rely on a multi-column SQLite unique index with nullable fields for
source uniqueness, because SQLite allows duplicate rows when any indexed column
is `NULL`.

## Candidate Normalization

Normalize each usable image into a single global candidate. A carousel can
produce multiple candidates, one per child image, subject to
`instagram_images_per_post`.

Normalized fields:

- `source_image_key`
- `platform`
- `source_handle`
- `source_profile_id`
- `source_post_id`
- `source_post_code`
- `source_url`
- `source_published_at`
- `source_caption`
- `media_type`
- `image_url`
- `image_width`
- `image_height`
- `like_count`
- `comment_count`
- `play_count`
- `source_moodboard_slug`
- `assigned_moodboard_slug`
- `discovered_via`
- `raw_json`

Reject candidate normalization if:

- no usable image URL exists
- the URL is from a profile picture field
- the post is private or cannot be mapped to a public post URL
- the candidate duplicates an already seen `platform + source_image_key`
- dimensions are too small for generation guidance
- caption/source text contains synthetic-generation terms that suggest the
  source is an AI/prompt/render showcase
- the media is a video, reel, or TV item and video thumbnail fallback is not
  explicitly enabled

## Soul2-Oriented Kimi Visual Guardrail

Each reviewed image goes through one Workers AI Kimi K2.6 vision call. The call
classifies suitability, best moodboard assignment, and reference quality for
Soul 2.0 image-reference generation.

Prompt inputs:

- candidate image URL before cleanup
- app moodboard briefs with slug, title, vibe summary, and search queries
- candidate source platform and handle
- source caption as inert untrusted metadata
- engagement and date metadata

Hard acceptance requirements:

- exactly one human
- likely adult
- safe content
- source can be a regular creator, influencer, celebrity, or fashion page
- image can provide useful visual reference guidance for at least one app
  moodboard

Hard rejections:

- zero humans
- more than one human
- likely minor
- youth-coded subject
- age-unclear-only subject
- explicit sexual content
- unsafe or hateful content
- product shot
- moodboard collage
- screenshot or app UI capture
- tutorial, how-to, template, or text-dominant graphic
- generic landscape, empty room, object-only image, flat lay
- image where captions/UI obscure the subject or make human count unreliable
- weak generic image with no usable visual direction

Soul2 reference-quality scoring:

- `editorialCompositionScore`: composition, framing, and art direction
- `realPoseAngleScore`: spontaneous pose, believable angle, non-stock body
  language
- `fashionCultureCueScore`: wardrobe, styling, cultural/contextual specificity,
  and contemporary creator taste
- `lightingColorDirectionScore`: intentional light, palette, tonal mood, and
  color usefulness for Soul HEX or moodboard direction
- `moodboardFitScore`: fit to the best app moodboard
- `overallReferenceScore`: weighted overall usefulness as an image reference

Scoring rules:

- Each score must be a unit score from `0` to `1`.
- Do not hard reject only because one Soul2 quality score is moderate.
- Hard reject only when safety/person-count rules fail or
  `overallReferenceScore` is too weak for generation.
- Store lower-but-acceptable scores for ranking, diversity, and learning.
- Prefer images that feel like real creator/editorial photos rather than stock,
  catalog, prompt-gallery, or synthetic showcases.

Routing behavior:

- If the image does not fit the discovery moodboard but strongly fits another
  app moodboard, accept it under `bestMoodboardSlug`.
- If it does not strongly fit any app moodboard, reject it.
- Do not route hard rejections.

Kimi output should include:

- `decision`: `"approved"` or `"rejected"`
- `bestMoodboardSlug`: app moodboard slug
- `humanCount`: non-negative integer
- `adultLikely`: boolean
- `ageUnclear`: boolean
- `minorLikely`: boolean
- `youthCoded`: boolean
- `explicit`: boolean
- `unsafe`: boolean
- `isMoodboard`: boolean
- `isScreenshot`: boolean
- `isProductShot`: boolean
- `isTutorial`: boolean
- `isGeneric`: boolean
- `instagramPostWorthy`: boolean
- `editorialCompositionScore`: number from `0` to `1`
- `realPoseAngleScore`: number from `0` to `1`
- `fashionCultureCueScore`: number from `0` to `1`
- `lightingColorDirectionScore`: number from `0` to `1`
- `moodboardFitScore`: number from `0` to `1`
- `overallReferenceScore`: number from `0` to `1`
- `pose`: short string
- `scene`: short string
- `lighting`: short string
- `framing`: short string
- `cameraFeel`: short string
- `stylingDirection`: short string
- `colorPalette`: string array
- `fashionCultureCues`: string array
- `compositionNotes`: string
- `rejectionReason`: string or null
- `reason`: short string

Acceptance thresholds:

- hard safety and person-count requirements must pass
- `decision` must be `"approved"`
- `bestMoodboardSlug` must match an active app moodboard slug
- `moodboardFitScore >= 0.72`
- `overallReferenceScore >= 0.70`
- at least two of `editorialCompositionScore`, `realPoseAngleScore`,
  `fashionCultureCueScore`, and `lightingColorDirectionScore` must be `>= 0.62`

Recommended SQL columns on `global_moodboard_references`:

- `editorial_composition_score REAL NOT NULL DEFAULT 0`
- `real_pose_angle_score REAL NOT NULL DEFAULT 0`
- `fashion_culture_cue_score REAL NOT NULL DEFAULT 0`
- `lighting_color_direction_score REAL NOT NULL DEFAULT 0`
- `moodboard_fit_score REAL NOT NULL DEFAULT 0`
- `overall_reference_score REAL NOT NULL DEFAULT 0`
- `pose TEXT`
- `scene TEXT`
- `lighting TEXT`
- `framing TEXT`
- `camera_feel TEXT`
- `styling_direction TEXT`
- `color_palette_json TEXT NOT NULL DEFAULT '[]'`
- `fashion_culture_cues_json TEXT NOT NULL DEFAULT '[]'`
- `composition_notes TEXT`
- `review_json TEXT NOT NULL DEFAULT '{}'`

## Seedream Text Cleanup

Cleanup is global and happens after Kimi visual approval, before storing a
reusable global reference.

Model/tool:

- Higgsfield MCP cleanup tool from `HIGGSFIELD_MCP_CLEANUP_TOOL`
- model from `HIGGSFIELD_MCP_CLEANUP_MODEL`, expected `seedream_5_lite`

Prompt must be exactly:

```text
Remove only the visible text from this image. Keep every non-text part of the image exactly the same.
```

Rules:

- The prompt must not ask Seedream to improve, restyle, beautify, crop, sharpen,
  relight, or otherwise alter the image.
- Retry cleanup up to `visual_reference_cleanup_retry_limit`.
- If cleanup fails after retries, mark the candidate `cleanup_failed` and
  continue searching for replacements.
- Only cleaned images are cached in R2.
- Raw source images are not cached as reusable references.
- If Kimi or Seedream cannot fetch the external source image because the URL is
  expired, returns `403`, returns `404`, times out repeatedly, or no longer
  serves image content, mark the candidate `source_unavailable` and continue
  searching for replacements.
- Do not enable ScrapeCreators `download_media=true` as an automatic retry in
  v1. If source URL expiry becomes common, revisit that as a costed product
  decision.

## Storage Model

Recommended schema:

- `moodboards`
  - user-owned selection rows
  - canonical source of truth for whether a user selected a moodboard
  - `id TEXT PRIMARY KEY`
  - `user_id TEXT NOT NULL`
  - `slug TEXT NOT NULL`
  - `selected INTEGER NOT NULL DEFAULT 0`
  - no required `clone_id`
  - `UNIQUE(user_id, slug)`
- `global_moodboard_definitions`
  - app-owned moodboard catalog
  - `slug TEXT PRIMARY KEY`
  - title, vibe summary, search queries, sort order, active status
  - definitions are synced into each user's `moodboards` rows lazily during
    `GET /api/onboarding/state` and moodboard save
  - new active definitions create unselected user rows
  - disabled definitions stay in `global_moodboard_definitions` for history but
    are hidden from new selection and excluded from future pool builds unless
    already referenced by queued generation work
- `global_moodboard_source_runs`
  - one row per global discovery run
  - stores moodboard slug, reason, status, selected search terms, counts, error,
    `created_at`, `updated_at`, `started_at`, and `completed_at`
- `global_moodboard_search_state`
  - tracks search term/page/date-window rotation per moodboard slug
- `global_moodboard_handles`
  - tracks discovered handles, yield, usage, cursor, cooldown, and failure state
    per moodboard slug
- `global_visual_reference_candidates`
  - global candidate and review records
  - `source_image_key TEXT NOT NULL`
  - stores source metadata, Kimi review, cleanup status, and cleanup attempts
  - uniqueness by `platform, source_image_key`
  - stores source moodboard slug for discovery audit
  - stores assigned moodboard slug after Kimi routing
  - rejected, source-unavailable, cleanup-failed, and review-failed states live
    here
- `global_moodboard_references`
  - cleaned, reusable global references for a moodboard
  - references `global_visual_reference_candidates.id`
  - references global `media_assets.id`
  - stores Kimi visual tags and Soul2 quality scores
  - status values: `active`, `disabled`, `deleted`
- `clone_visual_reference_compatibility`
  - mandatory table for clone/reference compatibility attempts
  - stores one row per `clone_id + global_reference_id`
  - status values: `queued`, `accepted`, `rejected`, `failed`
  - retry fields: `attempt_count`, `last_error_code`, `last_error_message`,
    `next_retry_at`, `last_attempted_at`, `accepted_at`, `rejected_at`
  - stores body-proportion, hair-length, and facial-hair decisions
  - incompatible references are recorded here and must not become Blitz-ready
    `visual_references`
- `clone_pool_waiting_moodboards`
  - indexed wakeup table for clone pools waiting on global moodboard supply
  - one row per `pool_run_id + moodboard_slug`
  - fields: `user_id`, `clone_id`, `pool_run_id`, `moodboard_slug`, `status`,
    `created_at`, `resolved_at`
  - required uniqueness: `UNIQUE(pool_run_id, moodboard_slug)`
  - indexes: `(moodboard_slug, status)`, `(clone_id, pool_run_id)`, and
    `(user_id, status)`
- `visual_references`
  - clone-scoped Blitz-ready references
  - `user_id TEXT NOT NULL`
  - `clone_id TEXT NOT NULL`
  - references `global_moodboard_references.id`
  - stores only compatibility-accepted references and generation usage counters
- `user_inspiration_pool`
  - remains clone-scoped for Blitz
  - references clone-scoped `visual_references`
  - must not point directly at `global_moodboard_references`

Storage key shape for cleaned global references:

```text
global-moodboard-references/<moodboard-slug>/<global-reference-id>/cleaned.<ext>
```

`media_assets` policy for global references:

- v1 must allow global media assets with `user_id = 'global'` and
  `clone_id = NULL`
- clone-scoped `visual_references` point to the global media asset only after
  compatibility acceptance
- do not create user-owned pointer/copy `media_assets` rows in v1

Generation loaders must enforce that clone-scoped `visual_references` were
created by compatibility acceptance for the requested user's clone before using
any global asset. Loader joins must allow `media_assets.user_id = 'global'` only
when all of these are true:

- requested `user_id` owns `visual_references.clone_id`
- `visual_references.user_id` equals requested `user_id`
- `visual_references.global_reference_id` points to an active
  `global_moodboard_references` row
- `clone_visual_reference_compatibility` has `status = 'accepted'` for the same
  `clone_id + global_reference_id`
- `visual_references.media_asset_id = global_moodboard_references.media_asset_id`
- `media_assets.id = global_moodboard_references.media_asset_id`
- `media_assets.clone_id IS NULL`

The existing `niche_cluster` field can keep mirroring `moodboard_slug` in
clone-scoped `visual_references` until Blitz naming is cleaned up.

## Migration Strategy

D1 uses SQLite semantics and cannot cleanly drop a `NOT NULL` constraint from an
existing column. The migration should rebuild affected tables instead of
attempting in-place constraint removal.

Because there are no production users, this migration can be a clean rebuild of
the visual-reference surface. It should still be append-only and deterministic
for local and preview databases.

Recommended migration name:

```text
1009_global_moodboard_reference_pipeline.sql
```

The migration must rebuild or recreate:

- `moodboards`
  - remove required `clone_id`
  - preserve user selection rows by `UNIQUE(user_id, slug)`
- `global_moodboard_definitions`
  - create app-owned catalog rows from the current moodboard seeds
- `discovery_sources`
  - remove clone/user ownership assumptions from global discovery source rows
    or replace with global-specific source tables
- `global_moodboard_source_runs`
- `global_moodboard_search_state`
- `global_moodboard_handles`
- `global_visual_reference_candidates`
- `global_moodboard_references`
- `clone_visual_reference_compatibility`
- `clone_pool_waiting_moodboards`
- `visual_references`
  - keep `clone_id NOT NULL`
  - add `global_reference_id`
  - store only compatibility-accepted Blitz-ready rows
- `user_inspiration_pool`
  - rebuild FKs and uniqueness so clone-scoped pool rows point at
    clone-scoped `visual_references`
- `user_reference_state`
  - stores derived selected moodboard IDs, slugs, and selected moodboard hash
    rebuilt from canonical `moodboards.selected`
- `global_moodboard_reference_state`
  - stores global supply status per moodboard slug
- `clone_reference_state` and `clone_pool_runs`
  - create clone-scoped pool status and pool-run token tables
- dependent indexes and foreign keys for all rebuilt tables

Do not preserve clone-owned candidate rows from failed local test clones unless
a later production migration explicitly requires data backfill.

## Queue Design

Split queue messages into global discovery and clone-scoped pool build.

Global messages:

- `EnsureGlobalMoodboardLibrary { moodboard_slug, reason }`
- `DiscoverGlobalInstagramHandles { moodboard_slug, run_id, search_term,
  date_window, page }`
- `FetchGlobalInstagramProfile { moodboard_slug, run_id, handle,
  discovered_via, related_depth }`
- `FetchGlobalInstagramPosts { moodboard_slug, run_id, handle, discovered_via,
  next_max_id, page }`
- `FetchGlobalInstagramPostDetail { moodboard_slug, run_id, handle,
  discovered_via, source_url }`
- `ReviewGlobalVisualCandidates { moodboard_slug, run_id, limit }`
- `CleanupGlobalMoodboardReference { moodboard_slug, run_id, candidate_id }`
- `FinalizeGlobalMoodboardLibrary { moodboard_slug, run_id, reason }`

User messages:

- No user-scoped queue message is required in v1. Moodboard save updates
  canonical `moodboards.selected`, rebuilds `user_reference_state`, triggers
  underfilled global discovery, and kicks clone pool build synchronously in the
  route handler before returning.

Clone-scoped messages:

- `BuildCloneReferencePool { user_id, clone_id, reason }`
- `RefreshPool { user_id, clone_id, reason }`
- `ValidateCloneCompatibility { user_id, clone_id, pool_run_id,
  global_reference_id }`
- `FinalizeCloneReferencePool { user_id, clone_id, pool_run_id, reason }`

Rules:

- Global discovery messages must not require `user_id` or `clone_id`.
- Global source reservation params must not include `userId` or `cloneId`.
- `EnsureGlobalMoodboardLibrary` is the only global message that does not carry
  a `run_id`.
- `EnsureGlobalMoodboardLibrary` creates, reuses, or supersedes the current
  `global_moodboard_source_runs` row before enqueueing downstream work, stores
  the selected run ID as `global_moodboard_reference_state.current_run_id`, and
  passes that `run_id` to every downstream global message.
- Every downstream global message must carry the same global `run_id`.
- `BuildCloneReferencePool` and `RefreshPool` are kickoff messages and do not
  carry `pool_run_id`.
- `BuildCloneReferencePool` and `RefreshPool` reuse a nonstale active current
  pool run when `clone_reference_state.current_pool_run_id` points to a
  `clone_pool_runs` row for the same clone in `queued`,
  `waiting_for_global_library`, or `compatibility_reviewing`, the selected
  moodboard hash still matches, and the run is not stale.
- Duplicate kickoff messages for a nonstale active pool run must not create
  another run. They may enqueue missing downstream nudges only through
  idempotent message reservation.
- `BuildCloneReferencePool` and `RefreshPool` create a new `clone_pool_runs` row
  only when there is no reusable nonstale active run, then store it as
  `clone_reference_state.current_pool_run_id`, and pass that `pool_run_id` to
  every downstream clone-pool message.
- `ValidateCloneCompatibility` and `FinalizeCloneReferencePool` must carry
  `pool_run_id`.
- Global handlers must verify the run is still current for that moodboard before
  updating global-visible status.
- Queue messages from stale global runs must be acked after recording
  `stale_run` or equivalent audit state.
- Clone-scoped handlers must never run Instagram discovery directly.
- If selected moodboards do not have enough active global references to attempt
  compatibility, clone-scoped handlers enqueue `EnsureGlobalMoodboardLibrary`
  for underfilled slugs, mark the clone pool run
  `waiting_for_global_library`, and defer or retry pool build after the global
  library changes.
- If global discovery is exhausted, selected moodboards have fewer than
  `global_refs_for_pool_min` active references, and at least one active global
  reference exists, clone pool build should attempt compatibility with the
  smaller candidate set so Blitz can reach `partial_pool_ready` when possible.
- If selected moodboards are below the ideal target but still have at least
  `global_refs_for_pool_min` active not-yet-rejected references, clone pool build
  should proceed while global discovery runs as background top-up.
- `FinalizeGlobalMoodboardLibrary` is responsible for resuming deferred clone
  pool work. After a current global run finishes, it must inspect ready,
  nonfailed clones in `waiting_for_global_library` for users who selected that
  moodboard slug through `clone_pool_waiting_moodboards`, not by scanning JSON,
  and create/enqueue fresh `BuildCloneReferencePool` messages when the global
  library has at least one active reference for the current selected moodboards.
- If `FinalizeGlobalMoodboardLibrary` finds waiting ready clones but the global
  library still has zero active references for selected moodboards and no
  retryable global discovery work remains, it should mark those current pool
  runs `insufficient_refs` rather than waiting forever.
- `clone_pool_waiting_moodboards` transition rules:
  - insert `waiting` rows when a current clone pool run enters
    `waiting_for_global_library`
  - mark rows `resumed` when `FinalizeGlobalMoodboardLibrary` enqueues a fresh
    `BuildCloneReferencePool` for that clone
  - mark rows `insufficient` when discovery is exhausted and no global
    references are available for the selected moodboards
  - mark rows `superseded` when `clone_reference_state.current_pool_run_id` no
    longer equals the row's `pool_run_id`, the clone is no longer ready, or the
    selected moodboard hash has changed
- Workers AI, ScrapeCreators, Seedream, image fetch, R2, and D1 failures must
  not panic the Worker.
- Per-candidate failures are recorded and do not fail the whole discovery run.
- Per-source failures are recorded and do not fail the whole discovery run
  unless no sources succeed.

## Status Storage

Do not store global discovery status in `clone_profiles.provider_config_json`.
A failed clone must not own global supply status.

Recommended status storage:

- `global_moodboard_reference_state`
  - `moodboard_slug`
  - `current_run_id`
  - `status`
  - active reference count
  - target reference count
  - underfilled boolean
  - `next_retry_at`
  - `created_at`
  - `updated_at`
  - `last_successful_refresh_at`
  - `last_ready_at`
  - `last_underfilled_at`
  - `last_insufficient_at`
- `global_moodboard_source_runs`
  - `id`
  - `moodboard_slug`
  - `status`
  - `reason`
  - selected search terms/date windows snapshot
  - discovered handle count
  - candidate count
  - approved count
  - cleaned count
  - `error_code`
  - `error_message`
  - `created_at`
  - `updated_at`
  - `started_at`
  - `completed_at`
- `user_reference_state`
  - `user_id`
  - `selected_moodboard_ids_json`
  - `selected_moodboard_slugs_json`
  - `selected_moodboard_hash`
  - derived cache rebuilt from canonical `moodboards.selected` rows
  - `created_at`
  - `updated_at`
- `clone_reference_state`
  - `user_id`
  - `clone_id`
  - `current_pool_run_id`
  - `selected_moodboard_hash`
  - `status`
  - compatibility counts
  - optional waiting moodboard slugs snapshot for diagnostics only
  - `created_at`
  - `updated_at`
  - `last_usable_pool_at`
  - `last_ready_at`
  - `last_partial_ready_at`
  - `last_insufficient_at`
- `clone_pool_runs`
  - `id`
  - `user_id`
  - `clone_id`
  - `status`
  - `reason`
  - `selected_moodboard_ids_snapshot_json`
  - `selected_moodboard_slugs_snapshot_json`
  - `selected_moodboard_hash`
  - optional waiting moodboard slugs snapshot for diagnostics only
  - compatibility counts
  - `error_code`
  - `error_message`
  - `created_at`
  - `updated_at`
  - `started_at`
  - `completed_at`
- `clone_pool_waiting_moodboards`
  - canonical wakeup index for waiting clone pools by moodboard slug
  - queried by `FinalizeGlobalMoodboardLibrary`
  - status values: `waiting`, `resumed`, `insufficient`, `superseded`

Status values:

- global discovery: `queued`, `scraping`, `reviewing`, `cleaning`,
  `library_ready`, `underfilled`, `insufficient_refs`, `discovery_failed`
- clone pool: `queued`, `waiting_for_global_library`,
  `compatibility_reviewing`, `pool_ready`, `insufficient_refs`,
  `partial_pool_ready`, `pool_failed`

Readiness thresholds:

- Global counts use active `global_moodboard_references` for the moodboard slug.
- `library_ready`: the moodboard has at least
  `global_refs_per_moodboard_target` active cleaned global references.
  Set `last_ready_at` and `last_successful_refresh_at`.
- `underfilled`: the moodboard has at least one active cleaned global reference,
  but fewer than `global_refs_per_moodboard_target`.
  Set `last_underfilled_at` and `last_successful_refresh_at`; do not update
  `last_ready_at`.
- `insufficient_refs`: global discovery has exhausted configured sources,
  retryable candidate work, and cleanup retries, and the moodboard has zero
  active cleaned references.
  Set `last_insufficient_at` and `next_retry_at`; do not update
  `last_successful_refresh_at`.
- `discovery_failed`: an infrastructure or provider failure prevents the run
  from making progress and no retryable queue work remains. If the run merely
  found no usable references after successful processing, use `insufficient_refs`
  instead.
- Clone-pool counts use clone-scoped `visual_references` for currently selected
  moodboard slugs and the current clone.
- `pool_ready`: compatible active clone-scoped references for selected
  moodboards are greater than or equal to `batch_size`.
  Set `last_ready_at` and `last_usable_pool_at`.
- `partial_pool_ready`: at least one compatible active clone-scoped reference is
  available for selected moodboards, but the count is below `batch_size`.
  Set `last_partial_ready_at` and `last_usable_pool_at`; do not update
  `last_ready_at`.
- `waiting_for_global_library`: the selected moodboards have fewer than
  `global_refs_for_pool_min` active not-yet-rejected global references for this
  clone, and one or more selected moodboards have an active or queued global
  discovery run.
- `insufficient_refs`: no compatible active clone-scoped references are
  available after compatibility work is exhausted, and no global discovery work
  is queued or in progress for the selected moodboards.
  Set `last_insufficient_at`; do not update `last_usable_pool_at`.
- `pool_failed`: an infrastructure or provider failure prevents pool build from
  making progress and no retryable queue work remains.

## Selection State, Run Tokens, and Staleness

`selected_moodboard_hash` must be deterministic: SHA-256 of the JSON array of
selected moodboard slugs sorted lexicographically, encoded with no extra fields.
Clone pool builds must use the user's current selected moodboard slugs, not a
stale snapshot from an older selection.

`moodboards.selected` is the canonical source of truth for user moodboard
selection. `user_reference_state.selected_moodboard_ids_json`,
`user_reference_state.selected_moodboard_slugs_json`, and
`user_reference_state.selected_moodboard_hash` are derived cache fields for fast
reads and run-token comparisons. Moodboard save must update the selected rows and
the derived `user_reference_state` row in the same transaction or in one
idempotent write sequence that can be safely replayed. If the cache is missing
or suspected stale, rebuild it from `moodboards.selected`.

Global run behavior:

- `EnsureGlobalMoodboardLibrary` reuses the current run when
  `global_moodboard_reference_state.current_run_id` points to a run for the same
  moodboard in `queued`, `scraping`, `reviewing`, or `cleaning` and that run is
  not stale.
- Duplicate `EnsureGlobalMoodboardLibrary` messages for a nonstale active run
  must not create another run. They may enqueue missing downstream nudges only
  through idempotent source/message reservation.
- If the current status is `insufficient_refs`, the library is below target, and
  `global_moodboard_reference_state.next_retry_at` is in the future,
  `EnsureGlobalMoodboardLibrary` must no-op after recording a skipped/blocked
  audit event.
- `EnsureGlobalMoodboardLibrary` creates a new run when there is no nonstale
  active current run and the global library is below target, stale, explicitly
  requested by a waiting clone, or the previous run is terminal.
- `EnsureGlobalMoodboardLibrary` supersedes a stale active run by creating a new
  run and moving `global_moodboard_reference_state.current_run_id` to the new
  run.
- Creating a new run stores its ID as
  `global_moodboard_reference_state.current_run_id`.
- When a stale active run is superseded, old messages may finish per-candidate
  audit writes but must not update current global-visible status.
- Downstream global messages update global-visible status only when their
  `run_id` still matches `global_moodboard_reference_state.current_run_id` for
  that moodboard slug.
- If a newer global run supersedes an older run, old messages may finish
  per-candidate audit writes, but they must not mark the current global library
  `library_ready`, `underfilled`, `insufficient_refs`, or `discovery_failed`.

Clone pool behavior:

- `BuildCloneReferencePool` and `RefreshPool` reuse the current pool run when
  `clone_reference_state.current_pool_run_id` points to a run for the same clone
  in `queued`, `waiting_for_global_library`, or `compatibility_reviewing`, the
  selected moodboard hash still matches, and that run is not stale.
- Duplicate clone pool kickoff messages for a reusable active run must not
  create another run. They may enqueue missing downstream nudges only through
  idempotent message reservation.
- `BuildCloneReferencePool` and `RefreshPool` create a new `clone_pool_runs` row
  only when no reusable active run exists, then store its ID as
  `clone_reference_state.current_pool_run_id`.
- Downstream clone pool messages update clone-visible pool status only when
  their `pool_run_id` still matches
  `clone_reference_state.current_pool_run_id`.
- If a user changes moodboard selection while older compatibility messages are
  still running, old messages may finish per-reference compatibility audit
  writes, but they must not mark the current clone pool `pool_ready`,
  `partial_pool_ready`, `waiting_for_global_library`, `insufficient_refs`, or
  `pool_failed`.

## Clone-Specific Pool Build

The clone-specific stage runs when Blitz needs references for a clone:

- after Soul training becomes ready
- when a user opens Blitz and no active pool exists
- when a pool is depleted or stale by `clone_pool_stale_after_hours`, selected
  moodboard hash mismatch, or no remaining selectable references
- after user moodboard selection changes and a ready clone exists
- after global discovery completes for a selected underfilled moodboard

Inputs:

- user ID
- clone ID
- clone reference images or Soul metadata needed for compatibility review
- selected active user moodboard slugs
- active global references for those moodboard slugs

Compatibility checks:

- similar body proportions
- similar hair length
- facial hair

Gender is intentionally not part of the v1 compatibility signal.

The compatibility prompt should reject references that conflict strongly with
body proportions, hair length, or facial hair. If those are acceptable, gender
differences should not be used as a rejection reason.

If the global library cannot supply enough candidates for pool build:

1. Determine whether selected moodboard slugs have at least
   `global_refs_for_pool_min` active global references in aggregate that have
   not already been rejected for this clone.
2. Enqueue `EnsureGlobalMoodboardLibrary` for selected slugs below
   `global_refs_per_moodboard_target`.
3. Insert `waiting` rows into `clone_pool_waiting_moodboards` for the selected
   slugs that need global supply.
4. Mark the current clone pool run `waiting_for_global_library`.
5. Stop clone compatibility work until global discovery finalizes or the pool
   run is superseded.
6. When global discovery finalizes, re-check global active reference counts and
   enqueue a fresh clone pool run if references are available.

If no more global discovery work is possible but at least one active global
reference exists for selected moodboards, attempt compatibility with that smaller
candidate set and allow `partial_pool_ready`. If zero active references exist and
no global discovery work is queued or retryable, mark the pool run
`insufficient_refs`.

If the selected moodboards have enough candidates to attempt compatibility but
one or more slugs are still below `global_refs_per_moodboard_target`, enqueue
global discovery as background top-up and continue compatibility work.

Outputs:

- insert compatibility attempts into `clone_visual_reference_compatibility`
- insert only compatible rows into clone-scoped `visual_references`
- copy or reference Kimi visual tags from `global_moodboard_references`
- set `visual_references.media_asset_id` to the compatible cleaned reference
  asset where `media_assets.user_id = 'global'` and `media_assets.clone_id IS NULL`
- set `visual_references.clone_id` to the target clone
- mark incompatible rows in `clone_visual_reference_compatibility` so the same
  clone/reference pair is not retried repeatedly and never appears in Blitz
  selection

Compatibility retry transitions:

- missing row -> insert `queued` with `attempt_count = 0`
- `queued` -> provider call starts by incrementing `attempt_count` and setting
  `last_attempted_at`
- provider success and compatibility accepted -> `accepted`, set `accepted_at`,
  clear retry error fields
- provider success and compatibility rejected -> `rejected`, set `rejected_at`,
  clear retry error fields
- retryable provider/infrastructure failure with attempts remaining -> `failed`,
  set `last_error_code`, `last_error_message`, and `next_retry_at`
- nonretryable failure or attempts exhausted -> `failed`, set
  `next_retry_at = NULL`
- pool builders may retry only `failed` rows whose `next_retry_at` is not null
  and is in the past
- `rejected` and `accepted` rows are terminal for that `clone_id +
  global_reference_id`

Idempotency:

- `UNIQUE(clone_id, global_reference_id)` is required on
  `clone_visual_reference_compatibility`.
- `UNIQUE(clone_id, global_reference_id)` is also required on clone-scoped
  `visual_references`.
- Re-running pool build for the same clone should add missing compatible refs,
  not duplicate existing ones.

## Moodboard Selection Changes

When a user changes moodboard selection, the system must prevent deselected
moodboards from feeding future Blitz batches for every clone owned by that user.

Required behavior:

- global `global_moodboard_references` remain active; they are shared app
  supply and can be used by other users or by the same user if reselected later
- clone-scoped `visual_references` for deselected moodboards must be excluded
  from future Blitz selection without breaking already queued generation jobs
- pool rebuild should add compatible references for newly selected moodboards
- currently queued generation jobs may finish, but newly created Blitz batches
  must use only currently selected moodboards

Required implementation approach:

- Do not set `visual_references.status = 'disabled'` during moodboard save.
- Existing generation loading treats `visual_references.status = 'active'` as
  generation eligibility. Changing that status can break already queued
  generation jobs that reference an active visual reference.
- Keep generation eligibility separate from Blitz selection eligibility.
- Make `load_visual_references_for_selection` join the user's selected
  moodboards and exclude deselected moodboards for newly created batches.
- If the implementation needs stored state, add a separate
  `selection_status` or `selection_eligible` field. Do not overload
  `visual_references.status` for moodboard deselection.

## Blitz Learning Behavior

Blitz continues to learn from clone-scoped `visual_references` and swipes.

Each generated Blitz image should retain:

- `visual_reference_id`
- `global_reference_id`
- `moodboard_slug`
- source platform
- Kimi visual tags: pose, scene, lighting, framing, camera feel, styling
  direction, color palette, fashion/culture cues, composition notes
- Soul2 quality scores

Storage:

- `generation_jobs.input_visual_reference_id` remains the primary generation
  reference pointer.
- `global_reference_id` is derived through
  `visual_references.global_reference_id` for generation history and analytics.
- `blitz_swipes.output_metadata_json` should snapshot `globalReferenceId`
  alongside `visualReferenceId` so swipe learning survives later reference-row
  changes.
- Do not add a redundant `global_reference_id` column to `generation_outputs`
  unless a later query path needs it for performance.

Likes increase future selection weight for similar moodboards and visual tags.
Dislikes decrease them. Diversity caps still apply, so the next batch does not
overuse one handle, one moodboard, or one visual pattern.

Reference selection caps:

- no more than 2 references from the same handle per Blitz batch
- no more than 2 references from the same moodboard per batch until all selected
  moodboards have been represented when possible
- reuse references only when the pool is too small or the user has liked that
  visual direction

## Generation Contract

Generation uses the cleaned media asset from the clone-scoped
`visual_references.media_asset_id`.

Generation guidance passed to Higgsfield should include:

- the cleaned reference image
- clone Soul ID
- selected aspect ratio based on reference image dimensions
- 4K quality setting when the Higgsfield tool supports it
- visual cues from Kimi review
- Soul2 reference-quality tags and scores

Generation guidance must exclude:

- source captions
- source post text
- source identity claims
- handle names
- requests to copy face, exact clothing, exact outfit, exact background, unique
  marks, or likeness

Generation and Blitz loaders must enforce ownership before using a reference:

- requested `user_id` owns the clone
- `visual_references.user_id` equals requested `user_id`
- `visual_references.clone_id` equals requested clone ID
- `visual_references.global_reference_id` points to an active global reference
- `clone_visual_reference_compatibility.status = 'accepted'` for the same
  `clone_id + global_reference_id`
- `media_assets.id = global_moodboard_references.media_asset_id`
- `media_assets.user_id = 'global'`
- `media_assets.clone_id IS NULL`

## Failed Clone Retry Behavior

When Soul training fails:

- the failed clone remains visible with its training failure reason
- the failed clone does not count toward active clone limits
- onboarding keeps moodboards available because moodboard selection is
  user-scoped
- the user can upload a fresh reference set
- the new clone can reuse the user's selected moodboards and the global
  moodboard reference library
- clone compatibility runs again for the new clone before Blitz generation

This avoids copying clone-owned data from failed clone A to retry clone B.

## Test Plan

Unit and domain tests should cover:

- moodboard IDs are deterministic by `user_id + slug`
- onboarding loads moodboards without an active clone
- `GET /api/onboarding/state` creates default moodboards when missing
- saving moodboards does not require a clone
- moodboard selection accepts 1-10 and rejects 0 or 11+
- selected moodboards must belong to the authenticated user
- failed clone retries preserve selected moodboards
- frontend allows moodboard selection without an active clone
- frontend accepts 1-10 selected moodboards, not exactly 5
- `EnsureGlobalMoodboardLibrary` has no input `runId` and creates or reuses the
  current global run before enqueueing downstream messages
- duplicate `EnsureGlobalMoodboardLibrary` messages reuse nonstale active runs
  and supersede terminal or stale runs
- global discovery queue messages serialize without `userId` or `cloneId`
- `BuildCloneReferencePool` and `RefreshPool` serialize without `poolRunId` and
  create the clone pool run before downstream work
- downstream clone-scoped pool messages serialize with `userId`, `cloneId`, and
  `poolRunId`
- global library stale and clone pool stale thresholds trigger the configured
  refresh behavior
- `moodboards.selected` is canonical selection state and `user_reference_state`
  is rebuilt from it as a derived cache
- global moodboard definitions sync new active definitions into existing users
  as unselected rows
- disabled global moodboard definitions are hidden from new selection and
  excluded from future pool builds
- global discovery source params do not include user or clone identifiers
- Reels search is used for owner-handle discovery
- Reels thumbnails are not stored as production references
- global search state rotates moodboard search terms, pages, and date windows
- global handle selection prefers fresh and high-yield handles
- global handle selection cools down repeated failures and repeated zero-yield
  handles
- overused handles are skipped unless the moodboard is starved
- Instagram profile normalizer never emits profile pictures as candidates
- user posts normalizer extracts static post image candidates
- carousel post enrichment extracts child images from `/v1/instagram/post`
- videos are skipped unless fallback thumbnails are explicitly enabled
- candidate uniqueness is once per `platform + source_image_key`, not per user,
  clone, moodboard, or handle
- `source_image_key` is stable across handle changes and does not include
  `source_handle`
- duplicate rediscovery appends audit metadata instead of inserting duplicate
  candidates
- Kimi review accepts only one likely adult with safe content
- Kimi review rejects moodboards, screenshots, product shots, tutorials, generic
  images, no-human images, multi-human images, minors, and unsafe images
- Kimi scoring stores editorial composition, pose/angle, fashion/culture,
  lighting/color, moodboard fit, and overall reference scores
- Kimi review requires the exact Soul2 score fields and acceptance thresholds
- Kimi review does not hard reject solely because one Soul2 quality score is
  moderate when overall reference value is acceptable
- off-style but strong images are assigned to another app moodboard
- Seedream cleanup prompt matches the exact text-only prompt
- expired or forbidden external source image URLs mark candidates
  `source_unavailable` and search continues
- cleanup retries are capped and failed candidates do not block replacements
- only cleaned images are cached to R2
- saving moodboards enqueues global discovery for selected underfilled slugs
- `global_refs_for_pool_min` is aggregate across selected moodboard slugs, not
  per slug
- clone pool build enqueues global discovery and marks
  `waiting_for_global_library` when selected moodboards have fewer than
  `global_refs_for_pool_min` active not-yet-rejected global references
- clone pool waiting wakeups use `clone_pool_waiting_moodboards` indexes, not
  JSON scans
- `clone_pool_waiting_moodboards` enforces
  `UNIQUE(pool_run_id, moodboard_slug)`
- `clone_pool_waiting_moodboards` rows transition to `resumed`,
  `insufficient`, or `superseded` under the specified conditions
- clone pool build proceeds while top-up discovery runs when selected moodboards
  are below target but have enough active refs to attempt compatibility
- clone pool build attempts partial compatibility when discovery is exhausted
  but at least one active global reference exists for selected moodboards
- `FinalizeGlobalMoodboardLibrary` resumes waiting ready clone pools or marks
  them insufficient when no references remain possible
- clone compatibility checks body proportions, hair length, and facial hair
- clone compatibility does not reject only because of gender
- clone compatibility retry state records `attempt_count`, last error, and
  `next_retry_at`, and retries only eligible failed rows
- incompatible global references are written to
  `clone_visual_reference_compatibility`, not to Blitz-ready
  `visual_references`
- compatible global references create clone-scoped `visual_references`
- incompatible global references are recorded and skipped for the same clone
- deselected moodboards are excluded from future Blitz selection without
  changing generation eligibility
- deselected moodboards do not change `visual_references.status` used by queued
  generation jobs
- generation and Blitz reference loading enforce clone ownership and accepted
  compatibility before using a global reference
- generation loaders permit `media_assets.user_id = 'global'` only through an
  active global reference and accepted clone compatibility
- generation loaders require `visual_references.media_asset_id` to equal
  `global_moodboard_references.media_asset_id`
- Blitz selection still reads clone-scoped `visual_references` by `clone_id`
- Workers AI, ScrapeCreators, Seedream, image fetch, R2, and D1 failures map to
  recorded failure states, not queue panics

## Open Implementation Notes

- Because there are no production users, the implementation can rebuild the
  affected visual-reference tables instead of preserving clone-scoped candidate
  rows.
- Prefer a new append-only migration:
  `1009_global_moodboard_reference_pipeline.sql`.
- The current `niche_research.rs` file is doing too much. Implementation should
  split global discovery, Instagram normalization, Kimi review, cleanup, clone
  compatibility, R2 caching, and queue orchestration into smaller modules.
- The ScrapeCreators OpenAPI document at `docs/scrape-creators-openapi.yaml`
  should be the source of truth for endpoint parameters such as Reels
  `date_posted`, page, profile trim, posts pagination, post detail
  `download_media`, and post detail trim behavior.
- Do not use ScrapeCreators `download_media=true` for v1 global reference
  cleanup unless the implementation explicitly decides the extra provider cost
  is worth permanent source media URLs. The current design fetches source URLs,
  runs Kimi review, then stores only Seedream-cleaned output in our own media
  storage.
