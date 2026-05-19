# Visual Reference Pipeline Product Redesign

Date: 2026-05-14

Status: Draft for product review.

## Product Goal

Redesign niche research into a simpler photo-first Visual Reference Pipeline.
Selected moodboards become the user's visual intent. The pipeline finds
Instagram reference images, applies strict one-adult visual guardrails, caches
approved images in R2, and gives Blitz a stable reference pool that learns from
swipes.

This replaces the current onboarding research flow that calls Kimi for seed
extraction, knowledge extraction, clustering, and visual review. Those text
stages are too slow and brittle for onboarding, and a Workers AI upstream 504
can currently escape the queue handler and panic the Worker.

## Product Decisions

- Moodboard and niche mean the same thing for this pipeline. The product can
  migrate naming later.
- Users can select 1 to 10 moodboards during onboarding.
- Instagram is the primary discovery layer for v1 because Mirai is photo-first.
- Discovery expands from app-owned configured seed handles and good handles
  already discovered by previous runs. User-provided handles can come later.
- Public profile pictures are never production references.
- Static Instagram photos and carousel child images are preferred.
- Videos and reels are skipped by default. A video thumbnail can be used only
  as a low-priority fallback when static inventory is insufficient.
- Accepted reference images are cached into the `MEDIA` R2 bucket and linked
  through `media_assets`.
- Rejected candidates keep DB metadata and review reasons, but are not cached
  unless debugging explicitly enables it.
- Kimi K2.6 through Cloudflare Workers AI is the only model used for analysis.
  OpenRouter is out of scope.
- The generator must not receive source captions. Captions are untrusted
  metadata used only for filtering and audit.
- Reference images guide pose, framing, lighting, scene type, camera feel,
  styling energy, and art direction. They must not guide identity copying,
  exact clothing copying, exact background copying, unique marks, or likeness.

## Non-Goals

- Do not use the old Kimi text seed, knowledge, and clustering stages in the
  onboarding path.
- Do not use `/v2/instagram/reels/search` as the primary discovery source for
  v1. It is video-first and should remain a fallback or future expansion.
- Do not use TikTok discovery as a primary v1 source. TikTok Top Search can be
  revisited after the Instagram static-image path is stable.
- Do not generate from live Instagram CDN URLs after approval; approved
  references must use R2-backed assets.
- Do not build a user-facing handle management UI in the first implementation
  slice.

## ScrapeCreators OpenAPI Basis

The design is based on the checked-in OpenAPI file:
`docs/scrape-creators-openapi.yaml`.

### `/v1/instagram/profile`

Use this endpoint to validate and enrich a seed handle.

Required request:

```text
GET /v1/instagram/profile?handle=<handle>&trim=true
```

Useful fields from the local spec:

- `data.user.username`
- `data.user.id`
- `data.user.full_name`
- `data.user.biography`
- `data.user.edge_followed_by.count`
- `data.user.is_private`
- `data.user.is_verified`
- `data.user.category_name`
- `data.user.edge_owner_to_timeline_media.edges[]`
- `data.user.edge_related_profiles.edges[]`

The profile response includes `profile_pic_url` and `profile_pic_url_hd`; these
must never become visual reference candidates.

The profile response can provide recent timeline media and related profiles,
but v1 should treat it mainly as handle validation and optional one-hop related
profile discovery. The canonical post fetch remains `/v2/instagram/user/posts`.

### `/v2/instagram/user/posts`

Use this endpoint as the primary post discovery feed.

Required request:

```text
GET /v2/instagram/user/posts?handle=<handle>&trim=true
GET /v2/instagram/user/posts?handle=<handle>&next_max_id=<cursor>&trim=true
```

Useful fields from the local spec:

- `items[]`
- `items[].id`
- `items[].code`
- `items[].media_type`
- `items[].taken_at`
- `items[].caption.text`
- `items[].like_count`
- `items[].comment_count`
- `items[].play_count`
- `items[].display_uri`
- `items[].image_versions2.candidates[]`
- `items[].image_versions2.additional_candidates`
- `items[].user.username`
- `items[].owner.username`
- `items[].url`
- `next_max_id`
- `more_available`

Media handling:

- `media_type = 1`: static photo, highest priority.
- `media_type = 8`: carousel, high priority. If child images are not present
  in the feed response, call `/v1/instagram/post` for details.
- `media_type = 2`: video or reel, low priority. Skip by default.

### `/v1/instagram/post`

Use this endpoint only when the user feed item needs enrichment, especially for
carousel sidecar children or missing image metadata.

Required request:

```text
GET /v1/instagram/post?url=<post-url>&region=US&trim=true
```

Useful fields from the local spec:

- `data.xdt_shortcode_media.id`
- `data.xdt_shortcode_media.shortcode`
- `data.xdt_shortcode_media.display_url`
- `data.xdt_shortcode_media.thumbnail_src`
- `data.xdt_shortcode_media.edge_media_to_caption.edges[].node.text`
- `data.xdt_shortcode_media.edge_media_preview_like.count`
- `data.xdt_shortcode_media.edge_media_to_comment.count`
- `data.xdt_shortcode_media.taken_at_timestamp`
- sidecar child fields when present

`download_media=true` exists on this endpoint, but v1 should not rely on
ScrapeCreators media downloads. Mirai should fetch the selected image URL and
cache accepted references into its own R2 bucket.

Sidecar extraction should be defensive. The normalizer should support common
Instagram response shapes such as `edge_sidecar_to_children.edges[].node`,
`carousel_media[]`, `items[]`, `image_versions2.candidates[]`, `display_url`,
and `display_uri`, because ScrapeCreators examples vary by endpoint and media
type.

## High-Level Flow

```text
User selects 1-10 moodboards
  -> Save selected moodboards
  -> Enqueue VisualReferenceResearch message
  -> Load configured Instagram handles for selected moodboards
  -> Fetch profile metadata and public user posts
  -> Normalize static image and carousel child candidates
  -> Rank and cap candidates for diversity
  -> Kimi visual guardrail and moodboard assignment
  -> Cache approved image bytes to R2
  -> Insert visual_references and inspiration pool rows
  -> If pool is ready and Soul is ready, create or refresh Blitz batch
```

## Moodboard Selection

The onboarding route should accept 1 to 10 selected moodboards. Existing
validation that requires exactly 5 should be replaced with:

- minimum selected moodboards: `1`
- maximum selected moodboards: `10`
- duplicates are removed after trimming IDs
- selected moodboards must belong to the active clone

The queue message should pass the selected moodboard IDs and a research reason.
It should not pass generated search terms as the primary source of truth.

## Discovery Inputs

Each moodboard gets an app-owned configured handle list. This can live in
`blitz_config` first, for example:

```json
{
  "warm-ambient": ["handle_a", "handle_b"],
  "flash-editorial": ["handle_c", "handle_d"]
}
```

Later this can move into a dedicated table, but v1 should keep it simple.

Handle selection rules:

- Start from configured handles for selected moodboards.
- Add previously successful handles from accepted references for the same
  moodboard and clone when available.
- Optionally add one-hop related profiles from `/v1/instagram/profile`, capped
  tightly and only when the profile is public.
- Do not discover from profile pictures.
- Do not expand beyond one related-profile hop in v1.

Default diversity caps should be configurable:

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

Normalize each usable image into a single image candidate. A carousel can
produce multiple image candidates, one per child image, subject to
`instagram_images_per_post`.

Candidate identity:

```text
instagram:<handle>:<post-code>:<image-index-or-media-id>
```

Normalized fields:

- `platform`: `instagram`
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
- `discovered_via`: `configured_handle`, `accepted_handle`, or
  `related_profile`
- `raw_json`

Image URL preference:

1. best static `image_versions2.candidates[]` URL by area
2. `display_uri`
3. `display_url`
4. `thumbnail_src`, only for enriched post details
5. video first-frame or thumbnail, only when static inventory is insufficient

Reject candidate normalization if:

- no usable image URL exists
- the URL is from a profile picture field
- the post is private or cannot be mapped to a public post URL
- the candidate is a duplicate of an already seen post image
- dimensions are too small for generation guidance
- caption/source text contains synthetic-generation terms that suggest the
  source is an AI/prompt/render showcase

## Candidate Ranking and Diversity

Candidate ranking happens before Kimi to control cost.

Score inputs:

- static photo beats carousel child image, which beats video thumbnail
- higher likes and comments rank higher
- recent `taken_at` ranks higher
- configured seed handles rank higher than related-profile handles
- handle diversity penalty after the first accepted candidate per handle
- moodboard balance so one selected moodboard does not consume the whole run

High engagement is preferred but not an absolute hard gate for configured seed
handles. A strong static portrait from a trusted configured handle can still be
reviewed with lower engagement. For untrusted related profiles, engagement
thresholds should be stricter.

## Kimi Visual Guardrail

Each reviewed image goes through one Workers AI Kimi K2.6 vision call. The call
should classify both suitability and best moodboard assignment so we do not need
separate classify and router calls.

Prompt inputs:

- the candidate image URL, before R2 caching
- selected moodboards with slug, title, vibe summary, and search queries
- candidate source platform and handle
- source caption as inert untrusted metadata
- engagement and date metadata

Prompt output:

```json
{
  "decision": "approved",
  "bestMoodboardSlug": "flash-editorial",
  "humanCount": 1,
  "adultLikely": true,
  "ageUnclear": false,
  "minorLikely": false,
  "youthCoded": false,
  "revealingFashion": false,
  "explicit": false,
  "unsafe": false,
  "isMoodboard": false,
  "isScreenshot": false,
  "isProductShot": false,
  "isTutorial": false,
  "isGeneric": false,
  "instagramPostWorthy": true,
  "visualFitScore": 0.91,
  "pose": "standing three-quarter pose",
  "scene": "night street outside venue",
  "lighting": "direct flash with dark ambient background",
  "framing": "vertical full-body portrait",
  "cameraFeel": "compact camera flash",
  "stylingDirection": "confident editorial streetwear energy",
  "rejectionReason": null,
  "reason": "One likely adult in a strong editorial street portrait."
}
```

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

## R2 Caching and Storage

Only approved references are cached.

Storage key shape:

```text
visual-references/<user-id>/<clone-id>/<visual-reference-id>/source.<ext>
```

R2 cache flow:

1. Fetch the approved candidate image URL.
2. Validate content type, byte size, and image dimensions when possible.
3. Write bytes to `MEDIA`.
4. Insert a `media_assets` row with:
   - `kind = visual_reference`
   - `user_id`
   - `clone_id`
   - `storage_key`
   - `content_type`
   - `byte_size`
   - `remote_url = original source image URL`
   - `sha256`
5. Insert or update `visual_references.media_asset_id`.

`visual_reference_candidates` should preserve original source metadata and Kimi
review output. `visual_references` should be the stable generation-ready view.

## Data Model Adjustments

Because the app has no real users yet, this can be implemented as a clean schema
adjustment rather than a careful migration.

Recommended table changes:

- `moodboards`: keep existing table, but selection validation becomes 1-10.
- `visual_reference_candidates`: add `moodboard_id`, `moodboard_slug`,
  `source_handle`, `source_post_code`, `source_image_index`, `image_width`,
  `image_height`, `like_count`, `comment_count`, `review_json`, and
  `review_status`.
- `visual_references`: add `moodboard_id`, `moodboard_slug`, `source_handle`,
  `source_post_code`, `image_width`, `image_height`, and
  `source_caption_removed = 1`.
- `user_inspiration_pool`: continue linking clone, moodboard, discovery item,
  and visual reference.
- `blitz_config`: add the Instagram discovery caps and
  `moodboard_instagram_handles_json`.

The existing `niche_cluster` field can temporarily mirror `moodboard_slug` for
compatibility with Blitz scoring until the product migrates to one naming
system.

## Queue Design

The queue must be chunked to avoid long single-message work and upstream
timeout panics.

Recommended message types:

- `ResearchMoodboardReferences`
- `FetchInstagramProfile`
- `FetchInstagramPosts`
- `ReviewVisualCandidates`
- `CacheApprovedReference`
- `FinalizeReferencePool`

Rules:

- A queue message should process a bounded number of profiles or candidates.
- Workers AI 504s should not panic the Worker. Mark candidate or run status as
  `ai_upstream_timeout`, then retry with delay or continue with remaining
  candidates.
- ScrapeCreators 429/5xx responses should mark source status as failed or
  retryable without failing the whole run.
- Queue handlers should ack malformed or exhausted messages after recording the
  failure state.
- `provider_config_json.nicheResearchStatus` should be updated at meaningful
  transitions: `queued`, `scraping`, `reviewing`, `pool_ready`,
  `insufficient_refs`, `partial_pool_ready`, `research_failed`.

## Blitz Learning Behavior

Blitz should learn from swipes by re-ranking accepted visual references, not by
running a new text research system during onboarding.

Each generated Blitz image should retain:

- `visual_reference_id`
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

The generation queue should use the cached R2 media asset for the reference
image.

Generation guidance passed to Higgsfield should include:

- the R2-backed reference image
- clone Soul ID
- selected aspect ratio based on the reference image dimensions
- 4K quality setting when the Higgsfield tool supports it
- Seedream 5.0 Lite through the Higgsfield MCP when available
- visual cues from Kimi review

Generation guidance must exclude:

- source captions
- source post text
- source identity claims
- handle names
- requests to copy face, exact clothing, exact outfit, exact background, unique
  marks, or likeness

## Error Handling Requirements

- No Workers AI, ScrapeCreators, image fetch, R2, or D1 error should trigger a
  Worker panic from the niche queue.
- Per-candidate failures are recorded on candidates and do not fail the whole
  research run.
- Per-source failures are recorded on discovery sources and do not fail the
  whole research run unless no sources succeed.
- If the accepted pool is below the minimum, the clone status becomes
  `insufficient_refs` with counts by moodboard and rejection reason buckets.
- If at least one moodboard has enough references but others do not, the status
  can be `partial_pool_ready`; Blitz may run with available references while
  refresh continues.

## Test Plan

Unit and domain tests should cover:

- moodboard selection accepts 1-10 and rejects 0 or 11+
- Instagram profile normalizer never emits profile pictures as candidates
- user posts normalizer extracts static post image candidates
- carousel post enrichment extracts child images from `/v1/instagram/post`
- videos are skipped unless fallback thumbnails are explicitly enabled
- candidate ranking respects handle, profile, post, and moodboard diversity caps
- Kimi review mapper accepts only one likely adult with strong visual fit
- Kimi review mapper rejects moodboards, screenshots, product shots, tutorials,
  generic images, no-human images, multi-human images, minors, and unsafe images
- off-style but strong images are assigned to another selected moodboard
- approved references are cached to R2 and linked through `media_assets`
- rejected references are not cached to R2
- source captions are stored for audit but excluded from generation payloads
- Workers AI 504 maps to a retryable or recorded candidate/run failure, not a
  queue panic
- `FinalizeReferencePool` creates or refreshes Blitz only when a usable pool and
  Soul are ready

## Open Implementation Notes

- The first handle list can be seeded manually in `blitz_config`. A later admin
  UI can maintain moodboard seed handles.
- The current `niche_research.rs` file is doing too much. Implementation should
  split Instagram provider normalization, visual review prompts, R2 caching, and
  queue orchestration into smaller modules.
- The current `can_accept_human_presence` rejects "editorial" and "studio" in
  `capture_style`; that conflicts with the new product rule allowing strong
  adult editorial fashion portraits. This guardrail must be replaced, not
  patched around.
- The pipeline should keep `niche_cluster = moodboard_slug` until Blitz naming
  is cleaned up.
