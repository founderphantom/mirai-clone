# User-Scoped Moodboard Reference Pipeline

Date: 2026-05-18

Status: Draft for implementation planning.

Supersedes: `docs/superpowers/specs/2026-05-14-visual-reference-pipeline-design.md`

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
  -> Ensure default moodboards for user
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

- `GET /api/onboarding/state` returns user moodboards regardless of clone state.
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

## Discovery Inputs

Each selected moodboard has app-owned search terms and configured handles.
Discovery starts from those inputs and from previously successful user-level
handles for the same moodboard.

Handle selection rules:

- Start from configured handles for selected moodboards.
- Use Reels search only to discover owner handles when configured handles are
  insufficient.
- Add previously successful handles from accepted cleaned user references for
  the same user and moodboard.
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
- `accepted_refs_per_profile_cap`: `3`
- `accepted_refs_per_moodboard_target`: `5`
- `max_accepted_refs_per_run`: `40`

## Candidate Normalization

Normalize each usable image into a single user-level candidate. A carousel can
produce multiple candidates, one per child image, subject to
`instagram_images_per_post`.

Candidate identity:

```text
instagram:<handle>:<post-code>:<image-index-or-media-id>
```

Normalized fields:

- `user_id`
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
- `moodboard_id`
- `moodboard_slug`
- `discovered_via`
- `raw_json`

Reject candidate normalization if:

- no usable image URL exists
- the URL is from a profile picture field
- the post is private or cannot be mapped to a public post URL
- the candidate duplicates an already seen user/moodboard/post image
- dimensions are too small for generation guidance
- caption/source text contains synthetic-generation terms that suggest the
  source is an AI/prompt/render showcase

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
  - `clone_id` should be nullable or removed
  - stores source metadata, Kimi review, cleanup status, and cleanup attempts
  - uniqueness by `user_id, platform, source_handle, source_post_code,
    source_image_index`
- `user_visual_references`
  - cleaned, reusable references for a user/moodboard
  - references `visual_reference_candidates.id`
  - references `media_assets.id` where `media_assets.clone_id IS NULL`
  - stores visual tags from Kimi review
  - status values: `active`, `rejected`, `cleanup_failed`, `disabled`
- `visual_references`
  - clone-scoped Blitz-ready references
  - references `user_visual_references.id`
  - keeps `clone_id NOT NULL`
  - stores clone compatibility result and generation usage counters

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

The existing `niche_cluster` field can keep mirroring `moodboard_slug` in
clone-scoped `visual_references` until Blitz naming is cleaned up.

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

- insert compatible rows into clone-scoped `visual_references`
- copy or reference Kimi visual tags from `user_visual_references`
- set `visual_references.media_asset_id` to the cleaned user reference asset
- set `visual_references.clone_id` to the target clone
- mark incompatible rows in a clone/reference compatibility record so the same
  pair is not retried repeatedly

Idempotency:

- `UNIQUE(clone_id, user_visual_reference_id)` on clone-scoped
  `visual_references` or a separate compatibility table.
- Re-running pool build for the same clone should add missing compatible refs,
  not duplicate existing ones.

## Queue Design

Split queue messages into user-scoped research and clone-scoped pool build.

User-scoped messages:

- `ResearchUserMoodboardReferences { user_id, moodboard_ids, reason }`
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

- `BuildCloneReferencePool { user_id, clone_id, reason }`
- `ValidateCloneCompatibility { user_id, clone_id, user_reference_id, run_id }`
- `FinalizeCloneReferencePool { user_id, clone_id, run_id, reason }`
- `RefreshPool { user_id, clone_id, reason }`

Rules:

- User-scoped messages must not require `clone_id`.
- Clone-scoped messages must not trigger Instagram discovery unless the user's
  cleaned library is insufficient.
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

- `user_research_runs`
  - `id`
  - `user_id`
  - `status`
  - `reason`
  - counts by moodboard
  - `error_code`
  - `error_message`
  - timestamps
- `clone_pool_runs`
  - `id`
  - `user_id`
  - `clone_id`
  - `status`
  - `reason`
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
- clone pool: `queued`, `compatibility_reviewing`, `pool_ready`,
  `insufficient_refs`, `partial_pool_ready`, `pool_failed`

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
- saving moodboards does not require a clone
- moodboard selection accepts 1-10 and rejects 0 or 11+
- selected moodboards must belong to the authenticated user
- failed clone retries preserve selected moodboards
- user-scoped queue messages serialize without `cloneId`
- clone-scoped pool messages serialize with `cloneId`
- Instagram profile normalizer never emits profile pictures as candidates
- user posts normalizer extracts static post image candidates
- carousel post enrichment extracts child images from `/v1/instagram/post`
- videos are skipped unless fallback thumbnails are explicitly enabled
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
- compatible user references create clone-scoped `visual_references`
- incompatible user references are recorded and skipped for the same clone
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
