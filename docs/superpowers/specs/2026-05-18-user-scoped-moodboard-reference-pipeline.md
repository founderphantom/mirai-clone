# User-Scoped Moodboard Reference Pipeline

Date: 2026-05-18

Status: Draft for implementation planning.

Supersedes/amends:

- `docs/superpowers/specs/2026-05-14-visual-reference-pipeline-design.md`
- `docs/superpowers/specs/2026-05-17-pipeline-v2-visual-reference-migration-design.md`
- `docs/superpowers/plans/2026-05-17-pipeline-v2-visual-reference-migration.md`

## Product Goal

Moodboards are a user's persistent visual preference layer, not a clone-owned
asset. A failed Soul clone retry must not lose moodboard selections, discovered
references, cleaned images, or research progress.

Niche discovery should do as much work as possible once per user. Clone-specific
work starts only when Mirai needs a Blitz generation pool for a specific clone.
Blitz remains clone-scoped and continues to read from `visual_references` by
`clone_id`.

## Target Model

1. Moodboard selection is user-specific.
   - Selection is saved once per user.
   - Failed clone retries do not lose moodboard preferences.
   - Onboarding can display and save moodboards even when no usable active clone
     exists.
2. Niche discovery is user/moodboard scoped where possible.
   - Search terms, discovered handles, raw post candidates, visual review, and
     text cleanup do not require a clone.
   - These results can be reused across clone retries.
3. Clone-specific work starts at pool build.
   - When building a Blitz pool for a clone, load the user's selected cleaned
     references.
   - Run clone compatibility for that specific clone.
   - Store compatible refs in clone-specific `visual_references`.
   - Blitz continues to read by `clone_id`.

## Current Logic Being Replaced

The current implementation couples moodboards and niche research to the active
clone:

- `moodboards.clone_id` is `NOT NULL`.
- onboarding loads moodboards for `activeClone.id`.
- saving moodboards requires a usable active clone.
- `NicheResearchMessage` variants all carry `clone_id`.
- `visual_reference_candidates` and `visual_references` are clone-owned.
- `user_inspiration_pool` is clone-owned and depends on clone-scoped
  references.
- cleanup and clone compatibility both run before the reference becomes usable.
- a failed clone can own selected moodboards and partially researched candidates
  that the next clone cannot reuse.

This is the root cause of the retry problem: a user can start moodboard research
against a clone that later fails Soul training, then create a new clone that has
no moodboards and no reusable reference pool.

## Product Decisions

- Moodboard and niche still mean the same product concept for this pipeline.
- Users can select 1 to 10 moodboards.
- Moodboard IDs should be deterministic by `user_id + slug`, not by
  `user_id + clone_id + slug`.
- Default moodboards are created once per user.
- `GET /api/onboarding/state` ensures default moodboards exist before returning
  state, so no-clone onboarding can show moodboards immediately.
- Saving moodboards does not require a clone.
- User-level research may start immediately after moodboards are saved.
- Clone-specific compatibility waits until a clone is ready to build a Blitz
  pool.
- Public profile pictures are never production references.
- Static Instagram photos and carousel child images are preferred.
- Videos and reels are skipped as image sources by default. Reels search may be
  used only for discovering owner handles.
- Kimi K2.6 through Cloudflare Workers AI is the only analysis model.
- Seedream 5.0 Lite cleanup removes only visible text from images.
- Only cleaned images are stored for reusable reference generation.
- Captions and source text are untrusted metadata. They may be stored for audit
  and filtering, but must not be sent to generation.

## Non-Goals

- Do not copy failed clone moodboard rows onto each retry clone.
- Do not run full Instagram discovery separately for every clone retry.
- Do not make Blitz read directly from user-level references; Blitz remains
  clone-scoped through `visual_references`.
- Do not use profile pictures, source captions, handle names, or identity claims
  as generation references.
- Do not add the secondary gender compatibility signal yet.

## High-Level Flow

```text
User opens onboarding
  -> GET /api/onboarding/state ensures default moodboards for user
  -> Load user moodboards and current usable active clone, independently

User selects 1-10 moodboards
  -> Save selected moodboards for user
  -> Enqueue user-scoped reference research

User-scoped reference research
  -> Load selected user moodboards
  -> Discover Instagram handles
  -> Fetch profile metadata and static posts/carousel children
  -> Normalize and rank candidates
  -> Kimi K2.6 visual guardrail and moodboard assignment
  -> Seedream 5.0 Lite text-only cleanup
  -> Cache only cleaned images to R2/media_assets with clone_id = NULL
  -> Store cleaned user references

Clone becomes ready or Blitz needs a pool
  -> Load selected cleaned user references
  -> Run clone compatibility for this clone
  -> Insert compatible clone-scoped visual_references
  -> Create or refresh Blitz batch for the clone
```

## Moodboard Selection

The onboarding API should treat moodboards as user-owned:

- `GET /api/onboarding/state` ensures default moodboards exist, then returns
  user moodboards regardless of clone state.
- `POST /api/onboarding/moodboards/generate` creates default moodboards for the
  authenticated user and does not require `cloneId`.
- `POST /api/onboarding/moodboards` saves selected moodboard IDs for the user and
  does not require `cloneId`.
- The existing `cloneId` request field can remain temporarily for frontend
  compatibility, but the backend must ignore it for moodboard persistence.

Validation:

- minimum selected moodboards: `1`
- maximum selected moodboards: `10`
- duplicates are removed after trimming IDs
- selected moodboards must belong to the authenticated user

The route should enqueue user-scoped research after saving selected moodboards.
If no clone exists or the only clone has `soul_status = 'failed'`, the research
still runs and the cleaned user reference library is preserved.

Frontend scope:

- The onboarding UI must allow moodboard selection when `activeClone` is `null`.
- The UI must accept 1 to 10 selected moodboards, not exactly 5.
- A failed clone retry banner must not hide or disable moodboard preferences.
- Upload and moodboard selection are independent onboarding surfaces; upload
  creates or retries the Soul clone, while moodboards persist at user scope.

## Discovery Inputs

Each selected moodboard has app-owned search terms and configured handles.
Configured handles should come from the existing
`moodboard_instagram_handles_json` configuration, not from a new schema surface.
Discovery starts from those inputs and from previously successful user-level
handles for the same assigned moodboard.

Handle selection rules:

- Start from configured handles for selected moodboards.
- Use Reels search only to discover owner handles when configured handles are
  insufficient.
- Add previously successful handles from accepted cleaned user references for
  the same user and Kimi-assigned moodboard. Preserve the source moodboard only
  for audit; do not use it as the reuse bucket after Kimi routes the image.
- Optionally add one-hop related profiles from `/v1/instagram/profile`, capped
  tightly and only when the profile is public.
- Do not discover from profile pictures.
- Do not expand beyond one related-profile hop in v1.

Default diversity caps should remain configurable:

- `instagram_search_terms_per_moodboard`: `2`
- `instagram_reels_pages_per_term`: `1`
- `instagram_max_handles_per_moodboard`: `20`
- `instagram_profiles_per_moodboard`: `3`
- `instagram_related_profiles_per_seed`: `2`
- `instagram_max_profiles_per_run`: `20`
- `instagram_posts_per_profile`: `12`
- `instagram_pages_per_profile`: `1`
- `instagram_images_per_post`: `3`
- `instagram_candidate_review_limit`: `60`
- `instagram_min_image_width`: `512`
- `instagram_min_image_height`: `512`
- `accepted_refs_per_profile_cap`: `3`
- `accepted_refs_per_moodboard_target`: `5`
- `max_accepted_refs_per_run`: `40`
- `visual_reference_cleanup_retry_limit`: `3`
- `visual_reference_compatibility_retry_limit`: `2`
- `clone_compatibility_reference_limit`: `4`
- `batch_size`: `5`

`clone_compatibility_reference_limit` controls how many clone identity/reference
images are sent into a compatibility review. It is not the number of user
references required for a Blitz batch. Blitz pool readiness uses `batch_size`.

## Candidate Normalization

Normalize each usable image into a single user-level candidate. A carousel can
produce multiple candidates, one per child image, subject to
`instagram_images_per_post`.

Candidate identity:

```text
instagram:<post-id-or-shortcode>:<image-index-or-child-media-id>
```

The identity must not include the Instagram handle. Handles can change, and the
same post image can be rediscovered through another handle or related profile.
Prefer stable Instagram media IDs and child media IDs when ScrapeCreators
returns them. Fall back to shortcode plus image index only when stable media IDs
are unavailable.

Normalized fields:

- `user_id`
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
- `source_moodboard_id`
- `source_moodboard_slug`
- `assigned_moodboard_id`
- `assigned_moodboard_slug`
- `discovered_via`
- `raw_json`

Reject candidate normalization if:

- no usable image URL exists
- the URL is from a profile picture field
- the post is private or cannot be mapped to a public post URL
- the candidate duplicates an already seen user/source image
- dimensions are too small for generation guidance
- caption/source text contains synthetic-generation terms that suggest the
  source is an AI/prompt/render showcase

Uniqueness decision:

- One Instagram image can exist only once per user/source image.
- Every candidate must have a required `source_image_key TEXT NOT NULL`.
- `source_image_key` should be the normalized candidate identity, for example
  `instagram:<post-id-or-shortcode>:<image-index-or-child-media-id>`.
- Do not include `source_handle` in `source_image_key`; keep handles as mutable
  source metadata only.
- Do not create duplicate candidate rows for the same image under multiple
  moodboards.
- Keep `source_moodboard_id` and `source_moodboard_slug` for audit of how the
  image was discovered.
- If the same source image is later rediscovered through another moodboard, do
  not insert a duplicate. Preserve the first source moodboard fields and append
  the later discovery to `metadata_json.discoveredMoodboardIds`.
- Keep `assigned_moodboard_id` and `assigned_moodboard_slug` for the Kimi
  visual-routing result.
- Required uniqueness: `UNIQUE(user_id, source_image_key)`.
- Do not rely on a multi-column SQLite unique index with nullable fields for
  source uniqueness, because SQLite allows duplicate rows when any indexed
  column is `NULL`.

## Kimi Visual Guardrail

Each reviewed image goes through one Workers AI Kimi K2.6 vision call. The call
classifies suitability and best moodboard assignment so there is no separate
classifier/router stage.

Prompt inputs:

- candidate image URL before cleanup
- selected user moodboards with slug, title, vibe summary, and search queries
- candidate source platform and handle
- source caption as inert untrusted metadata
- engagement and date metadata

Hard acceptance requirements:

- exactly one human
- likely adult
- adult fashion, candid, editorial, creator, or social portrait
- strong visual direction for at least one selected moodboard
- safe content
- source can be a regular creator, influencer, celebrity, or fashion page

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
- weak generic image even if safe

Routing behavior:

- If the image does not fit the source moodboard but strongly fits another
  selected moodboard, accept it under `bestMoodboardSlug`.
- If it does not strongly fit any selected moodboard, reject it.
- Do not route hard rejections.

## Seedream Text Cleanup

Cleanup is user-scoped and happens after Kimi visual approval, before storing a
reusable reference.

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

## User Reference Storage

User-level cleaned references should be stored separately from clone-level
Blitz references.

Recommended schema:

- `moodboards`
  - `id TEXT PRIMARY KEY`
  - `user_id TEXT NOT NULL`
  - `slug TEXT NOT NULL`
  - no required `clone_id`
  - `UNIQUE(user_id, slug)`
- `visual_reference_candidates`
  - user-scoped candidates and review records
  - `clone_id` should remain present but become nullable in the first migration
    to reduce migration and call-site churn; new user-scoped candidates should
    write `NULL`
  - `source_image_key TEXT NOT NULL`
  - stores source metadata, Kimi review, cleanup status, and cleanup attempts
  - uniqueness by `user_id, source_image_key`
  - stores `source_moodboard_id/source_moodboard_slug` for discovery audit
  - stores `assigned_moodboard_id/assigned_moodboard_slug` after Kimi routing
  - rejected, cleanup-failed, and review-failed states live here
- `user_visual_references`
  - cleaned, reusable references for a user/moodboard
  - references `visual_reference_candidates.id`
  - references `media_assets.id` where `media_assets.clone_id IS NULL`
  - uses the candidate's assigned moodboard as the reusable reference moodboard
  - stores visual tags from Kimi review
  - status values: `active`, `disabled`, `deleted`
- `clone_visual_reference_compatibility`
  - mandatory table for clone/reference compatibility attempts
  - stores one row per `clone_id + user_visual_reference_id`
  - status values: `queued`, `accepted`, `rejected`, `failed`
  - stores body-proportion, hair-length, and facial-hair decisions
  - incompatible references are recorded here and must not become Blitz-ready
    `visual_references`
- `visual_references`
  - clone-scoped Blitz-ready references
  - `user_id TEXT NOT NULL`
  - references `user_visual_references.id`
  - keeps `clone_id NOT NULL`
  - stores only compatibility-accepted references and generation usage counters
- `user_inspiration_pool`
  - remains clone-scoped for Blitz
  - references clone-scoped `visual_references`
  - must not point directly at `user_visual_references`

Storage key shape for cleaned user references:

```text
visual-reference-library/<user-id>/<user-reference-id>/cleaned.<ext>
```

`media_assets` rows for cleaned user references:

- `kind = visual_reference`
- `source = instagram`
- `user_id = <user id>`
- `clone_id = NULL`
- `remote_url = cleaned image URL returned by Seedream`
- metadata includes candidate ID, source post code, moodboard slug, and cleanup
  model/tool
- asset ownership must be enforced by joining or checking
  `media_assets.user_id = user_visual_references.user_id`

Clone-scoped `visual_references` that point at a user-level cleaned asset must
also satisfy:

- `visual_references.user_id = user_visual_references.user_id`
- `visual_references.user_id = media_assets.user_id`
- request user ID equals all three `user_id` values
- `media_assets.clone_id IS NULL OR media_assets.clone_id = visual_references.clone_id`

The existing `niche_cluster` field can keep mirroring `moodboard_slug` in
clone-scoped `visual_references` until Blitz naming is cleaned up.

## Migration Strategy

D1 uses SQLite semantics and cannot cleanly drop a `NOT NULL` constraint from an
existing column. The `1009_user_scoped_moodboard_references.sql` migration must
therefore rebuild the affected tables instead of attempting in-place constraint
removal.

Because there are no production users, this migration can be a clean rebuild of
the visual-reference surface. It should still be append-only and deterministic
for local and preview databases.

The migration must rebuild or recreate:

- `moodboards`
  - remove required `clone_id`
  - add or preserve `UNIQUE(user_id, slug)`
- `visual_reference_candidates`
  - make `clone_id` nullable while removing clone ownership from new rows
  - add required `source_image_key TEXT NOT NULL`
  - add source and assigned moodboard audit columns
  - enforce once-per-user/source-image uniqueness
- `user_visual_references`
  - create the reusable cleaned reference library
- `clone_visual_reference_compatibility`
  - create the mandatory clone/reference compatibility table
- `visual_references`
  - keep `clone_id NOT NULL`
  - add `user_visual_reference_id`
  - store only compatibility-accepted Blitz-ready rows
- `user_inspiration_pool`
  - rebuild FKs and uniqueness so clone-scoped pool rows point at
    clone-scoped `visual_references`
- `user_reference_state` and `user_research_runs`
  - create user-scoped research status and run-token tables
- `clone_reference_state` and `clone_pool_runs`
  - create clone-scoped pool status and pool-run token tables
- dependent indexes and foreign keys for all rebuilt tables

Recommended migration shape:

1. Disable foreign keys.
2. Drop dependent tables or rebuild into `_new` tables.
3. Recreate tables with the user-scoped schema.
4. Recreate indexes and foreign keys.
5. Re-enable foreign keys.

Do not preserve clone-owned moodboard or candidate rows from failed local test
clones unless a later production migration explicitly requires data backfill.

## Clone-Specific Pool Build

The clone-specific stage runs when Blitz needs references for a clone:

- after Soul training becomes ready
- when a user opens Blitz and no active pool exists
- when a pool is depleted or stale
- after user moodboard selection changes and a ready clone exists

Inputs:

- user ID
- clone ID
- clone reference images or Soul metadata needed for compatibility review
- selected active user moodboards
- cleaned user references for those moodboards

Compatibility checks:

- similar body proportions
- similar hair length
- facial hair

Gender is intentionally not part of the v1 compatibility signal.

The compatibility prompt should reject references that conflict strongly with
body proportions, hair length, or facial hair. If those are acceptable, gender
differences should not be used as a rejection reason.

Outputs:

- insert compatibility attempts into `clone_visual_reference_compatibility`
- insert only compatible rows into clone-scoped `visual_references`
- copy or reference Kimi visual tags from `user_visual_references`
- set `visual_references.media_asset_id` to the cleaned user reference asset
- set `visual_references.clone_id` to the target clone
- mark incompatible rows in `clone_visual_reference_compatibility` so the same
  pair is not retried repeatedly and never appears in Blitz selection

Idempotency:

- `UNIQUE(clone_id, user_visual_reference_id)` is required on
  `clone_visual_reference_compatibility`.
- `UNIQUE(clone_id, user_visual_reference_id)` is also required on
  clone-scoped `visual_references` where `user_visual_reference_id IS NOT NULL`.
- Re-running pool build for the same clone should add missing compatible refs,
  not duplicate existing ones.

## Moodboard Selection Changes

When a user changes moodboard selection, the system must prevent deselected
moodboards from feeding future Blitz batches for every clone owned by that user.

Required behavior:

- user-level `user_visual_references` can remain `active`; they are reusable if
  the moodboard is selected again later
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

The default is selection-time exclusion through the current user moodboard
selection, then pool rebuild for ready clones. This allows queued generation jobs
to finish while preventing deselected moodboards from entering future batches.

## Queue Design

Split queue messages into user-scoped research and clone-scoped pool build.

User-scoped messages:

- `ResearchUserMoodboardReferences { user_id, run_id, moodboard_ids,
  selected_moodboard_hash, reason }`
- `DiscoverInstagramHandles { user_id, run_id, moodboard_id, moodboard_slug,
  search_term, page }`
- `FetchInstagramProfile { user_id, run_id, moodboard_id, moodboard_slug,
  handle, discovered_via, related_depth }`
- `FetchInstagramPosts { user_id, run_id, moodboard_id, moodboard_slug, handle,
  discovered_via, next_max_id, page }`
- `FetchInstagramPostDetail { user_id, run_id, moodboard_id, moodboard_slug,
  handle, discovered_via, source_url }`
- `ReviewUserVisualCandidates { user_id, run_id, limit }`
- `CleanupApprovedUserReference { user_id, run_id, candidate_id }`
- `FinalizeUserReferenceLibrary { user_id, run_id, reason }`

Clone-scoped messages:

- `BuildCloneReferencePool { user_id, clone_id, pool_run_id, reason }`
- `ValidateCloneCompatibility { user_id, clone_id, pool_run_id,
  user_reference_id }`
- `FinalizeCloneReferencePool { user_id, clone_id, pool_run_id, reason }`
- `RefreshPool { user_id, clone_id, pool_run_id, reason }`

Rules:

- User-scoped messages must not require `clone_id`.
- `POST /api/onboarding/moodboards` creates the `user_research_runs` row before
  enqueueing research.
- The route stores `run_id` as `user_reference_state.current_research_run_id`.
- The kickoff message must carry `run_id` and `selected_moodboard_hash`; it does
  not create the run.
- `selected_moodboard_hash` must be deterministic: SHA-256 of the JSON array of
  selected moodboard IDs sorted lexicographically, encoded with no extra fields.
- Every downstream user-scoped message must carry the same `run_id`.
- User-scoped kickoff handling must verify the selected moodboard hash before
  enqueueing downstream work.
- The producer of `BuildCloneReferencePool` or `RefreshPool` must create the
  `clone_pool_runs` row before enqueueing the message.
- The producer stores `pool_run_id` as
  `clone_reference_state.current_pool_run_id`.
- `BuildCloneReferencePool`, `RefreshPool`, and every downstream clone-scoped
  message must carry the same `pool_run_id`.
- Clone-scoped handlers must never run Instagram discovery directly.
- If the user's cleaned library is insufficient, the clone-scoped handler should
  enqueue a new user-scoped `ResearchUserMoodboardReferences` run, mark the
  clone pool run `waiting_for_library`, and defer or retry pool build after the
  user library changes.
- `FinalizeUserReferenceLibrary` is responsible for resuming deferred clone pool
  work. After a current user research run finishes, it must inspect ready,
  nonfailed clones in `waiting_for_library` for that user and create/enqueue new
  `BuildCloneReferencePool` messages with fresh `pool_run_id` values when there
  is at least one active cleaned user reference for the current selected
  moodboards.
- If `FinalizeUserReferenceLibrary` finds waiting ready clones but the current
  user library still has zero active cleaned references for selected moodboards
  and no retryable user-research work remains, it should mark those current pool
  runs `insufficient_refs` rather than waiting forever.
- Queue messages from stale user research runs must be acked after recording
  `stale_run` or equivalent audit state. They must not mark the current user
  library as ready or failed.
- Queue messages from stale clone pool runs must be acked after recording
  `stale_pool_run` or equivalent audit state. They must not mark the current
  clone pool status as ready, partial, insufficient, or failed.
- Queue handlers should ack malformed or exhausted messages after recording the
  failure state.
- Workers AI, ScrapeCreators, Seedream, image fetch, R2, and D1 failures must
  not panic the Worker.
- Per-candidate failures are recorded and do not fail the whole research run.
- Per-source failures are recorded and do not fail the whole research run unless
  no sources succeed.

## Status Storage

Do not store user-level research status only in
`clone_profiles.provider_config_json`. A failed clone must not own the user's
research status.

Recommended status storage:

- `user_reference_state`
  - `user_id`
  - `current_research_run_id`
  - `selected_moodboard_ids_json`
  - `selected_moodboard_hash`
  - `status`
  - counts by moodboard
  - timestamps
- `user_research_runs`
  - `id`
  - `user_id`
  - `status`
  - `reason`
  - `selected_moodboard_ids_snapshot_json`
  - `selected_moodboard_hash`
  - counts by moodboard
  - `error_code`
  - `error_message`
  - timestamps
- `clone_reference_state`
  - `user_id`
  - `clone_id`
  - `current_pool_run_id`
  - `selected_moodboard_hash`
  - `status`
  - compatibility counts
  - timestamps
- `clone_pool_runs`
  - `id`
  - `user_id`
  - `clone_id`
  - `status`
  - `reason`
  - `selected_moodboard_ids_snapshot_json`
  - `selected_moodboard_hash`
  - compatibility counts
  - `error_code`
  - `error_message`
  - timestamps

Temporary compatibility path:

- It is acceptable to keep clone-level pool status in
  `clone_profiles.provider_config_json.nicheResearchStatus`.
- User-level research status must live outside the failed clone lifecycle.

Status values:

- user research: `queued`, `scraping`, `reviewing`, `cleaning`, `library_ready`,
  `insufficient_refs`, `partial_library_ready`, `research_failed`
- clone pool: `queued`, `waiting_for_library`, `compatibility_reviewing`,
  `pool_ready`, `insufficient_refs`, `partial_pool_ready`, `pool_failed`

Readiness thresholds:

- User-library counts use active `user_visual_references` whose assigned
  moodboard is currently selected.
- `library_ready`: every currently selected moodboard has at least
  `accepted_refs_per_moodboard_target` active cleaned user references.
- `partial_library_ready`: the full per-moodboard target is not met, but the
  user has at least one active cleaned reference for the current selected
  moodboards.
- `insufficient_refs`: user research has exhausted configured sources,
  retryable candidate work, and cleanup retries, and there are zero active
  cleaned references for the current selected moodboards.
- `research_failed`: an infrastructure or provider failure prevents the run from
  making progress and no retryable queue work remains. If the run merely found
  no usable references after successful processing, use `insufficient_refs`
  instead.
- Clone-pool counts use clone-scoped `visual_references` for currently selected
  moodboards and the current clone.
- `pool_ready`: compatible active clone-scoped references for selected
  moodboards are greater than or equal to `batch_size`.
- `partial_pool_ready`: at least one compatible active clone-scoped reference is
  available for selected moodboards, but the count is below `batch_size`.
- `waiting_for_library`: the clone has no compatible refs to build from yet and
  a user-scoped research run has been queued or is still in progress.
- `insufficient_refs`: no compatible active clone-scoped references are
  available after compatibility work is exhausted, and no user-library work is
  queued or in progress for the current selection.
- `pool_failed`: an infrastructure or provider failure prevents pool build from
  making progress and no retryable queue work remains.

Stale run behavior:

- Saving moodboards creates a new `user_research_runs` row and stores its ID as
  `user_reference_state.current_research_run_id`.
- Downstream user research messages update user-visible status only when their
  `run_id` still matches `user_reference_state.current_research_run_id`.
- If a user changes selection while older queue messages are still running, the
  old messages may finish per-candidate writes, but they must not mark the
  current selection `library_ready`, `partial_library_ready`, or
  `research_failed`.
- Clone pool builds must use the current selected moodboard IDs, not the
  moodboard snapshot from an older user research run.

Stale pool behavior:

- Starting or refreshing a clone pool creates a new `clone_pool_runs` row and
  stores its ID as `clone_reference_state.current_pool_run_id`.
- Downstream clone pool messages update clone-visible pool status only when
  their `pool_run_id` still matches
  `clone_reference_state.current_pool_run_id`.
- If a user changes moodboard selection while older compatibility messages are
  still running, old messages may finish per-reference compatibility audit
  writes, but they must not mark the current clone pool `pool_ready`,
  `partial_pool_ready`, `insufficient_refs`, or `pool_failed`.

## Blitz Learning Behavior

Blitz continues to learn from clone-scoped `visual_references` and swipes.

Each generated Blitz image should retain:

- `visual_reference_id`
- `user_visual_reference_id`
- `moodboard_id`
- `moodboard_slug`
- `source_handle`
- `source_platform`
- Kimi visual tags: pose, scene, lighting, framing, camera feel, styling
  direction

Storage:

- `generation_jobs.input_visual_reference_id` remains the primary generation
  reference pointer.
- `user_visual_reference_id` is derived through
  `visual_references.user_visual_reference_id` for generation history and
  analytics.
- `blitz_swipes.output_metadata_json` should snapshot `userVisualReferenceId`
  alongside `visualReferenceId` so swipe learning survives later reference-row
  changes.
- Do not add a redundant `user_visual_reference_id` column to
  `generation_outputs` unless a later query path needs it for performance.

Likes increase future selection weight for similar moodboards, handles, and
visual tags. Dislikes decrease them. Diversity caps still apply, so the next
batch does not overuse one handle or one visual pattern.

Reference selection caps:

- no more than 2 references from the same handle per Blitz batch
- no more than 2 references from the same moodboard per batch until all selected
  moodboards have been represented when possible
- reuse references only when the pool is too small or the user has liked that
  visual direction

## Generation Contract

Generation uses the cleaned R2 media asset from the clone-scoped
`visual_references.media_asset_id`.

Generation guidance passed to Higgsfield should include:

- the R2-backed cleaned reference image
- clone Soul ID
- selected aspect ratio based on reference image dimensions
- 4K quality setting when the Higgsfield tool supports it
- visual cues from Kimi review

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
- `user_visual_references.user_id` equals requested `user_id`
- `media_assets.user_id` equals requested `user_id`
- `media_assets.clone_id` is either `NULL` for user-level cleaned references or
  matches the clone for clone-owned assets

## Failed Clone Retry Behavior

When Soul training fails:

- the failed clone remains visible with its training failure reason
- the failed clone does not count toward active clone limits
- onboarding keeps moodboards available because moodboards are user-scoped
- the user can upload a fresh reference set
- the new clone can reuse the user's selected moodboards and cleaned reference
  library
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
- user-scoped queue messages serialize without `cloneId`
- clone-scoped pool messages serialize with `cloneId`
- user research messages from stale run IDs cannot update current user status
- clone pool messages carry `poolRunId`, not generic `runId`
- clone pool messages from stale `poolRunId` values cannot update current clone
  pool status
- insufficient user reference libraries enqueue user-scoped research and defer
  pool build; clone-scoped handlers never run Instagram discovery directly
- `FinalizeUserReferenceLibrary` resumes waiting ready clone pools or marks them
  insufficient when no references remain possible
- user-library and clone-pool statuses follow the explicit readiness thresholds
- Instagram profile normalizer never emits profile pictures as candidates
- user posts normalizer extracts static post image candidates
- carousel post enrichment extracts child images from `/v1/instagram/post`
- videos are skipped unless fallback thumbnails are explicitly enabled
- candidate uniqueness is once per `user_id + source image`, not per moodboard
- source uniqueness uses required `source_image_key`, not nullable multi-column
  SQLite uniqueness
- `source_image_key` is stable across handle changes and does not include
  `source_handle`
- previously successful handles are reused by assigned moodboard, not source
  moodboard
- candidate ranking respects handle, profile, post, and moodboard diversity caps
- Kimi review accepts only one likely adult with strong visual fit
- Kimi review rejects moodboards, screenshots, product shots, tutorials, generic
  images, no-human images, multi-human images, minors, and unsafe images
- off-style but strong images are assigned to another selected moodboard
- Seedream cleanup prompt matches the exact text-only prompt
- cleanup retries are capped and failed candidates do not block replacements
- only cleaned images are cached to R2
- clone compatibility checks body proportions, hair length, and facial hair
- clone compatibility does not reject only because of gender
- incompatible user references are written to
  `clone_visual_reference_compatibility`, not to Blitz-ready
  `visual_references`
- compatible user references create clone-scoped `visual_references`
- incompatible user references are recorded and skipped for the same clone
- deselected moodboards are excluded from future Blitz selection without changing
  generation eligibility
- deselected moodboards do not change `visual_references.status` used by queued
  generation jobs
- generation and Blitz reference loading enforce user asset ownership across
  `visual_references`, `user_visual_references`, and `media_assets`
- Blitz selection still reads clone-scoped `visual_references` by `clone_id`
- Workers AI, ScrapeCreators, Seedream, image fetch, R2, and D1 failures map to
  recorded failure states, not queue panics

## Open Implementation Notes

- Because there are no production users, the implementation can rebuild the
  affected visual-reference tables instead of preserving clone-scoped moodboard
  rows.
- Prefer a new append-only migration, such as
  `1009_user_scoped_moodboard_references.sql`, so local and preview databases
  move forward predictably.
- `media_assets.clone_id` is already nullable and can support user-level
  cleaned reference assets.
- Existing frontend types can keep `activeClone` and `moodboards` side by side;
  the key change is that moodboards no longer depend on `activeClone`.
- The current `niche_research.rs` file is doing too much. Implementation should
  split user research, clone compatibility, Instagram normalization, cleanup,
  R2 caching, and queue orchestration into smaller modules.
