# Niche Research and Blitz Batch System Design

Date: 2026-05-10

## Status

Design in progress. Builds on the merged Rust Product Worker (40 commits, 42
tests passing). This is the second backend implementation slice.

## Product Scope

This slice adds two connected systems:

1. **Niche research pipeline**: builds per-clone visual reference pools from
   bubble-derived search terms, ScrapeCreators scraping, Kimi K2.6 extraction,
   clustering, expansion, and visual candidate selection with human presence
   verification.

2. **Blitz batch system**: Tinder-like swipe deck with pre-generated batches,
   configurable batch size (default 5), accumulated taste feedback, and
   per-user generation quotas.

## Key Product Decisions

- Batch size is **5** (configurable via `BLITZ_BATCH_SIZE` env var).
- Free users get **10** generations per user (not per clone).
- Pro users get **50** generations per user.
- Bubbles are **per clone**, not per user. More bubble aesthetics will be added
  over time.
- Niche is **inferred from the clone's selected bubbles** -- no explicit niche
  picker. Bubble search queries and vibe summaries become seed terms.
- Every 5 swipes (likes + dislikes) produces taste metadata that influences
  future batches, but with a one-batch delay for UX speed.
- The AI model for niche research is **Kimi K2.6** via **OpenRouter** (not
  Workers AI), using the OpenAI-compatible chat completions endpoint with
  structured JSON output.

## Blitz Batch Lifecycle

The Blitz deck works like Tinder: the next batch is always pre-generating so
the user never waits.

### Generation flow with taste delay

```
User opens Blitz for clone C (no prior batches):
  1. System creates Batch 1 (5 images) from visual reference pool, NO taste influence.
  2. System immediately starts generating Batch 2 (5 images), NO taste influence.
  3. User swipes through Batch 1 (likes 3, dislikes 2).
  4. Batch 2 is already done or in progress -- it was NOT influenced by Batch 1 swipes.
  5. System starts generating Batch 3 using taste from Batch 1 swipes (first 5).
  6. User swipes through Batch 2 (likes 2, dislikes 3).
  7. System starts generating Batch 4 using taste from Batches 1+2 (first 10).
  8. ...and so on. Each new batch uses ALL prior accumulated taste.
```

The one-batch delay is intentional: the batch generating while the user swipes
cannot use that batch's swipe data because it started before swipes happened.
This keeps the UX fast without fake waiting.

### Batch states

```
pending    -> generating -> ready -> swiping -> completed
                         -> failed
```

- `pending`: batch record created, waiting for queue pickup.
- `generating`: generation jobs submitted to provider.
- `ready`: all images generated and stored. Deck can show this batch.
- `swiping`: user has seen at least one card in this batch.
- `completed`: all cards in this batch have been swiped.
- `failed`: generation failed after retries.

### Pre-generation trigger

When the user swipes the **first card** of the current batch, the system checks
whether a next batch exists in `pending`/`generating`/`ready` state. If not, it
creates the next batch. This means:

- Batch N+1 starts generating when user swipes card 1 of Batch N.
- Batch N+1 does NOT use Batch N's taste (Batch N is not yet completed).
- Batch N+2 starts generating when user swipes card 1 of Batch N+1.
- Batch N+2 uses taste from all completed batches (Batch 1 through N).

### Quota enforcement

Quotas are per user, not per clone:

| Plan | Generations per user | Batches at size 5 |
|------|---------------------|-------------------|
| Free | 10                  | 2                 |
| Pro  | 50                  | 10                |

Quota resets daily at midnight UTC. The backend checks remaining quota before
creating a new batch. If quota is exhausted, the API returns the current batch
(if any) plus a `quotaExhausted: true` flag.

Quota is tracked via `generation_jobs` count for the user where
`DATE(queued_at) = DATE('now')`, not via the credit ledger (credits are for
future monetization of individual features).

## Niche Research Pipeline

Ported from `social-page/pipeline` concepts into the Rust Worker's
`NICHE_RESEARCH_QUEUE` consumer. Does NOT run the Node CLI. Calls
ScrapeCreators HTTP API and Kimi K2.6 (OpenRouter) from Rust.

### Pipeline stages

**Stage 1 -- Seed**

Triggered when user saves bubbles for a clone (`POST /api/onboarding/bubbles`).

1. Collect `search_queries_json` from all selected bubbles for the clone.
2. For each search query:
   a. Call ScrapeCreators `GET /v1/reddit/search` with `query`, `sort=top`,
      `timeframe=month`.
   b. Call ScrapeCreators `GET /v1/tiktok/search/keyword` with `query`,
      `sort_by=relevance`.
   c. For top Reddit posts, call `GET /v1/reddit/post/comments` for comment
      extraction.
3. Aggregate scraped text content.
4. Send to Kimi K2.6 (OpenRouter) with extraction prompt requesting JSON:
   `{ queries: [{ query, source }], knowledge: [{ bit, source_platform }] }`.
5. Store extracted queries in `niche_research_queries` and knowledge bits in
   `niche_knowledge`, both scoped to `user_id` and linked to `bubble_id`.

**Stage 2 -- Expand (cluster and deepen)**

Runs as a follow-up queue message after seed completes.

1. Load all knowledge bits and unused queries for the clone's user.
2. Send to Kimi K2.6 for clustering:
   `{ clusters: [{ name, bit_ids, query_ids, description }] }`.
3. Update cluster assignments in D1.
4. For each cluster, ask Kimi K2.6 for 3-5 deeper search queries.
5. Scrape deeper queries via ScrapeCreators.
6. Extract new queries and knowledge from deeper scrape results.
7. Store new entries in D1.

**Stage 3 -- Visual research**

Runs as a follow-up queue message after expand completes.

1. Collect search terms and TikTok hashtags derived from bubbles.
2. Call ScrapeCreators `GET /v1/tiktok/search/keyword` with `sort_by=most-liked`
   for each term.
3. Call ScrapeCreators `GET /v1/tiktok/search/hashtag` for each hashtag.
4. Filter by engagement threshold (configurable, default 10000 likes).
5. Deduplicate by URL.
6. For each high-engagement video, extract cover/thumbnail URL.
7. Store candidates in `visual_reference_candidates` with
   `human_presence_status = 'unreviewed'`.

**Stage 4 -- Human presence verification**

Runs inline during visual research or as a follow-up.

1. For each unreviewed candidate with an image URL:
   a. Call Kimi K2.6 with vision capability, sending the image URL and a
      structured prompt:
      ```
      Analyze this image. Is there a human visible?
      Return JSON: {
        "human_present": true/false,
        "presence_type": "human_full_body" | "human_upper_body" | "human_face" | "human_partial" | "no_human",
        "confidence": 0.0-1.0,
        "aesthetic_tags": ["tag1", "tag2", ...],
        "description": "brief description of style/mood"
      }
      ```
   b. Accept candidates with `human_present: true` and `confidence >= 0.7`.
   c. Store accepted rows in `visual_references` with aesthetic tags.
   d. Reject others with typed reason.
2. Log all AI calls in `ai_model_invocations`.

### ScrapeCreators HTTP client

The Rust Worker calls `https://api.scrapecreators.com` with:
- Header: `x-api-key: <SCRAPECREATORS_API_KEY>` (Cloudflare Secret).
- All responses are JSON.
- Rate limit with configurable delay between calls (`SCRAPE_DELAY_MS`, default
  1000).

Endpoints used:

| Endpoint | Purpose | Credits |
|----------|---------|---------|
| `GET /v1/reddit/search` | Search Reddit by keyword | 1 |
| `GET /v1/reddit/post/comments` | Get post comments | 1 |
| `GET /v1/tiktok/search/keyword` | Search TikTok by keyword | 1 |
| `GET /v1/tiktok/search/hashtag` | Search TikTok by hashtag | 1 |

### Kimi K2.6 via OpenRouter

Model: `moonshotai/kimi-k2.6` on OpenRouter (`https://openrouter.ai/api/v1`).

The Rust Worker uses the OpenAI-compatible chat completions API:

```
POST https://openrouter.ai/api/v1/chat/completions
Authorization: Bearer <OPENROUTER_API_KEY>
Content-Type: application/json

{
  "model": "moonshotai/kimi-k2.6",
  "messages": [...],
  "response_format": { "type": "json_object" },
  "temperature": 0.7
}
```

For vision tasks (human presence detection), include image URLs in message
content using the standard vision format:

```json
{
  "role": "user",
  "content": [
    { "type": "text", "text": "Analyze this image..." },
    { "type": "image_url", "image_url": { "url": "https://..." } }
  ]
}
```

Secret: `OPENROUTER_API_KEY` (Cloudflare Secret).

AI task types added to `AiTask` enum:
- `NicheSeedExtraction` (existing)
- `NicheClusterExpansion` (existing)
- `VisualReferenceSelection` (existing)
- `HumanPresenceDetection` (existing)
- `BlitzTasteInfluence` (new) -- generates weighted visual reference selection
  prompt from accumulated swipe metadata.

## Schema Changes

### New migration: `1002_blitz_niche_research.sql`

```sql
-- Blitz batches track pre-generated image sets per clone
CREATE TABLE IF NOT EXISTS blitz_batches (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  batch_number INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  batch_size INTEGER NOT NULL DEFAULT 5,
  taste_snapshot_json TEXT NOT NULL DEFAULT '{}',
  visual_ref_ids_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  UNIQUE(clone_id, batch_number)
);

-- Blitz swipes record per-card feedback
CREATE TABLE IF NOT EXISTS blitz_swipes (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  batch_id TEXT NOT NULL,
  generation_output_id TEXT NOT NULL,
  visual_reference_id TEXT,
  direction TEXT NOT NULL,
  aesthetic_tags_json TEXT NOT NULL DEFAULT '[]',
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (batch_id) REFERENCES blitz_batches(id) ON DELETE CASCADE,
  FOREIGN KEY (generation_output_id) REFERENCES generation_outputs(id) ON DELETE CASCADE,
  UNIQUE(batch_id, generation_output_id)
);

CREATE INDEX IF NOT EXISTS idx_blitz_batches_clone_status
  ON blitz_batches(clone_id, status, batch_number);
CREATE INDEX IF NOT EXISTS idx_blitz_batches_user_date
  ON blitz_batches(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_blitz_swipes_batch
  ON blitz_swipes(batch_id, created_at);
CREATE INDEX IF NOT EXISTS idx_blitz_swipes_user_clone
  ON blitz_swipes(user_id, clone_id, created_at DESC);
```

### Modifications to existing tables

`inspiration_bubbles`: already has `clone_id` column. The onboarding route
(`save_bubbles`) must require `clone_id` and reject requests without it.
Bubbles are per clone, not per user.

`generation_jobs`: add `blitz_batch_id TEXT` column referencing
`blitz_batches(id)`. This links generation jobs to their batch.

```sql
ALTER TABLE generation_jobs ADD COLUMN blitz_batch_id TEXT
  REFERENCES blitz_batches(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_generation_jobs_blitz_batch
  ON generation_jobs(blitz_batch_id);
```

`accounts`: add generation quota fields:

```sql
ALTER TABLE accounts ADD COLUMN daily_generation_limit INTEGER NOT NULL DEFAULT 10;
ALTER TABLE accounts ADD COLUMN generation_quota_reset_at TEXT;
```

## API Routes

### Blitz routes

```
GET  /api/blitz/current?cloneId=X
```

Returns the current active batch for the clone. If no batch exists and quota
allows, creates the first batch and enqueues generation.

Response:
```json
{
  "batch": {
    "id": "batch_xxx",
    "cloneId": "clone_xxx",
    "batchNumber": 1,
    "status": "ready",
    "cards": [
      {
        "outputId": "out_xxx",
        "mediaId": "media_xxx",
        "mediaUrl": "/api/media/media_xxx",
        "visualReferenceId": "vref_xxx",
        "aestheticTags": ["streetwear", "neon"]
      }
    ]
  },
  "nextBatchStatus": "generating",
  "quotaRemaining": 5,
  "quotaTotal": 10,
  "quotaExhausted": false
}
```

```
POST /api/blitz/swipe
```

Records a swipe on a card. `direction` is `"like"` or `"dislike"`.

Request:
```json
{
  "batchId": "batch_xxx",
  "outputId": "out_xxx",
  "direction": "like"
}
```

On the first swipe of a batch, triggers pre-generation of the next batch if
quota allows and no next batch exists.

On batch completion (all cards swiped), marks the batch `completed` and stores
the taste snapshot for future influence.

Response:
```json
{
  "swipeId": "swipe_xxx",
  "batchProgress": { "swiped": 1, "total": 5 },
  "nextBatchTriggered": true
}
```

### Niche research routes (internal/debug)

```
GET  /api/niche/status?cloneId=X
```

Returns pipeline status for the clone: seed/expand/visual stage completion,
query count, knowledge count, visual reference count, accepted reference count.

### Modified existing routes

`POST /api/onboarding/bubbles`: require `cloneId` in the request body. Reject
if missing. Store bubbles with `clone_id`. Enqueue `SeedFromBubbles` with
`clone_id`.

## Queue Messages

### Niche Research Queue

Existing message extended with new variants:

```rust
pub enum NicheResearchMessage {
    SeedFromBubbles {
        user_id: String,
        clone_id: String,
        bubble_ids: Vec<String>,
        moderation_level: u8,
    },
    ExpandClusters {
        user_id: String,
        clone_id: String,
    },
    ResearchVisuals {
        user_id: String,
        clone_id: String,
        search_terms: Vec<String>,
        hashtags: Vec<String>,
        engagement_threshold: u32,
    },
    VerifyHumanPresence {
        user_id: String,
        clone_id: String,
        candidate_ids: Vec<String>,
    },
}
```

### Generation Queue

New message variant for batch generation:

```rust
pub enum GenerationMessage {
    GenerateBlitzBatch {
        batch_id: String,
        clone_id: String,
        user_id: String,
        visual_reference_ids: Vec<String>,
        taste_snapshot_json: String,
    },
}
```

## Taste Influence System

### Snapshot format

After a batch is completed, the system builds a taste snapshot from all
completed batches for that clone:

```json
{
  "totalSwipes": 10,
  "likes": 6,
  "dislikes": 4,
  "likedTags": {
    "streetwear": 3,
    "neon": 2,
    "urban": 2
  },
  "dislikedTags": {
    "minimal": 2,
    "pastel": 1
  },
  "likedVisualRefIds": ["vref_1", "vref_3", "vref_5"],
  "dislikedVisualRefIds": ["vref_2", "vref_4"]
}
```

### Visual reference selection with taste

When generating a batch WITH taste influence:

1. Load the clone's visual reference pool (accepted `visual_references`).
2. Score each reference by:
   - Base score from `user_inspiration_pool.score`.
   - Boost for references with tags matching `likedTags` (proportional to
     frequency).
   - Penalty for references with tags matching `dislikedTags`.
   - Freshness bonus for unused references.
   - Variety penalty for references with same tags as already-selected refs in
     this batch.
3. Select top `batch_size` references by score.
4. Store the selected IDs in `blitz_batches.visual_ref_ids_json`.

When generating WITHOUT taste (first batch, or pre-generated batch before
swipes are in):

1. Select from the visual reference pool randomly with variety weighting.
2. Prefer references not yet used in prior batches.

### Influence prompt to Kimi K2.6

For taste-influenced batches, send the taste snapshot to Kimi K2.6 with a
selection prompt:

```
Given these visual references and the user's taste history, select the best 5
for the next generation batch. Prioritize references that match liked aesthetic
tags while maintaining variety.

Taste: {taste_snapshot}
Available references: [{id, tags, description}, ...]

Return JSON: { "selectedIds": ["id1", "id2", ...], "reasoning": "..." }
```

This is the `BlitzTasteInfluence` AI task. Log in `ai_model_invocations`.

## Configuration

All tuning knobs as Wrangler env vars with sensible defaults:

| Var | Default | Purpose |
|-----|---------|---------|
| `BLITZ_BATCH_SIZE` | `5` | Cards per Blitz batch |
| `FREE_DAILY_GENERATION_LIMIT` | `10` | Free user daily quota |
| `PRO_DAILY_GENERATION_LIMIT` | `50` | Pro user daily quota |
| `SCRAPE_DELAY_MS` | `1000` | Delay between ScrapeCreators calls |
| `SCRAPE_MAX_POSTS_PER_QUERY` | `10` | Max posts per scrape query |
| `ENGAGEMENT_THRESHOLD` | `10000` | Min likes for visual research |
| `HUMAN_PRESENCE_MIN_CONFIDENCE` | `0.7` | Min confidence for human detection |
| `OPENROUTER_MODEL` | `moonshotai/kimi-k2.6` | Model for niche research |
| `NICHE_RESEARCH_MAX_QUERIES` | `30` | Max queries extracted per seed |
| `NICHE_RESEARCH_MAX_KNOWLEDGE` | `60` | Max knowledge bits per seed |

## File Ownership

New files:

```
workers/product/src/routes/blitz.rs
workers/product/src/services/blitz.rs
workers/product/src/services/niche_research.rs
workers/product/src/services/scrape_creators.rs
workers/product/src/services/openrouter.rs
workers/product/src/services/taste.rs
workers/product/src/domain/quota.rs
config/d1/migrations/1002_blitz_niche_research.sql
```

Modified files:

```
workers/product/src/http/router.rs          -- add blitz routes
workers/product/src/queues/messages.rs      -- add GenerationMessage
workers/product/src/queues/niche_research.rs -- implement pipeline stages
workers/product/src/routes/onboarding.rs    -- require clone_id for bubbles
workers/product/src/domain/entitlements.rs  -- add generation limits
workers/product/src/ai/tasks.rs             -- add BlitzTasteInfluence
workers/product/src/ai/model_router.rs      -- add OpenRouter provider routing
workers/product/wrangler.product.jsonc      -- add new env vars
```

## Testing Strategy

Unit tests (pure Rust, no bindings):

- Quota calculation: daily limit check, reset logic, plan-based limits.
- Taste snapshot building from swipe records.
- Visual reference scoring with taste influence.
- Batch state transitions.
- ScrapeCreators response parsing.
- OpenRouter response parsing.
- Human presence result parsing and acceptance thresholds.

Route tests (with D1/queue mocks where available):

- `GET /api/blitz/current` returns 401 without auth.
- `POST /api/blitz/swipe` validates direction enum.
- `POST /api/blitz/swipe` returns `quotaExhausted` when limit reached.
- `POST /api/onboarding/bubbles` rejects missing `cloneId`.
- `GET /api/niche/status` returns pipeline stage counts.

Queue tests:

- Niche research message variants serialize/deserialize correctly.
- Generation batch message includes taste snapshot.

## Reliability

- Idempotency keys for batch creation: `blitz:{clone_id}:{batch_number}`.
- Idempotency keys for swipes: `swipe:{batch_id}:{output_id}`.
- Batch generation is atomic: if any image fails, retry the batch (not
  individual images).
- ScrapeCreators calls use retry with exponential backoff (max 3 attempts).
- OpenRouter calls use retry with exponential backoff (max 3 attempts).
- If visual reference pool is empty (research not yet done), fall back to
  bubble-derived prompts for generation.
- DLQ for failed niche research messages.

## Privacy

- Visual reference candidates store public source URLs only.
- Swipe data is scoped to user and not exposed to other users.
- ScrapeCreators API key is a Cloudflare Secret, never in D1 or client
  responses.
- OpenRouter API key is a Cloudflare Secret.
- Taste snapshots contain aggregate tag counts, not raw image data.

## Documentation Sources

- ScrapeCreators OpenAPI: `docs/scrape-creators-openapi.yaml`
- Kimi K2.6 on OpenRouter: `https://openrouter.ai/moonshotai/kimi-k2.6`
- Cloudflare Queues: `https://developers.cloudflare.com/queues/`
- Existing niche research prototype: `../social-page/pipeline`
- Existing Rust Worker: `workers/product/src/`
- Existing schema: `config/d1/migrations/1000_rust_product_core.sql`
