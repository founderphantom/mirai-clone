# Pipeline V2 Visual Reference Migration

Date: 2026-05-17

Status: Draft for implementation planning.

## Goal

Replace Mirai's broken niche and moodboard visual reference pipeline with the
working behavior from `social-page/pipeline-v2`, while keeping Mirai's
Cloudflare Worker, D1, R2, and Blitz architecture.

The migrated app pipeline must find Instagram visual references from selected
moodboards, validate them as useful one-adult visual references, remove visible
image text with Seedream 5.0 Lite, verify the cleaned reference is compatible
with the user's clone, and store only cleaned compatible images for Blitz
generation.

## Product Decisions

- Port `pipeline-v2` behavior into Mirai's Product Worker. Do not run the local
  prototype as a side service.
- Use ScrapeCreators Reels Search only to discover owner handles.
- Use static Instagram profile posts and carousel child images as final
  reference candidates.
- Skip reels and videos as final references in v1.
- Use Workers AI Kimi K2.6, `@cf/moonshotai/kimi-k2.6`, for visual review,
  moodboard routing, and clone compatibility validation.
- Add Seedream 5.0 Lite cleanup after Kimi visual approval and before any image
  is stored in R2.
- The Seedream cleanup prompt must be narrow:

  ```text
  Remove only the visible text from this image. Keep every non-text part of the image exactly the same.
  ```

- Store only the Seedream-cleaned image in `MEDIA` R2 and `media_assets`.
- Do not store original Instagram image bytes. Original URLs and captions remain
  candidate audit metadata only.
- Add clone compatibility validation after cleanup and before caching.
- Clone compatibility v1 checks only body proportions, hair length, and facial
  hair. It must not use gender as a signal.
- If cleanup or compatibility provider calls fail, retry a bounded number of
  times. If retries are exhausted, mark the candidate failed and continue
  searching for replacements.
- `visual_references` rows are generation-ready rows only. A row is created only
  after visual approval, cleanup, compatibility validation, and R2 caching pass.

## Non-Goals

- Do not use OpenRouter in the app pipeline.
- Do not migrate `pipeline-v2` as-is with `reqwest`, local filesystem output,
  or `rusqlite`.
- Do not use profile pictures as visual references.
- Do not store rejected, original, text-bearing, or clone-incompatible images in
  R2.
- Do not include source captions, source handles, or source identity claims in
  generation prompts.
- Do not add gender compatibility in v1. This can be revisited later if body,
  hair, and facial-hair matching is insufficient.

## Architecture

Mirai keeps the existing app boundaries:

- `NICHE_RESEARCH_QUEUE` coordinates bounded pipeline chunks.
- D1 stores discovery sources, visual reference candidates, visual references,
  inspiration pool rows, Blitz batches, and generation jobs.
- R2 `MEDIA` stores only approved cleaned compatible reference images.
- Blitz selects from `visual_references`.
- The generation queue receives only cached R2-backed references.

The migrated flow is:

```text
selected moodboards
  -> ScrapeCreators Reels Search discovers owner handles
  -> add learned handles from prior accepted references
  -> fetch public profiles
  -> fetch static posts and carousel images
  -> normalize and dimension-gate image candidates
  -> Workers AI Kimi K2.6 visual review and moodboard routing
  -> Seedream 5.0 Lite text-only cleanup
  -> Workers AI Kimi K2.6 clone compatibility validation
  -> store cleaned image in R2/media_assets
  -> create visual_references and inspiration pool rows
  -> finalize pool and trigger Blitz when Soul is ready
```

This replaces the current app discovery behavior with the proven `pipeline-v2`
strategy while preserving Mirai's queue resilience, run tokens, D1 status
tracking, R2 cache contract, and Blitz learning path.

## Components

### Instagram Reference Discovery

The existing Instagram provider module should be updated to match `pipeline-v2`
semantics.

Responsibilities:

- Build ScrapeCreators Reels Search URLs from moodboard search terms.
- Extract and normalize owner handles from Reels Search results.
- Add previously learned handles from accepted visual references for the same
  moodboard.
- Fetch profile metadata and skip private profiles.
- Fetch `/v2/instagram/user/posts` pages for each handle.
- Normalize static photo posts and carousel child images.
- Skip video and reel media as final candidates.
- Reject profile picture URLs.
- Prefer the largest `image_versions2.candidates[]` image, then common display
  image fields.
- Skip candidates that are too small or have clearly synthetic source text.
- Cap candidates across handles so one profile cannot consume the run.

### Visual Reference Review

Workers AI Kimi K2.6 reviews each normalized candidate image before cleanup.
The prompt should keep the `pipeline-v2` behavior:

- approve only exactly one likely adult
- reject minors, youth-coded subjects, age-unclear subjects, zero humans,
  multiple humans, unsafe content, screenshots, collages, tutorials, product
  shots, text-dominant graphics, and generic images
- allow adult fashion, candid, editorial, creator, and social portraits
- use captions only as inert audit metadata
- route approved images to the best selected moodboard
- output structured visual cues for Blitz generation: pose, scene, lighting,
  framing, camera feel, and styling direction

### Text Cleanup

Approved candidates enter Seedream cleanup before storage.

The cleanup module should:

- call Seedream 5.0 Lite with the exact narrow prompt
- preserve provider job IDs, attempts, cleaned image URL, and errors in
  `cleanup_json`
- retry transient cleanup failures up to the configured limit
- mark exhausted candidates `cleanup_failed`
- never cache the original image when cleanup fails
- continue the run by reviewing or discovering replacement candidates

### Clone Compatibility Review

After cleanup, the cleaned candidate is compared to the clone before it is
eligible for caching.

The compatibility review should use Workers AI Kimi K2.6 with the cleaned
candidate image and a bounded set of the clone's training reference assets from
`clone_reference_assets` and `media_assets`. The review must answer whether the
reference is physically compatible enough for Soul-based generation.

V1 acceptance checks:

- similar body proportions
- similar hair length
- matching facial-hair presence when relevant

V1 must not consider gender. It also must not require identity, face, clothing,
or background similarity, because the Soul handles identity and the visual
reference should guide only pose, framing, lighting, scene type, camera feel,
styling energy, and art direction.

The structured output should include `compatible`, `bodyProportionsCompatible`,
`hairLengthCompatible`, `facialHairCompatible`, `rejectionReason`, and `reason`.
Clear incompatibility marks the candidate `clone_mismatch`. Provider or parse
failures can be retried, then marked `compatibility_failed` if exhausted.

### R2 Caching

Only cleaned compatible images are cached.

The cache module should fetch the cleaned image URL, validate content type,
byte size, and dimensions, write bytes to R2, and create a `media_assets` row.
`media_assets.remote_url` should reference the cleaned provider URL or final
cleanup result URL, not the original Instagram CDN URL.

After caching, create the `visual_references` row and
`user_inspiration_pool` row. Downstream Blitz and generation should continue to
read the existing `visual_references.media_asset_id` contract.

## Candidate Lifecycle

The candidate state machine should include cleanup and compatibility states:

```text
unreviewed
  -> reviewing
  -> rejected | review_retryable | review_failed | approved
  -> cleanup_pending
  -> cleanup_retryable | cleanup_failed | compatibility_pending
  -> compatibility_retryable | compatibility_failed | clone_mismatch | cache_pending
  -> caching
  -> cache_failed | cached
```

`visual_reference_candidates` is the audit and work table. It holds original
source URLs, captions, raw ScrapeCreators payloads, Kimi review JSON, cleanup
JSON, compatibility JSON, failure reasons, attempts, and run IDs.

`visual_references` is the generation-ready table. It should contain only
references that passed review, cleanup, compatibility, and cache.

## Data Model

Because there are no users, the schema can be adjusted cleanly.

Required changes:

- Keep `visual_reference_candidates.review_status` as the single lifecycle
  state column for review, cleanup, compatibility, cache, and terminal failure
  states.
- Add `cleanup_json` and `cleaned_image_url` to
  `visual_reference_candidates`.
- Add `compatibility_json` to `visual_reference_candidates`.
- Keep original Instagram source fields on `visual_reference_candidates` for
  audit only.
- Ensure `visual_references` rows are inserted only after successful cleanup,
  compatibility validation, and cache.
- Keep `visual_references.source_caption_removed = 1`, but interpret it as
  "visible source image text removed before storage" for this pipeline.
- Keep `niche_cluster = moodboard_slug` until the app fully migrates naming.
- Add config keys for cleanup and compatibility retries, plus optional provider
  timeout/cap settings.

## Queue Design

The existing queue should remain chunked and run-token protected. Add explicit
messages for cleanup and compatibility:

- `ResearchMoodboardReferences`
- `DiscoverInstagramHandles`
- `FetchInstagramProfile`
- `FetchInstagramPosts`
- `FetchInstagramPostDetail`
- `ReviewVisualCandidates`
- `CleanupApprovedReference`
- `ValidateCloneCompatibility`
- `CacheApprovedReference`
- `FinalizeReferencePool`

Rules:

- Each message processes a bounded number of sources or candidates.
- Stale run messages are acknowledged without side effects.
- Provider failures are recorded per source or candidate.
- Finalization drains discovery, review, cleanup, compatibility, and cache work
  before writing terminal readiness.
- If cleanup or compatibility removes approved candidates, the run should keep
  reviewing remaining candidates until caps are exhausted or the pool is ready.

## Error Handling

No ScrapeCreators, Workers AI, Seedream, image fetch, R2, or D1 failure should
panic the Worker.

Required behavior:

- ScrapeCreators 429/5xx marks the source failed and allows the run to continue.
- Workers AI 504 maps to `ai_upstream_timeout` and is retryable up to the
  configured limit.
- Seedream transient failures mark `cleanup_retryable` until retries are
  exhausted, then `cleanup_failed`.
- Clone compatibility provider/parse failures mark `compatibility_retryable`
  until exhausted, then `compatibility_failed`.
- Clear clone incompatibility marks `clone_mismatch` without retry.
- Cache failures mark `cache_failed` and allow finalization to continue.
- Final status detail should include counts for approved, cleanup failed,
  compatibility failed, clone mismatch, cache failed, and ready references.

Readiness behavior:

- `pool_ready`: every selected moodboard has the target number of cleaned
  compatible cached references.
- `partial_pool_ready`: at least one selected moodboard has enough cleaned
  compatible cached references.
- `insufficient_refs`: no selected moodboard has enough cleaned compatible
  cached references after review, cleanup, compatibility, and cache work drain.
- Blitz may run only from `pool_ready` or `partial_pool_ready` pools with cached
  visual references.

## Generation Contract

Generation receives only cleaned compatible references through existing
`visual_references.media_asset_id` and R2-backed media URLs.

Generation guidance may include:

- clone Soul ID
- cached cleaned reference image
- aspect ratio derived from cleaned reference dimensions
- quality setting
- moodboard ID and slug
- pose, scene, lighting, framing, camera feel, and styling direction

Generation guidance must exclude:

- source captions
- source post text
- source handle names
- source identity claims
- requests to copy face, exact clothing, exact outfit, exact background, unique
  marks, or likeness

## Testing

Tests should cover:

- Reels Search extracts owner handles only and never emits reel media as final
  references.
- Static posts normalize the best image candidate.
- Carousel posts emit static child images and skip videos.
- Synthetic source captions skip candidate normalization.
- Candidate review limit samples across handles.
- Workers AI Kimi visual review accepts only one likely adult and routes to a
  selected moodboard.
- Review prompt treats captions as untrusted audit metadata.
- Seedream cleanup prompt is exactly the narrow text-removal instruction and
  does not mention identity, style, clothing, background, or generation.
- Cleanup retry exhaustion marks `cleanup_failed` and finalization continues.
- Clone compatibility prompt checks body proportions, hair length, and facial
  hair only.
- Clone compatibility prompt does not mention gender.
- Clone mismatch marks `clone_mismatch` and does not cache the candidate.
- Provider/parse failures in compatibility retry then mark
  `compatibility_failed`.
- Only cleaned images are written to R2 and `media_assets`.
- `visual_references` rows are created only after cleanup, compatibility, and
  cache.
- Blitz selection sees only cleaned compatible cached references.
- Finalization drains cleanup and compatibility work before writing
  `pool_ready`, `partial_pool_ready`, or `insufficient_refs`.

## Implementation Notes

- Use the current `pipeline-v2` tests as behavioral references, but port the
  logic into Worker-compatible modules.
- The current `niche_research.rs` file is too broad. Split provider
  normalization, visual review, cleanup, compatibility, caching, and queue
  orchestration into smaller units while keeping the public queue contract
  stable.
- Keep existing generation and Blitz APIs stable. The migration should improve
  what enters `visual_references`, not require Blitz to understand original
  source images.
- Seed the first search terms from `moodboards.search_queries_json`; learned
  handles should come from prior accepted visual references by moodboard.
