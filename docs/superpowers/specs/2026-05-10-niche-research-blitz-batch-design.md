# Niche Research and Blitz Batch System Design

Date: 2026-05-10

## Status

Design approved in brainstorming. This document defines the second Rust
backend implementation slice: niche research visual reference pools,
the generation queue handler, and the Blitz batch system.

## Product Scope

This slice builds the niche research pipeline and Blitz batch system on top
of the existing Rust Product Worker (40 commits merged, 42 tests passing).
The existing clone training queue, auth, media, onboarding bubbles, and
account routes remain unchanged except where noted.

Three capabilities ship together as a full vertical slice:

1. Niche research queue handler: scrapes organic creator content, extracts
   visual references with single-human presence, builds per-clone visual
   reference pools.
2. Generation queue handler: submits Higgsfield image-guided Soul v2 jobs,
   manages credits, assembles Blitz batches.
3. Blitz batch API routes: serves ready batches, records swipe feedback,
   triggers pre-fetched next-batch generation with metadata influence.

## Key Product Decisions

- Bubbles are per-clone, not per-user. Each clone gets its own bubble
  selection, niche research, and visual reference pool.
- Blitz is per-clone. The user selects which clone to Blitz.
- Generation limits are per-user (not per-clone): 10 images/day free,
  50 images/day pro. Daily reset. All configurable.
- Batch size is 5 (configurable). Each image in a batch consumes 1
  generation credit.
- Swipe actions are like (right) or dislike (left) only.
- Influence is metadata-based re-ranking (no extra model calls). Aesthetic
  tags, niche clusters, and source platforms from liked/disliked images
  re-rank which visual references get selected for the next batch.
- Visual references require exactly one human presence. Multiple humans
  or no humans are rejected.
- Research platform allowlist is TikTok and Instagram only. Reddit, YouTube,
  and Pinterest are out of scope for this slice.
- Research queries and accepted source content must avoid synthetic/generation
  topics. Source text containing terms such as "AI", "generated",
  "synthetic", "render", "CGI", "avatar", "Midjourney", "Stable Diffusion",
  or "DALL-E" is rejected before it can become knowledge or a visual
  reference.
- Reference photos must look like organic creator content: captured on
  iPhone/phone cameras, compact digital cameras, or casual photoshoot-style
  setups. Overly professional studio, stock, render-like, polished campaign,
  or synthetic-looking imagery is rejected.
- Reference photos must be recent. Use source post/video publish time as
  the default freshness signal, or embedded capture metadata if available.
  Known source dates older than the rolling 5-year cutoff are rejected.
  Unknown dates are not rejected automatically, but they rank lower and must
  come from recent-first research queries/results so the pipeline does not
  pull very old references.
- Workers AI Kimi K2.6 is the only model provider for app analysis tasks:
  `@cf/moonshotai/kimi-k2.6` through the Workers AI binding. OpenRouter and
  OpenCode are not used. Higgsfield MCP remains the provider for Soul clone
  training and image generation only.
- ScrapeCreators HTTP API for platform scraping (TikTok, Instagram). No CLI.

## Architecture

### Cold Start Flow

```text
User finishes onboarding → saves 5+ bubbles for clone
  → niche_research_queue: SeedFromBubbles message
  → Niche research runs 5 stages
  → ≥5 visual references accepted
  → First blitz_batch created (status: generating)
  → generation_queue: GenerateBlitzBatch message (5 jobs)
  → All 5 complete → batch status: ready
  → User sees first Blitz deck
```

### Steady-State Pre-Fetch Pipeline

```text
Batch 1: cold start, no influence
  User starts swiping → trigger Batch 2 (no influence, pre-fetch)
  User finishes (5 swipes)

Batch 2: no influence (pre-generated)
  User starts swiping → trigger Batch 3 (influence from Batch 1)
  User finishes (10 total swipes)

Batch 3: influenced by Batch 1
  User starts swiping → trigger Batch 4 (influence from Batch 1+2)
  ...and so on
```

Rule: Batch N+1 uses feedback from Batches 1..N-1 (all previously
completed batches, never the current batch being swiped).

## New Tables

### blitz_batches

Tracks batch lifecycle from creation through completion.

```sql
CREATE TABLE IF NOT EXISTS blitz_batches (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  batch_number INTEGER NOT NULL,
  batch_size INTEGER NOT NULL DEFAULT 5,
  status TEXT NOT NULL DEFAULT 'pending',
  influence_json TEXT NOT NULL DEFAULT '{}',
  generation_count INTEGER NOT NULL DEFAULT 0,
  like_count INTEGER NOT NULL DEFAULT 0,
  dislike_count INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  ready_at TEXT,
  served_at TEXT,
  completed_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  UNIQUE(clone_id, batch_number)
);
```

Status values: `pending`, `generating`, `ready`, `active`, `completed`,
`failed`.

### blitz_swipes

Records individual like/dislike decisions per output.

```sql
CREATE TABLE IF NOT EXISTS blitz_swipes (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  batch_id TEXT NOT NULL,
  generation_output_id TEXT,
  visual_reference_id TEXT,
  action TEXT NOT NULL,
  output_metadata_json TEXT NOT NULL DEFAULT '{}',
  swipe_index INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (batch_id) REFERENCES blitz_batches(id) ON DELETE CASCADE,
  FOREIGN KEY (generation_output_id) REFERENCES generation_outputs(id)
    ON DELETE SET NULL,
  FOREIGN KEY (visual_reference_id) REFERENCES visual_references(id)
    ON DELETE SET NULL,
  UNIQUE(batch_id, swipe_index)
);
```

`output_metadata_json` snapshots the visual reference's aesthetic tags,
niche cluster, and source platform at swipe time. This is the data used
for influence computation.

### generation_daily_usage

Tracks per-user daily generation consumption with configurable limits.

```sql
CREATE TABLE IF NOT EXISTS generation_daily_usage (
  user_id TEXT NOT NULL,
  usage_date TEXT NOT NULL,
  images_generated INTEGER NOT NULL DEFAULT 0,
  images_limit INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (user_id, usage_date)
);
```

### blitz_config

App-level configurable parameters. Stored in D1 so they can be changed
without redeploying. Environment variables or feature flags can override.

```sql
CREATE TABLE IF NOT EXISTS blitz_config (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

Initial keys:

- `batch_size`: `5`
- `free_daily_limit`: `10`
- `pro_daily_limit`: `50`
- `influence_window`: `5` (swipes before influence kicks in)
- `min_visual_refs`: `5` (minimum pool size before generation)
- `platform_engagement_thresholds_json`: `{"tiktok":{"likes":10000},"instagram":{"likes":5000}}`
  (platform-normalized quality gates for visual reference candidates)
- `freshness_window_years`: `5` (maximum age for accepted source media)
- `allow_unknown_source_date`: `true` (allow candidates without a verifiable
  source publish/capture date, but rank them lower)
- `recent_search_window`: `last-year` (default search freshness window where
  supported)
- `cluster_relevance_threshold`: `0.72` (minimum score for cluster expansion)
- `expand_clusters_per_run`: `4` (maximum relevant clusters to deepen per
  niche research run)
- `max_reference_generation_uses`: `4` (maximum successful generations per
  visual reference, including the first use)
- `scrape_delay_ms`: `1000` (delay between ScrapeCreators API calls)

## Modified Existing Tables

### inspiration_bubbles

Change: `clone_id` becomes NOT NULL. Bubbles are per-clone.

```sql
-- Migration: make clone_id required
-- Existing rows with NULL clone_id should be cleaned up or backfilled
ALTER TABLE inspiration_bubbles
  -- SQLite does not support ALTER COLUMN; migration will recreate table
```

### generation_jobs

Add: `blitz_batch_id` column linking generation jobs to their Blitz batch.
NULL for non-Blitz generations (future Create flow).

```sql
ALTER TABLE generation_jobs ADD COLUMN blitz_batch_id TEXT
  REFERENCES blitz_batches(id) ON DELETE SET NULL;
```

### discovery_items

Add: normalized source publish/capture date for freshness filtering.

```sql
ALTER TABLE discovery_items ADD COLUMN source_published_at TEXT;
```

`source_published_at` stores the best available source media date from the
platform payload. For TikTok and Instagram this is normally the video/post
creation timestamp. If reliable image capture metadata is available, it can
override the platform publish timestamp. Items with no trustworthy date remain
eligible only when `allow_unknown_source_date` is true, rank lower than dated
recent items, and still need to pass Kimi's visual freshness check.

### visual_reference_candidates

Add: `clone_id` column scoping candidates to a clone's niche research, plus
a snapshot of source freshness metadata.

```sql
ALTER TABLE visual_reference_candidates ADD COLUMN clone_id TEXT
  REFERENCES clone_profiles(id) ON DELETE CASCADE;
ALTER TABLE visual_reference_candidates ADD COLUMN source_published_at TEXT;
ALTER TABLE visual_reference_candidates ADD COLUMN freshness_status TEXT
  NOT NULL DEFAULT 'unreviewed';
```

`freshness_status` values: `unreviewed`, `recent`, `unknown_date`, `too_old`.

### visual_references

Add: `clone_id` column. Accepted visual references belong to a specific
clone's pool. Also snapshot source freshness so accepted references remain
auditable if the source item changes or expires.

```sql
ALTER TABLE visual_references ADD COLUMN clone_id TEXT
  REFERENCES clone_profiles(id) ON DELETE CASCADE;
ALTER TABLE visual_references ADD COLUMN source_published_at TEXT;
ALTER TABLE visual_references ADD COLUMN generation_use_count INTEGER
  NOT NULL DEFAULT 0;
ALTER TABLE visual_references ADD COLUMN last_used_batch_id TEXT
  REFERENCES blitz_batches(id) ON DELETE SET NULL;
ALTER TABLE visual_references ADD COLUMN last_liked_at TEXT;
```

### niche_research_queries

Add: `clone_id` column. Research queries scoped per-clone.

```sql
ALTER TABLE niche_research_queries ADD COLUMN clone_id TEXT
  REFERENCES clone_profiles(id) ON DELETE SET NULL;
```

### niche_knowledge

Add: `clone_id` column. Knowledge bits scoped per-clone.

```sql
ALTER TABLE niche_knowledge ADD COLUMN clone_id TEXT
  REFERENCES clone_profiles(id) ON DELETE SET NULL;
```

### user_inspiration_pool

Add: `clone_id` column (NOT NULL). Pool entries scoped per-clone.
Update unique constraint to `(clone_id, visual_reference_id)`.

```sql
ALTER TABLE user_inspiration_pool ADD COLUMN clone_id TEXT NOT NULL
  REFERENCES clone_profiles(id) ON DELETE CASCADE;
-- Recreate unique constraint: UNIQUE(clone_id, visual_reference_id)
```

## New Indexes

```sql
CREATE INDEX IF NOT EXISTS idx_blitz_batches_clone_status
  ON blitz_batches(clone_id, status, batch_number DESC);
CREATE INDEX IF NOT EXISTS idx_blitz_batches_user_date
  ON blitz_batches(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_blitz_swipes_batch
  ON blitz_swipes(batch_id, swipe_index);
CREATE INDEX IF NOT EXISTS idx_blitz_swipes_clone
  ON blitz_swipes(clone_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_generation_daily_usage_date
  ON generation_daily_usage(user_id, usage_date DESC);
CREATE INDEX IF NOT EXISTS idx_generation_jobs_batch
  ON generation_jobs(blitz_batch_id) WHERE blitz_batch_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_generation_jobs_visual_ref
  ON generation_jobs(input_visual_reference_id, status)
  WHERE input_visual_reference_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_discovery_items_platform_published
  ON discovery_items(platform, source_published_at DESC);
CREATE INDEX IF NOT EXISTS idx_visual_references_clone
  ON visual_references(clone_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_visual_references_clone_published
  ON visual_references(clone_id, source_published_at DESC)
  WHERE source_published_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_visual_references_clone_reuse
  ON visual_references(clone_id, generation_use_count, last_liked_at);
CREATE INDEX IF NOT EXISTS idx_visual_ref_candidates_clone
  ON visual_reference_candidates(clone_id, human_presence_status);
CREATE INDEX IF NOT EXISTS idx_niche_research_queries_clone
  ON niche_research_queries(clone_id, status);
CREATE INDEX IF NOT EXISTS idx_niche_knowledge_clone
  ON niche_knowledge(clone_id, cluster);
```

## API Routes

### New Routes

`GET /api/blitz/current?clone_id={clone_id}`

Returns the current or next ready Blitz batch for a clone.

Response when batch is ready:

```json
{
  "batch": {
    "id": "batch_abc",
    "batch_number": 3,
    "status": "ready",
    "images": [
      {
        "output_id": "out_1",
        "media_url": "/api/media/m_1",
        "visual_reference_id": "vref_1",
        "swipe_index": 0,
        "swiped": false
      }
    ]
  },
  "usage": {
    "images_today": 15,
    "daily_limit": 50,
    "remaining": 35
  },
  "next_batch_status": "generating"
}
```

Response when no batch is ready:

```json
{
  "batch": null,
  "status": "generating",
  "progress": {
    "phase": "niche_research",
    "detail": "Scraping visual references..."
  }
}
```

`POST /api/blitz/swipe`

Records a like or dislike on a Blitz image.

Request:

```json
{
  "batch_id": "batch_abc",
  "output_id": "out_1",
  "action": "like"
}
```

Response:

```json
{
  "swipe_index": 0,
  "batch_progress": "1/5",
  "batch_complete": false,
  "next_batch_triggered": true
}
```

Side effects:

- Records `blitz_swipes` row with metadata snapshot.
- If action is `like`, updates the linked visual reference's `last_liked_at`
  so it can be reused in future batches up to the configured cap.
- On first swipe of batch: triggers next batch generation (uses influence
  from all previously completed batches).
- On last swipe (5/5): marks batch `completed`.
- Rejects duplicate swipe for same swipe_index.

`GET /api/blitz/history?clone_id={clone_id}&limit=10`

Returns completed batch summaries for a clone.

### Modified Routes

`POST /api/onboarding/bubbles`

Changes:

- Validate at least 5 bubbles selected (required for niche research).
- Save bubbles with `clone_id` NOT NULL.
- Send `niche_research_queue` message with type `seed_from_bubbles`.

`GET /api/account/usage`

Changes:

- Add `generation_usage` field with `images_today`, `daily_limit`,
  `remaining`, and `limit_resets_at`.

## Niche Research Queue Handler

### Message Types

`SeedFromBubbles` (initial trigger from onboarding):

```json
{
  "type": "seed_from_bubbles",
  "user_id": "user_...",
  "clone_id": "clone_...",
  "bubble_ids": ["b1", "b2", "b3", "b4", "b5"],
  "moderation_level": 4,
  "platforms": ["tiktok", "instagram"]
}
```

`RefreshPool` (periodic or on-demand refresh):

```json
{
  "type": "refresh_pool",
  "user_id": "user_...",
  "clone_id": "clone_...",
  "reason": "pool_depleted"
}
```

### Pipeline Stages

Stage 1: Seed extraction.

- Load selected bubbles (title, vibe_summary, search_queries_json).
- Build active niche guardrails from the selected bubbles:
  - `active_niche`: short label synthesized from the selected bubble set.
  - `focus_keywords`: 12-30 normalized keywords/phrases that define the
    clone's current niche.
  - `negative_focus_keywords`: terms that would pull research away from the
    selected niche.
- Run structured seed extraction through Workers AI Kimi K2.6:
  `@cf/moonshotai/kimi-k2.6`.
- Extract 15-25 search queries for organic visual content on the platform
  allowlist only.
- Reject or rewrite any query that contains synthetic/generation terms.
- Insert into `niche_research_queries` with `clone_id`.

Stage 2: Platform scraping.

- For each query and platform, call ScrapeCreators HTTP API:
  - TikTok: `GET /v1/tiktok/search/keyword?sort_by=date-posted&date_posted=last-6-months&trim=true`
  - TikTok: `GET /v1/tiktok/search/hashtag?trim=true` (first page only;
    validate `create_time`/publish timestamp before accepting)
  - Instagram: `GET /v2/instagram/reels/search?date_posted=last-year`
- Rate limit: 1 second delay between calls (configurable).
- Normalize each result's `source_published_at` from the platform payload.
  Use the source media/post publish timestamp, or embedded capture
  metadata if reliably available.
- Apply recency-first filtering before insertion: prefer recent search windows
  (`last-6-months` for TikTok keyword search, `last-year` for Instagram reels)
  and reject results with known dates older than `freshness_window_years`.
  With today's date of May 11, 2026, the stale cutoff is May 11, 2021.
  Unknown source dates are allowed when `allow_unknown_source_date` is true,
  but they rank lower than dated recent items and still need visual freshness
  approval.
- Store accepted results in `discovery_sources` and `discovery_items`.
- Deduplicate by `(platform, external_id)`.
- Enforce platform allowlist at both request construction and response
  ingestion. Results from any other platform are dropped.

Stage 3: Knowledge extraction.

- Batch discovery item text (titles, descriptions, captions).
- Drop items whose text contains synthetic/generation terms before extraction.
- Run structured extraction through Workers AI Kimi K2.6.
- Extract 30-60 knowledge bits and 15-30 deeper queries.
- Insert into `niche_knowledge` and `niche_research_queries` with
  `clone_id`.
- Reject or rewrite extracted queries/knowledge if they contain
  synthetic/generation terms.
- Reject or rewrite extracted queries/knowledge that do not overlap the
  active niche guardrails.

Stage 4: Clustering.

- Send all knowledge bits and queries for this clone to Workers AI Kimi K2.6.
- Group into subtopic clusters with kebab-case names.
- Score each cluster against `active_niche`, `focus_keywords`, and
  `negative_focus_keywords`.
- Store a `cluster_relevance_score` from 0.0 to 1.0 and a short
  `cluster_relevance_reason`.
- Skip clusters below `cluster_relevance_threshold`; do not generate deeper
  search queries or scrape for them.
- Sort remaining clusters by relevance score and expand at most
  `expand_clusters_per_run` clusters per research run.
- Update `cluster` field on `niche_knowledge` and
  `niche_research_queries`.
- For each selected relevant cluster, generate 3-5 deeper search queries that
  include at least one focus keyword and avoid negative focus keywords.
- Run Stage 2 scraping on the deeper queries (one deepening round).

Stage 5: Visual reference selection.

- Filter discovery items by platform-normalized engagement thresholds.
  TikTok and Instagram use like counts when present.
- Filter for items with image URLs.
- For each candidate image:
  - Insert into `visual_reference_candidates` with `clone_id`.
  - Run Workers AI Kimi K2.6 vision check for single-human presence,
    organic-photo quality, and visual freshness cues.
  - Accept: exactly one human, confidence above 0.7.
  - Accept only if the image looks like phone, compact digital camera, or
    casual photoshoot content.
  - Accept if `source_published_at` is within the rolling 5-year freshness
    window, or if the date is unknown but the item came from a recent-first
    search path and passes visual freshness review.
  - Reject: no human, multiple humans, low quality, text-heavy, overly
    professional/studio, stock/campaign, render-like, synthetic-looking, or
    sourced from blocked text terms. Reject known-stale source media with
    `freshness_status` of `too_old`.
  - Accepted candidates inserted into `visual_references` with
    `clone_id`, aesthetic tags, human presence type, and score.
- If 5 or more visual references accepted and the clone's
  `soul_status` is `ready`: create first `blitz_batches` row and
  enqueue generation.
- If 5 or more visual references accepted but `soul_status` is not
  `ready`: mark research as `pool_ready_awaiting_soul`. The first
  Blitz batch is created when the clone training queue marks the Soul
  ready.
- If fewer than 5: mark niche research as `insufficient_refs`. Can
  retry with broader queries later.
- If the visual reference pool is depleted during steady-state (all
  references used), send a `RefreshPool` message to replenish.

### Structured Analysis Prompts

Seed extraction prompt:

```text
Given these aesthetic directions for a creator clone:
{bubble titles + vibe summaries}

Generate 15-25 search queries for finding recent organic creator visual
content on TikTok and Instagram.
Focus on: single-person outfit/lifestyle inspiration, creator aesthetics,
phone-camera photos, compact-digital-camera photos, mirror shots, casual
photoshoot aesthetics, and trending visual styles.
Do not include synthetic/generation topics or terms.
Avoid outdated era labels unless the selected bubble explicitly asks for a
revival aesthetic; even then, search for current creators posting that look
within the last 5 years.

Return JSON: { "queries": [{ "query": "...", "platforms": ["tiktok", "instagram"] }] }
```

Knowledge extraction prompt (adapted from social-page):

```text
You are analyzing social media content about "{clone's niche directions}".
Extract two things:

1. QUERIES: Recurring questions or subtopics people search for.
   Format: short, searchable phrases. Extract 15-30 unique queries.

2. KNOWLEDGE BITS: Specific, actionable tips or insights.
   Deduplicate similar advice. Keep each under 30 words.
   Include source type. Do not extract from known-stale source items.

Return JSON: {
  "queries": [{ "query": "...", "source": "tiktok|instagram" }],
  "knowledge": [{ "bit": "...", "source_platform": "..." }]
}
```

Clustering prompt (adapted from social-page):

```text
Active niche: {active_niche}
Focus keywords: {focus_keywords}
Negative focus keywords: {negative_focus_keywords}

Group these knowledge bits and search queries into coherent subtopic
clusters that stay inside the active niche. Name each with a short kebab-case
label. Score each cluster's relevance to the active niche from 0.0 to 1.0.
Penalize broad lifestyle/fashion clusters that do not clearly connect to the
focus keywords, and penalize clusters that match negative focus keywords.

Return JSON: {
  "clusters": [{
    "name": "kebab-case-name",
    "bit_ids": [1, 2, 3],
    "query_ids": [1, 2],
    "description": "what this cluster covers",
    "cluster_relevance_score": 0.0,
    "cluster_relevance_reason": "why this belongs or does not belong"
  }]
}
```

Human presence detection prompt:

```text
Analyze this image. Does it contain exactly one human person, does it look
like organic creator content captured on a phone, compact digital camera, or
casual photoshoot setup, and does it avoid stale visual trends?

Return JSON: {
  "has_human": true/false,
  "human_count": 0/1/2+,
  "human_type": "full_body" | "upper_body" | "face" | "partial" | "none",
  "confidence": 0.0-1.0,
  "organic_photo_score": 0.0-1.0,
  "freshness_visual_score": 0.0-1.0,
  "capture_style": "phone" | "compact_digital_camera" | "casual_photoshoot"
                   | "professional_studio" | "stock_campaign"
                   | "render_like" | "unknown",
  "aesthetic_tags": ["minimalist", "street", "warm", ...],
  "rejection_reason": null | "no_human" | "multiple_humans"
                      | "low_quality" | "text_heavy"
                      | "too_professional" | "stock_or_campaign"
                      | "synthetic_or_render_like" | "stale_visual_trend"
}
```

## Generation Queue Handler

### Message Types

`GenerateBlitzBatch`:

```json
{
  "type": "generate_blitz_batch",
  "batch_id": "batch_...",
  "clone_id": "clone_...",
  "user_id": "user_...",
  "idempotency_key": "blitz_gen:batch_...",
  "visual_reference_ids": ["vref_1", "vref_2", "vref_3", "vref_4", "vref_5"],
  "provider_soul_id": "soul_..."
}
```

`PollGeneration`:

```json
{
  "type": "poll_generation",
  "job_id": "gen_...",
  "batch_id": "batch_...",
  "attempt": 1,
  "max_attempts": 30
}
```

### Handler Steps

For each visual reference in the batch:

1. Check Soul readiness. Verify `clone_profiles.soul_status` is `ready`
   and `provider_soul_id` is set. If not ready, skip batch.
2. Check daily quota. Load `generation_daily_usage` for user and today.
   If at limit, skip this image.
2. Materialize visual reference. Download source image to R2 if not
   already cached.
3. Create generation job. Insert `generation_jobs` row with
   `blitz_batch_id`, `input_visual_reference_id`, status `queued`.
4. Submit to Higgsfield. Authenticate, lease provider account, call
   Higgsfield MCP `text2image_soul_v2` with empty prompt, input image
   URL, and Soul ID.
5. Poll or complete. If provider returns immediately, persist output.
   If async, re-enqueue `PollGeneration` with delay.
6. On completion: download generated image, store in R2, insert
   `generation_outputs` row, settle credit, increment
   `blitz_batches.generation_count`, and increment
   `visual_references.generation_use_count` for the successful
   `input_visual_reference_id`.
7. Check batch complete. If `generation_count == batch_size`, set batch
   status to `ready`.

### Error Handling

- Provider auth failure: retry with different provider account.
- Provider quota exhausted: mark batch generating, retry later.
- Image download failure: retry 3 times, then mark individual job failed.
- All jobs fail: mark batch failed, refund all credits.
- Partial failure: mark batch ready with fewer images, deliver what
  completed.

## Visual Reference Selection with Influence

### Without Influence (Batch 1 and 2)

Select from `visual_references` where `clone_id` matches, status is
`active`, and `generation_use_count = 0`. Order by source recency first,
then human/organic/freshness score.

### With Influence (Batch 3+)

Accumulated `influence_json` on the batch:

```json
{
  "liked_tags": { "minimalist": 3, "street": 2 },
  "disliked_tags": { "neon": 2 },
  "liked_clusters": { "outfit-inspo": 2 },
  "disliked_clusters": { "formal-wear": 1 },
  "liked_platforms": { "tiktok": 3, "instagram": 2 },
  "liked_visual_reference_ids": { "vref_1": 1 }
}
```

Scoring per visual reference:

```text
base_score     = human_presence_score
tag_boost      = sum(liked_tags[tag] for tag in ref.aesthetic_tags)
tag_penalty    = sum(disliked_tags[tag] for tag in ref.aesthetic_tags)
cluster_boost  = liked_clusters.get(ref.cluster, 0)
source_recency = 1.0 if source_published_at is recent,
                 0.5 if source date is unknown,
                 0.0 if source date is known stale
reuse_boost    = 0.5 if ref.id in liked_visual_reference_ids
reuse_penalty  = 0.4 * generation_use_count

final_score = base_score
            + (tag_boost * 0.3)
            - (tag_penalty * 0.2)
            + (cluster_boost * 0.2)
            + (source_recency * 0.3)
            + reuse_boost
            - reuse_penalty
```

Variety constraints: max 2 from the same cluster, max 3 from the same
platform. Sort by `final_score` descending, take top `batch_size`.

Reference reuse rule:

- Unliked references are one-shot by default.
- A reference that receives at least one like becomes eligible for future
  batches.
- A visual reference can produce at most `max_reference_generation_uses`
  successful generations total, including the first use. Default cap: 4.
- Do not select the same `visual_reference_id` twice in a single batch.
- Increment `visual_references.generation_use_count` only for successful
  generation outputs. Failed provider jobs do not consume the reuse cap.
- Store `last_liked_at` when a swipe likes an output generated from that
  reference.
- Higgsfield can receive the same Soul ID and same visual reference image on
  repeat uses; each provider submission is a new generation job, so outputs
  are expected to vary while keeping the same reference direction.

## Entitlements and Credits

### Daily Generation Limits

- Free users: 10 images per day (configurable via `blitz_config`).
- Pro users: 50 images per day (configurable via `blitz_config`).
- Limits are per-user, not per-clone.
- Reset daily at midnight UTC (configurable reset period).
- Credits consumed per image generated, regardless of swipe result.

### Quota Enforcement

- Check `generation_daily_usage` before creating each generation job.
- If user is at limit, do not generate. Return quota info to frontend.
- Frontend shows remaining generations and next reset time.

### Credit Flow

1. Reserve: increment `generation_daily_usage.images_generated` before
   submitting provider job.
2. Settle: on successful generation, credit stays consumed.
3. Refund: on failed generation, decrement `images_generated`.
4. Idempotency: generation job idempotency key prevents double-counting.

## Reliability

- Idempotency keys on batch creation, generation jobs, swipe recording,
  credit operations, and provider submissions.
- DLQ for niche_research_queue and generation_queue.
- Stale batch reconciliation: scheduled check for batches stuck in
  `generating` status beyond a timeout.
- Provider lease release on terminal success or failure.
- Typed failure reasons for user-visible states.

## Testing Strategy

### Unit Tests

- Blitz batch lifecycle state transitions.
- Visual reference scoring with influence (tag boosts, penalties, variety).
- Daily usage quota checks (under, at, over limit, reset).
- Influence accumulation (merge swipe metadata into influence_json).
- ScrapeCreators response parsing (TikTok, Instagram).
- Human presence result validation (single-human accept, multi/none reject).
- Source freshness validation (recent known dates pass, stale known dates
  reject, unknown dates receive lower score but can pass when allowed).
- Cluster relevance guardrails (active niche keywords, negative keywords,
  threshold skipping, max expanded clusters per run).
- Reference reuse eligibility and cap (liked references can repeat, unliked
  references stay one-shot, cap stops at 4 successful generations).
- Batch pre-fetch trigger logic (first swipe triggers, subsequent no-op).
- Configurable batch size and limits.

### Route Tests

- `GET /api/blitz/current` (no batch, generating, ready, active states).
- `POST /api/blitz/swipe` (valid swipe, duplicate rejection, batch
  completion, next-batch trigger).
- `GET /api/blitz/history` (pagination, clone scoping).
- `POST /api/onboarding/bubbles` (≥5 validation, queue enqueue).
- `GET /api/account/usage` (generation usage included).

### Queue Handler Tests

- Niche research: seed extraction to query storage.
- Niche research: visual reference acceptance and rejection.
- Generation: batch creation to job submission.
- Generation: credit reservation, settlement, and refund.
- Generation: batch completion detection.
- Generation: partial failure handling.
- Generation: poll and retry lifecycle.

### Integration Tests

- Full pipeline: bubbles to niche research to generation to Blitz.
- ScrapeCreators API real calls with staging key for TikTok and Instagram.
- Workers AI Kimi K2.6 structured extraction and vision checks.
- Higgsfield MCP real generation.
- Daily limit enforcement end-to-end.

## Documentation Sources

- Workers AI Kimi K2.6:
  https://developers.cloudflare.com/workers-ai/models/kimi-k2.6/
- Workers AI structured output:
  https://developers.cloudflare.com/workers-ai/
- workers-rs D1, Queue, R2 bindings:
  https://github.com/cloudflare/workers-rs
- ScrapeCreators HTTP API:
  https://api.scrapecreators.com (x-api-key auth)
- ScrapeCreators OpenAPI spec:
  docs/scrape-creators-openapi.yaml
- Existing Rust Product Worker:
  workers/product/src/
- Existing D1 migrations:
  config/d1/migrations/1000_rust_product_core.sql
- Niche research concept prototype:
  ../social-page/pipeline/
- Higgsfield MCP endpoint:
  https://mcp.higgsfield.ai/mcp
- Previous design spec:
  docs/superpowers/specs/2026-05-08-rust-product-backend-design.md
