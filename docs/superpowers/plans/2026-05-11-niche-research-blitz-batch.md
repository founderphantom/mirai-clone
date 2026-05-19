# Niche Research Blitz Batch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the clone-scoped niche research pipeline, generation queue handler, and Blitz batch API so each ready Soul can receive researched visual-reference batches, generate image-guided outputs, and learn from like/dislike swipes.

**Architecture:** Keep the Rust Product Worker as the owner of Blitz state, clone-scoped research state, generation usage, private media, queues, and provider calls. D1 stores batch lifecycle, visual reference pools, daily generation usage, and configurable Blitz parameters; Cloudflare Queues run niche research and generation; Workers AI Kimi K2.6 performs all analysis; ScrapeCreators provides TikTok/Instagram source content; Higgsfield MCP remains the Soul generation provider. The React client consumes the new Blitz routes from the existing mobile shell and no longer builds the Blitz deck from generic generation history.

**Tech Stack:** Cloudflare Workers, `workers-rs` 0.6, Rust/Wasm, D1, R2, Cloudflare Queues, Workers AI, ScrapeCreators HTTP API, Higgsfield MCP, React/Vite, Vitest, Rust unit tests.

---

## Scope And Execution Rules

This plan implements `docs/superpowers/specs/2026-05-10-niche-research-blitz-batch-design.md`.

Working assumptions:

- Implement directly on the current main-branch workspace. Do not create a worktree.
- There are no production users. Use a destructive D1 migration for the affected Rust-owned product tables instead of compatibility backfills.
- Preserve Better Auth and Polar tables. Do not drop `accounts`, Better Auth tables, `billing_events`, or auth-owned state.
- Keep research platform allowlist to TikTok and Instagram.
- Workers AI Kimi K2.6 (`@cf/moonshotai/kimi-k2.6`) is the only analysis model. Do not add OpenRouter, OpenCode, or alternate app-analysis providers.
- Higgsfield MCP is used only for clone training and Soul image generation.
- Commit after each task when executing the plan.

## File Ownership Map

Sequential foundation:

- Task 1 owns D1 migration and Worker config.
- Task 2 owns Blitz domain logic and pure tests.
- Task 3 owns ScrapeCreators normalization and pure tests.
- Task 4 owns Workers AI prompt/client helpers and pure tests.

Queue and service layer:

- Task 5 owns niche research queue state machine.
- Task 6 owns generation usage and generation queue state machine.
- Task 7 owns Blitz service/database operations.

API and product integration:

- Task 8 owns Blitz HTTP routes and router exports.
- Task 9 owns onboarding and account usage changes.
- Task 10 owns clone-training readiness hook for first Blitz batch creation.
- Task 11 owns React client Blitz integration.
- Task 12 owns scheduled reconciliation and final verification.

## Target File Structure

```text
config/d1/migrations/
  1002_niche_research_blitz.sql

workers/product/src/domain/
  blitz.rs
  mod.rs

workers/product/src/providers/
  scrapecreators.rs
  mod.rs

workers/product/src/ai/
  workers_ai.rs
  model_router.rs
  tasks.rs
  mod.rs

workers/product/src/services/
  blitz.rs
  generation_usage.rs
  mod.rs

workers/product/src/queues/
  generation.rs
  niche_research.rs
  messages.rs
  mod.rs

workers/product/src/routes/
  blitz.rs
  account.rs
  onboarding.rs
  mod.rs

src/client/
  types.ts
  router.tsx
  components/SwipeDeck.tsx
  screens/BlitzScreen.tsx
  screens/MeScreen.tsx
```

---

## Task 1: Destructive Blitz Schema Migration And Runtime Config

**Order:** Run first.

**Can parallelize:** No.

**Files:**
- Create: `config/d1/migrations/1002_niche_research_blitz.sql`
- Modify: `workers/product/wrangler.product.jsonc`

**Acceptance Criteria:**
- D1 has `blitz_batches`, `blitz_swipes`, `generation_daily_usage`, and `blitz_config`.
- Bubbles, queries, knowledge, visual candidates, visual references, and inspiration pool rows are clone-scoped.
- `generation_jobs.blitz_batch_id`, `discovery_items.source_published_at`, freshness columns, and reuse columns exist.
- Worker config exposes ScrapeCreators and Higgsfield generation tool names.

- [ ] **Step 1: Create the migration**

Create `config/d1/migrations/1002_niche_research_blitz.sql`:

```sql
PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS blitz_swipes;
DROP TABLE IF EXISTS generation_outputs;
DROP TABLE IF EXISTS generation_jobs;
DROP TABLE IF EXISTS user_inspiration_pool;
DROP TABLE IF EXISTS visual_references;
DROP TABLE IF EXISTS visual_reference_candidates;
DROP TABLE IF EXISTS niche_knowledge;
DROP TABLE IF EXISTS niche_research_queries;
DROP TABLE IF EXISTS inspiration_bubbles;
DROP TABLE IF EXISTS discovery_items;
DROP TABLE IF EXISTS discovery_sources;
DROP TABLE IF EXISTS blitz_batches;
DROP TABLE IF EXISTS generation_daily_usage;
DROP TABLE IF EXISTS blitz_config;

CREATE TABLE IF NOT EXISTS discovery_sources (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  source TEXT NOT NULL,
  params_json TEXT NOT NULL DEFAULT '{}',
  refreshed_at TEXT,
  expires_at TEXT,
  status TEXT NOT NULL DEFAULT 'stale',
  UNIQUE(provider, source, params_json)
);

CREATE TABLE IF NOT EXISTS discovery_items (
  id TEXT PRIMARY KEY,
  source_id TEXT NOT NULL,
  external_id TEXT NOT NULL,
  platform TEXT NOT NULL,
  media_type TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  author_handle TEXT NOT NULL DEFAULT '',
  thumbnail_url TEXT,
  image_url TEXT,
  source_url TEXT,
  source_published_at TEXT,
  metrics_json TEXT NOT NULL DEFAULT '{}',
  raw_json TEXT NOT NULL DEFAULT '{}',
  discovered_at TEXT NOT NULL,
  expires_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (source_id) REFERENCES discovery_sources(id) ON DELETE CASCADE,
  UNIQUE(platform, external_id)
);

CREATE TABLE IF NOT EXISTS inspiration_bubbles (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  slug TEXT NOT NULL,
  title TEXT NOT NULL,
  vibe_summary TEXT NOT NULL DEFAULT '',
  search_queries_json TEXT NOT NULL DEFAULT '[]',
  selected INTEGER NOT NULL DEFAULT 0,
  weight REAL NOT NULL DEFAULT 1,
  sort_order INTEGER NOT NULL DEFAULT 0,
  source TEXT NOT NULL DEFAULT 'default',
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS niche_research_queries (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  clone_id TEXT,
  bubble_id TEXT,
  query TEXT NOT NULL,
  source TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'new',
  cluster TEXT,
  cluster_relevance_score REAL,
  cluster_relevance_reason TEXT,
  raw_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  used_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE SET NULL,
  FOREIGN KEY (bubble_id) REFERENCES inspiration_bubbles(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS niche_knowledge (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  clone_id TEXT,
  bit TEXT NOT NULL,
  cluster TEXT,
  cluster_relevance_score REAL,
  cluster_relevance_reason TEXT,
  source_platform TEXT,
  source_url TEXT,
  score REAL NOT NULL DEFAULT 1,
  raw_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE SET NULL
);

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
  error_code TEXT,
  error_message TEXT,
  created_at TEXT NOT NULL,
  ready_at TEXT,
  served_at TEXT,
  completed_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  UNIQUE(clone_id, batch_number)
);

CREATE TABLE IF NOT EXISTS generation_jobs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  blitz_batch_id TEXT,
  input_visual_reference_id TEXT,
  input_media_asset_id TEXT,
  provider TEXT NOT NULL DEFAULT 'higgsfield',
  provider_account_id TEXT,
  provider_job_ids_json TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL DEFAULT 'queued',
  prompt TEXT,
  aspect_ratio TEXT,
  quality TEXT,
  request_json TEXT NOT NULL DEFAULT '{}',
  response_json TEXT NOT NULL DEFAULT '{}',
  error_code TEXT,
  error_message TEXT,
  queued_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (blitz_batch_id) REFERENCES blitz_batches(id) ON DELETE SET NULL,
  FOREIGN KEY (input_visual_reference_id) REFERENCES visual_references(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS generation_outputs (
  id TEXT PRIMARY KEY,
  job_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  media_asset_id TEXT,
  provider_asset_id TEXT,
  raw_url TEXT,
  output_index INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (job_id) REFERENCES generation_jobs(id) ON DELETE CASCADE,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS visual_reference_candidates (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  clone_id TEXT,
  discovery_item_id TEXT,
  source_platform TEXT NOT NULL,
  source_url TEXT,
  source_published_at TEXT,
  freshness_status TEXT NOT NULL DEFAULT 'unreviewed',
  image_url TEXT,
  thumbnail_media_asset_id TEXT,
  human_presence_status TEXT NOT NULL DEFAULT 'unreviewed',
  human_presence_score REAL,
  organic_photo_score REAL,
  freshness_visual_score REAL,
  capture_style TEXT,
  niche_cluster TEXT,
  rejection_reason TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  reviewed_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (discovery_item_id) REFERENCES discovery_items(id) ON DELETE SET NULL,
  FOREIGN KEY (thumbnail_media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS visual_references (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  clone_id TEXT,
  candidate_id TEXT,
  media_asset_id TEXT,
  source_platform TEXT NOT NULL,
  source_url TEXT,
  source_published_at TEXT,
  generation_use_count INTEGER NOT NULL DEFAULT 0,
  last_used_batch_id TEXT,
  last_liked_at TEXT,
  aesthetic_tags_json TEXT NOT NULL DEFAULT '[]',
  niche_cluster TEXT,
  human_presence_type TEXT NOT NULL,
  human_presence_score REAL NOT NULL,
  organic_photo_score REAL NOT NULL DEFAULT 0,
  freshness_visual_score REAL NOT NULL DEFAULT 0,
  moderation_level INTEGER NOT NULL DEFAULT 4,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (candidate_id) REFERENCES visual_reference_candidates(id) ON DELETE SET NULL,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL,
  FOREIGN KEY (last_used_batch_id) REFERENCES blitz_batches(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS user_inspiration_pool (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  bubble_id TEXT,
  visual_reference_id TEXT,
  discovery_item_id TEXT,
  score REAL NOT NULL DEFAULT 1,
  used_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (bubble_id) REFERENCES inspiration_bubbles(id) ON DELETE SET NULL,
  FOREIGN KEY (visual_reference_id) REFERENCES visual_references(id) ON DELETE CASCADE,
  FOREIGN KEY (discovery_item_id) REFERENCES discovery_items(id) ON DELETE CASCADE,
  UNIQUE(clone_id, visual_reference_id),
  UNIQUE(clone_id, discovery_item_id)
);

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
  FOREIGN KEY (generation_output_id) REFERENCES generation_outputs(id) ON DELETE SET NULL,
  FOREIGN KEY (visual_reference_id) REFERENCES visual_references(id) ON DELETE SET NULL,
  UNIQUE(batch_id, swipe_index),
  UNIQUE(batch_id, generation_output_id)
);

CREATE TABLE IF NOT EXISTS generation_daily_usage (
  user_id TEXT NOT NULL,
  usage_date TEXT NOT NULL,
  images_generated INTEGER NOT NULL DEFAULT 0,
  images_limit INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (user_id, usage_date)
);

CREATE TABLE IF NOT EXISTS blitz_config (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

INSERT INTO blitz_config (key, value, updated_at) VALUES
  ('batch_size', '5', '2026-05-11T00:00:00.000Z'),
  ('free_daily_limit', '10', '2026-05-11T00:00:00.000Z'),
  ('pro_daily_limit', '50', '2026-05-11T00:00:00.000Z'),
  ('influence_window', '5', '2026-05-11T00:00:00.000Z'),
  ('min_visual_refs', '5', '2026-05-11T00:00:00.000Z'),
  ('platform_engagement_thresholds_json', '{"tiktok":{"likes":10000},"instagram":{"likes":5000}}', '2026-05-11T00:00:00.000Z'),
  ('freshness_window_years', '5', '2026-05-11T00:00:00.000Z'),
  ('allow_unknown_source_date', 'true', '2026-05-11T00:00:00.000Z'),
  ('recent_search_window', 'last-year', '2026-05-11T00:00:00.000Z'),
  ('cluster_relevance_threshold', '0.72', '2026-05-11T00:00:00.000Z'),
  ('expand_clusters_per_run', '4', '2026-05-11T00:00:00.000Z'),
  ('max_reference_generation_uses', '4', '2026-05-11T00:00:00.000Z'),
  ('scrape_delay_ms', '1000', '2026-05-11T00:00:00.000Z');

CREATE INDEX IF NOT EXISTS idx_inspiration_bubbles_user_clone ON inspiration_bubbles(user_id, clone_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_blitz_batches_clone_status ON blitz_batches(clone_id, status, batch_number DESC);
CREATE INDEX IF NOT EXISTS idx_blitz_batches_user_date ON blitz_batches(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_blitz_swipes_batch ON blitz_swipes(batch_id, swipe_index);
CREATE INDEX IF NOT EXISTS idx_blitz_swipes_clone ON blitz_swipes(clone_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_generation_daily_usage_date ON generation_daily_usage(user_id, usage_date DESC);
CREATE INDEX IF NOT EXISTS idx_generation_jobs_batch ON generation_jobs(blitz_batch_id) WHERE blitz_batch_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_generation_jobs_visual_ref ON generation_jobs(input_visual_reference_id, status) WHERE input_visual_reference_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_generation_outputs_job ON generation_outputs(job_id, output_index);
CREATE INDEX IF NOT EXISTS idx_discovery_items_source ON discovery_items(source_id, discovered_at DESC);
CREATE INDEX IF NOT EXISTS idx_discovery_items_platform_published ON discovery_items(platform, source_published_at DESC);
CREATE INDEX IF NOT EXISTS idx_visual_references_clone ON visual_references(clone_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_visual_references_clone_published ON visual_references(clone_id, source_published_at DESC) WHERE source_published_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_visual_references_clone_reuse ON visual_references(clone_id, generation_use_count, last_liked_at);
CREATE INDEX IF NOT EXISTS idx_visual_ref_candidates_clone ON visual_reference_candidates(clone_id, human_presence_status);
CREATE INDEX IF NOT EXISTS idx_niche_research_queries_status ON niche_research_queries(status, created_at);
CREATE INDEX IF NOT EXISTS idx_niche_research_queries_clone ON niche_research_queries(clone_id, status);
CREATE INDEX IF NOT EXISTS idx_niche_knowledge_clone ON niche_knowledge(clone_id, cluster);
CREATE INDEX IF NOT EXISTS idx_user_inspiration_pool_clone_unused ON user_inspiration_pool(clone_id, used_at, score DESC);

PRAGMA foreign_keys = ON;
```

- [ ] **Step 2: Add runtime vars**

Modify `workers/product/wrangler.product.jsonc` `vars` to include these keys:

```jsonc
{
  "SCRAPECREATORS_BASE_URL": "https://api.scrapecreators.com",
  "HIGGSFIELD_MCP_GENERATION_TOOL": "text2image_soul_v2",
  "BLITZ_BATCH_STALE_MINUTES": "45"
}
```

Keep existing vars such as `APP_NAME`, `MODERATION_LEVEL`, `SCRAPECREATORS_CACHE_TTL_SECONDS`, and `DISCOVERY_DEFAULT_REGION`.

- [ ] **Step 3: Verify migration applies locally**

Run:

```bash
npm run db:migrate:local
```

Expected: PASS. If D1 reports an already-applied local migration conflict, reset only the local D1 database state and rerun because this repository has no users for this slice.

- [ ] **Step 4: Commit**

```bash
git add config/d1/migrations/1002_niche_research_blitz.sql workers/product/wrangler.product.jsonc
git commit -m "feat: add blitz research schema"
```

---

## Task 2: Blitz Domain Logic, Config, Freshness, Quota, And Influence

**Order:** Run after Task 1.

**Can parallelize:** Yes, with Tasks 3 and 4.

**Files:**
- Create: `workers/product/src/domain/blitz.rs`
- Modify: `workers/product/src/domain/mod.rs`
- Modify: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- Pure functions cover blocked synthetic terms, freshness status, human-presence acceptance, daily limit policy, influence accumulation, visual-reference scoring, variety constraints, and reuse caps.
- The unit tests listed below fail before implementation and pass after implementation.

- [ ] **Step 1: Write failing domain tests**

Append these imports to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::domain::blitz::{
    accumulate_influence, can_accept_human_presence, classify_freshness, daily_generation_limit,
    filter_synthetic_terms, select_visual_references, FreshnessDecision, HumanPresenceReview,
    Influence, SwipeMetadata, VisualReferenceForSelection,
};
```

Append these tests:

```rust
#[test]
fn synthetic_generation_terms_are_rejected_case_insensitively() {
    assert!(filter_synthetic_terms("clean girl outfit inspo").is_ok());
    assert_eq!(
        filter_synthetic_terms("AI generated avatar inspo").unwrap_err(),
        "synthetic_generation_term"
    );
    assert_eq!(
        filter_synthetic_terms("Midjourney fashion render").unwrap_err(),
        "synthetic_generation_term"
    );
}

#[test]
fn source_freshness_uses_rolling_five_year_cutoff() {
    assert_eq!(
        classify_freshness(Some("2024-02-01T00:00:00.000Z"), true, "2026-05-11T00:00:00.000Z", 5),
        FreshnessDecision::Recent
    );
    assert_eq!(
        classify_freshness(Some("2020-05-10T00:00:00.000Z"), true, "2026-05-11T00:00:00.000Z", 5),
        FreshnessDecision::TooOld
    );
    assert_eq!(
        classify_freshness(None, true, "2026-05-11T00:00:00.000Z", 5),
        FreshnessDecision::UnknownAllowed
    );
    assert_eq!(
        classify_freshness(None, false, "2026-05-11T00:00:00.000Z", 5),
        FreshnessDecision::UnknownRejected
    );
}

#[test]
fn human_presence_accepts_single_organic_recent_images_only() {
    let accepted = HumanPresenceReview {
        has_human: true,
        human_count: 1,
        human_type: "full_body".to_string(),
        confidence: 0.82,
        organic_photo_score: 0.8,
        freshness_visual_score: 0.78,
        capture_style: "phone".to_string(),
        aesthetic_tags: vec!["street".to_string()],
        rejection_reason: None,
    };
    assert!(can_accept_human_presence(&accepted).is_ok());

    let mut multiple = accepted.clone();
    multiple.human_count = 2;
    assert_eq!(
        can_accept_human_presence(&multiple).unwrap_err(),
        "multiple_humans"
    );

    let mut studio = accepted.clone();
    studio.capture_style = "professional_studio".to_string();
    assert_eq!(
        can_accept_human_presence(&studio).unwrap_err(),
        "too_professional"
    );
}

#[test]
fn daily_generation_limits_follow_plan() {
    assert_eq!(daily_generation_limit("free", 10, 50), 10);
    assert_eq!(daily_generation_limit("paid", 10, 50), 50);
    assert_eq!(daily_generation_limit("studio", 10, 50), 50);
    assert_eq!(daily_generation_limit("unknown", 10, 50), 10);
}

#[test]
fn influence_accumulates_likes_and_dislikes_from_metadata() {
    let influence = accumulate_influence(&[
        SwipeMetadata {
            action: "like".to_string(),
            aesthetic_tags: vec!["minimalist".to_string(), "street".to_string()],
            niche_cluster: Some("outfit-inspo".to_string()),
            source_platform: "tiktok".to_string(),
            visual_reference_id: Some("vref_1".to_string()),
        },
        SwipeMetadata {
            action: "dislike".to_string(),
            aesthetic_tags: vec!["neon".to_string()],
            niche_cluster: Some("formal-wear".to_string()),
            source_platform: "instagram".to_string(),
            visual_reference_id: Some("vref_2".to_string()),
        },
    ]);

    assert_eq!(influence.liked_tags["minimalist"], 1);
    assert_eq!(influence.liked_clusters["outfit-inspo"], 1);
    assert_eq!(influence.disliked_tags["neon"], 1);
    assert_eq!(influence.disliked_clusters["formal-wear"], 1);
    assert_eq!(influence.liked_platforms["tiktok"], 1);
    assert_eq!(influence.liked_visual_reference_ids["vref_1"], 1);
}

#[test]
fn selection_respects_influence_variety_and_reuse_cap() {
    let refs = vec![
        VisualReferenceForSelection {
            id: "liked_repeat".to_string(),
            source_platform: "tiktok".to_string(),
            source_published_at: Some("2025-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("outfit-inspo".to_string()),
            aesthetic_tags: vec!["minimalist".to_string()],
            human_presence_score: 0.8,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 1,
            last_liked_at: Some("2026-05-10T00:00:00.000Z".to_string()),
        },
        VisualReferenceForSelection {
            id: "unliked_used".to_string(),
            source_platform: "instagram".to_string(),
            source_published_at: Some("2025-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("outfit-inspo".to_string()),
            aesthetic_tags: vec!["minimalist".to_string()],
            human_presence_score: 0.95,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 1,
            last_liked_at: None,
        },
        VisualReferenceForSelection {
            id: "fresh_unused".to_string(),
            source_platform: "instagram".to_string(),
            source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("mirror-fit".to_string()),
            aesthetic_tags: vec!["street".to_string()],
            human_presence_score: 0.7,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 0,
            last_liked_at: None,
        },
        VisualReferenceForSelection {
            id: "capped".to_string(),
            source_platform: "tiktok".to_string(),
            source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            niche_cluster: Some("mirror-fit".to_string()),
            aesthetic_tags: vec!["street".to_string()],
            human_presence_score: 0.9,
            organic_photo_score: 0.8,
            freshness_visual_score: 0.8,
            generation_use_count: 4,
            last_liked_at: Some("2026-05-10T00:00:00.000Z".to_string()),
        },
    ];
    let mut influence = Influence::default();
    influence.liked_tags.insert("minimalist".to_string(), 3);
    influence.liked_visual_reference_ids.insert("liked_repeat".to_string(), 1);

    let selected = select_visual_references(&refs, &influence, 2, 4, "2026-05-11T00:00:00.000Z");
    let ids = selected.into_iter().map(|item| item.id).collect::<Vec<_>>();

    assert_eq!(ids, vec!["liked_repeat".to_string(), "fresh_unused".to_string()]);
    assert!(!ids.contains(&"unliked_used".to_string()));
    assert!(!ids.contains(&"capped".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm run product:test
```

Expected: FAIL with unresolved import `mirai_product_worker::domain::blitz`.

- [ ] **Step 3: Implement `domain::blitz`**

Create `workers/product/src/domain/blitz.rs` with these public types and functions:

```rust
use std::collections::HashMap;
use time::{Duration, OffsetDateTime};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FreshnessDecision {
    Recent,
    TooOld,
    UnknownAllowed,
    UnknownRejected,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HumanPresenceReview {
    pub has_human: bool,
    pub human_count: u8,
    pub human_type: String,
    pub confidence: f64,
    pub organic_photo_score: f64,
    pub freshness_visual_score: f64,
    pub capture_style: String,
    pub aesthetic_tags: Vec<String>,
    pub rejection_reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Influence {
    pub liked_tags: HashMap<String, u32>,
    pub disliked_tags: HashMap<String, u32>,
    pub liked_clusters: HashMap<String, u32>,
    pub disliked_clusters: HashMap<String, u32>,
    pub liked_platforms: HashMap<String, u32>,
    pub liked_visual_reference_ids: HashMap<String, u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwipeMetadata {
    pub action: String,
    pub aesthetic_tags: Vec<String>,
    pub niche_cluster: Option<String>,
    pub source_platform: String,
    pub visual_reference_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisualReferenceForSelection {
    pub id: String,
    pub source_platform: String,
    pub source_published_at: Option<String>,
    pub niche_cluster: Option<String>,
    pub aesthetic_tags: Vec<String>,
    pub human_presence_score: f64,
    pub organic_photo_score: f64,
    pub freshness_visual_score: f64,
    pub generation_use_count: u32,
    pub last_liked_at: Option<String>,
}

const SYNTHETIC_TERMS: &[&str] = &[
    "ai",
    "generated",
    "synthetic",
    "render",
    "cgi",
    "avatar",
    "midjourney",
    "stable diffusion",
    "dall-e",
    "dalle",
];

pub fn filter_synthetic_terms(value: &str) -> Result<(), &'static str> {
    let normalized = value.to_ascii_lowercase();
    let words = normalized
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    for term in SYNTHETIC_TERMS {
        if term.contains(' ') || term.contains('-') {
            if normalized.contains(term) {
                return Err("synthetic_generation_term");
            }
            continue;
        }
        if words.iter().any(|word| *word == *term) {
            return Err("synthetic_generation_term");
        }
    }

    Ok(())
}

pub fn classify_freshness(
    source_published_at: Option<&str>,
    allow_unknown_source_date: bool,
    now_iso: &str,
    freshness_window_years: i64,
) -> FreshnessDecision {
    let Some(source_published_at) = source_published_at else {
        return if allow_unknown_source_date {
            FreshnessDecision::UnknownAllowed
        } else {
            FreshnessDecision::UnknownRejected
        };
    };
    let Ok(now) = OffsetDateTime::parse(now_iso, &time::format_description::well_known::Rfc3339)
    else {
        return FreshnessDecision::TooOld;
    };
    let Ok(source_time) = OffsetDateTime::parse(
        source_published_at,
        &time::format_description::well_known::Rfc3339,
    ) else {
        return if allow_unknown_source_date {
            FreshnessDecision::UnknownAllowed
        } else {
            FreshnessDecision::UnknownRejected
        };
    };

    let cutoff = now - Duration::days(365 * freshness_window_years);
    if source_time >= cutoff {
        FreshnessDecision::Recent
    } else {
        FreshnessDecision::TooOld
    }
}

pub fn can_accept_human_presence(review: &HumanPresenceReview) -> Result<(), &'static str> {
    if !review.has_human || review.human_count == 0 {
        return Err("no_human");
    }
    if review.human_count != 1 {
        return Err("multiple_humans");
    }
    if review.confidence < 0.7 {
        return Err("low_confidence");
    }
    if review.organic_photo_score < 0.65 {
        return Err("too_professional");
    }
    if review.freshness_visual_score < 0.55 {
        return Err("stale_visual_trend");
    }
    if matches!(
        review.capture_style.as_str(),
        "professional_studio" | "stock_campaign" | "render_like"
    ) {
        return Err(match review.capture_style.as_str() {
            "render_like" => "synthetic_or_render_like",
            "stock_campaign" => "stock_or_campaign",
            _ => "too_professional",
        });
    }
    if let Some(reason) = review.rejection_reason.as_deref() {
        if !reason.trim().is_empty() {
            return Err("rejected_by_review");
        }
    }
    Ok(())
}

pub fn daily_generation_limit(plan: &str, free_daily_limit: u32, pro_daily_limit: u32) -> u32 {
    match plan {
        "paid" | "pro" | "studio" => pro_daily_limit,
        _ => free_daily_limit,
    }
}

pub fn accumulate_influence(swipes: &[SwipeMetadata]) -> Influence {
    let mut influence = Influence::default();
    for swipe in swipes {
        let liked = swipe.action == "like";
        let tag_map = if liked {
            &mut influence.liked_tags
        } else {
            &mut influence.disliked_tags
        };
        for tag in &swipe.aesthetic_tags {
            *tag_map.entry(tag.clone()).or_insert(0) += 1;
        }
        if let Some(cluster) = swipe.niche_cluster.as_ref() {
            let cluster_map = if liked {
                &mut influence.liked_clusters
            } else {
                &mut influence.disliked_clusters
            };
            *cluster_map.entry(cluster.clone()).or_insert(0) += 1;
        }
        if liked {
            *influence
                .liked_platforms
                .entry(swipe.source_platform.clone())
                .or_insert(0) += 1;
            if let Some(visual_reference_id) = swipe.visual_reference_id.as_ref() {
                *influence
                    .liked_visual_reference_ids
                    .entry(visual_reference_id.clone())
                    .or_insert(0) += 1;
            }
        }
    }
    influence
}

pub fn select_visual_references(
    references: &[VisualReferenceForSelection],
    influence: &Influence,
    batch_size: usize,
    max_reference_generation_uses: u32,
    now_iso: &str,
) -> Vec<VisualReferenceForSelection> {
    let mut scored = references
        .iter()
        .filter(|reference| reference.generation_use_count < max_reference_generation_uses)
        .filter(|reference| reference.generation_use_count == 0 || reference.last_liked_at.is_some())
        .map(|reference| {
            let tag_boost = reference
                .aesthetic_tags
                .iter()
                .map(|tag| influence.liked_tags.get(tag).copied().unwrap_or(0) as f64)
                .sum::<f64>();
            let tag_penalty = reference
                .aesthetic_tags
                .iter()
                .map(|tag| influence.disliked_tags.get(tag).copied().unwrap_or(0) as f64)
                .sum::<f64>();
            let cluster_boost = reference
                .niche_cluster
                .as_ref()
                .and_then(|cluster| influence.liked_clusters.get(cluster).copied())
                .unwrap_or(0) as f64;
            let cluster_penalty = reference
                .niche_cluster
                .as_ref()
                .and_then(|cluster| influence.disliked_clusters.get(cluster).copied())
                .unwrap_or(0) as f64;
            let source_recency = match classify_freshness(
                reference.source_published_at.as_deref(),
                true,
                now_iso,
                5,
            ) {
                FreshnessDecision::Recent => 1.0,
                FreshnessDecision::UnknownAllowed => 0.5,
                _ => 0.0,
            };
            let reuse_boost = if influence
                .liked_visual_reference_ids
                .contains_key(reference.id.as_str())
            {
                0.5
            } else {
                0.0
            };
            let final_score = reference.human_presence_score
                + (tag_boost * 0.3)
                - (tag_penalty * 0.2)
                + (cluster_boost * 0.2)
                - (cluster_penalty * 0.2)
                + (source_recency * 0.3)
                + reuse_boost
                - (0.4 * reference.generation_use_count as f64);
            (final_score, reference.clone())
        })
        .collect::<Vec<_>>();

    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.id.cmp(&right.1.id))
    });

    let mut selected = Vec::new();
    let mut cluster_counts: HashMap<String, usize> = HashMap::new();
    let mut platform_counts: HashMap<String, usize> = HashMap::new();
    for (_, reference) in scored {
        let cluster_key = reference
            .niche_cluster
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        if cluster_counts.get(&cluster_key).copied().unwrap_or(0) >= 2 {
            continue;
        }
        if platform_counts
            .get(&reference.source_platform)
            .copied()
            .unwrap_or(0)
            >= 3
        {
            continue;
        }
        *cluster_counts.entry(cluster_key).or_insert(0) += 1;
        *platform_counts
            .entry(reference.source_platform.clone())
            .or_insert(0) += 1;
        selected.push(reference);
        if selected.len() == batch_size {
            break;
        }
    }
    selected
}
```

Modify `workers/product/src/domain/mod.rs`:

```rust
pub mod blitz;
pub mod entitlements;
pub mod idempotency;
pub mod media_validation;
pub mod status;
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
npm run product:test
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add workers/product/src/domain/blitz.rs workers/product/src/domain/mod.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add blitz domain policies"
```

---

## Task 3: ScrapeCreators Provider And Response Normalization

**Order:** Run after Task 1.

**Can parallelize:** Yes, with Tasks 2 and 4.

**Files:**
- Create: `workers/product/src/providers/scrapecreators.rs`
- Modify: `workers/product/src/providers/mod.rs`
- Modify: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- Only TikTok and Instagram requests can be constructed.
- TikTok keyword, TikTok hashtag, and Instagram reels search URLs match the spec.
- Normalizers extract external id, caption/title text, image/thumbnail URL, source URL, like count, and `source_published_at`.
- Known unsupported platforms are rejected before an HTTP request is made.

- [ ] **Step 1: Write failing provider tests**

Append this import to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::providers::scrapecreators::{
    build_scrape_request, normalize_instagram_reels_search, normalize_tiktok_keyword_search,
    ScrapePlatform,
};
```

Append these tests:

```rust
#[test]
fn scrape_request_builder_allows_only_tiktok_and_instagram() {
    let tiktok = build_scrape_request(
        "https://api.scrapecreators.com",
        ScrapePlatform::TikTokKeyword,
        "streetwear fit",
        "US",
    )
    .unwrap();
    assert_eq!(
        tiktok,
        "https://api.scrapecreators.com/v1/tiktok/search/keyword?query=streetwear%20fit&sort_by=date-posted&date_posted=last-6-months&trim=true&region=US"
    );

    let instagram = build_scrape_request(
        "https://api.scrapecreators.com",
        ScrapePlatform::InstagramReels,
        "clean girl morning",
        "US",
    )
    .unwrap();
    assert_eq!(
        instagram,
        "https://api.scrapecreators.com/v2/instagram/reels/search?query=clean%20girl%20morning&date_posted=last-year"
    );
}

#[test]
fn tiktok_keyword_normalizer_extracts_recent_image_candidates() {
    let raw = serde_json::json!({
        "search_item_list": [{
            "aweme_info": {
                "aweme_id": "725",
                "desc": "city mirror fit",
                "create_time": 1767225600,
                "create_time_utc": "2026-01-01T00:00:00.000Z",
                "share_url": "https://www.tiktok.com/@creator/video/725",
                "statistics": { "digg_count": 23456 },
                "author": { "unique_id": "creator" },
                "video": {
                    "cover": { "url_list": ["https://cdn.example/cover.jpg"] }
                }
            }
        }]
    });

    let items = normalize_tiktok_keyword_search(&raw);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].external_id, "725");
    assert_eq!(items[0].platform, "tiktok");
    assert_eq!(items[0].title, "city mirror fit");
    assert_eq!(items[0].like_count, Some(23456));
    assert_eq!(
        items[0].source_published_at.as_deref(),
        Some("2026-01-01T00:00:00.000Z")
    );
    assert_eq!(items[0].image_url.as_deref(), Some("https://cdn.example/cover.jpg"));
}

#[test]
fn instagram_reels_normalizer_extracts_reel_candidates() {
    let raw = serde_json::json!({
        "reels": [{
            "shortcode": "ABC123",
            "caption": { "text": "neutral outfit morning" },
            "thumbnail_url": "https://cdn.example/ig.jpg",
            "url": "https://www.instagram.com/reel/ABC123/",
            "like_count": 6000,
            "owner": { "username": "igcreator" },
            "taken_at": "2026-02-03T04:05:06.000Z"
        }]
    });

    let items = normalize_instagram_reels_search(&raw);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].external_id, "ABC123");
    assert_eq!(items[0].platform, "instagram");
    assert_eq!(items[0].author_handle, "igcreator");
    assert_eq!(items[0].like_count, Some(6000));
    assert_eq!(
        items[0].source_published_at.as_deref(),
        Some("2026-02-03T04:05:06.000Z")
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm run product:test
```

Expected: FAIL with unresolved import `mirai_product_worker::providers::scrapecreators`.

- [ ] **Step 3: Implement provider module**

Create `workers/product/src/providers/scrapecreators.rs`:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::JsValue;
use worker::{Fetch, Headers, Method, Request, RequestInit, Result as WorkerResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrapePlatform {
    TikTokKeyword,
    TikTokHashtag,
    InstagramReels,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedDiscoveryItem {
    pub external_id: String,
    pub platform: String,
    pub media_type: String,
    pub title: String,
    pub author_handle: String,
    pub thumbnail_url: Option<String>,
    pub image_url: Option<String>,
    pub source_url: Option<String>,
    pub source_published_at: Option<String>,
    pub like_count: Option<u64>,
    pub raw_json: Value,
}

pub fn build_scrape_request(
    base_url: &str,
    platform: ScrapePlatform,
    query: &str,
    region: &str,
) -> Result<String, &'static str> {
    let base = base_url.trim_end_matches('/');
    let encoded_query = url_encode(query);
    let encoded_region = url_encode(region);
    match platform {
        ScrapePlatform::TikTokKeyword => Ok(format!(
            "{base}/v1/tiktok/search/keyword?query={encoded_query}&sort_by=date-posted&date_posted=last-6-months&trim=true&region={encoded_region}"
        )),
        ScrapePlatform::TikTokHashtag => Ok(format!(
            "{base}/v1/tiktok/search/hashtag?hashtag={encoded_query}&trim=true&region={encoded_region}"
        )),
        ScrapePlatform::InstagramReels => Ok(format!(
            "{base}/v2/instagram/reels/search?query={encoded_query}&date_posted=last-year"
        )),
    }
}

pub async fn fetch_scrapecreators_json(
    url: &str,
    api_key: &str,
) -> WorkerResult<Value> {
    let headers = Headers::new();
    headers.set("x-api-key", api_key)?;
    headers.set("accept", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    let request = Request::new_with_init(url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    let text = response.text().await.unwrap_or_default();
    if status >= 400 {
        return Err(worker::Error::RustError(format!(
            "scrapecreators_http_status:{status}:{text}"
        )));
    }
    serde_json::from_str(&text).map_err(|error| worker::Error::RustError(error.to_string()))
}

pub fn normalize_tiktok_keyword_search(raw: &Value) -> Vec<NormalizedDiscoveryItem> {
    raw.get("search_item_list")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| normalize_tiktok_aweme(item.get("aweme_info").unwrap_or(item)))
        .collect()
}

pub fn normalize_tiktok_hashtag_search(raw: &Value) -> Vec<NormalizedDiscoveryItem> {
    raw.get("aweme_list")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(normalize_tiktok_aweme)
        .collect()
}

pub fn normalize_instagram_reels_search(raw: &Value) -> Vec<NormalizedDiscoveryItem> {
    raw.get("reels")
        .or_else(|| raw.get("items"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let external_id = text_at(item, &["shortcode"])
                .or_else(|| text_at(item, &["code"]))
                .or_else(|| text_at(item, &["id"]))?;
            let title = text_at(item, &["caption", "text"])
                .or_else(|| text_at(item, &["caption"]))
                .unwrap_or_default();
            let author_handle = text_at(item, &["owner", "username"]).unwrap_or_default();
            let image_url = text_at(item, &["thumbnail_url"])
                .or_else(|| text_at(item, &["display_url"]))
                .or_else(|| text_at(item, &["image_versions2", "candidates", "0", "url"]));
            let source_url = text_at(item, &["url"]).or_else(|| {
                Some(format!("https://www.instagram.com/reel/{external_id}/"))
            });
            Some(NormalizedDiscoveryItem {
                external_id,
                platform: "instagram".to_string(),
                media_type: "reel".to_string(),
                title,
                author_handle,
                thumbnail_url: image_url.clone(),
                image_url,
                source_url,
                source_published_at: text_at(item, &["taken_at"])
                    .or_else(|| text_at(item, &["taken_at_date"])),
                like_count: number_at(item, &["like_count"]),
                raw_json: item.clone(),
            })
        })
        .collect()
}

fn normalize_tiktok_aweme(item: &Value) -> Option<NormalizedDiscoveryItem> {
    let external_id = text_at(item, &["aweme_id"]).or_else(|| text_at(item, &["id"]))?;
    let title = text_at(item, &["desc"]).unwrap_or_default();
    let image_url = text_at(item, &["video", "cover", "url_list", "0"])
        .or_else(|| text_at(item, &["video", "origin_cover", "url_list", "0"]))
        .or_else(|| text_at(item, &["image_post_info", "images", "0", "display_image", "url_list", "0"]));
    Some(NormalizedDiscoveryItem {
        external_id,
        platform: "tiktok".to_string(),
        media_type: "video".to_string(),
        title,
        author_handle: text_at(item, &["author", "unique_id"]).unwrap_or_default(),
        thumbnail_url: image_url.clone(),
        image_url,
        source_url: text_at(item, &["share_url"]),
        source_published_at: text_at(item, &["create_time_utc"])
            .or_else(|| number_at(item, &["create_time"]).map(unix_seconds_to_iso)),
        like_count: number_at(item, &["statistics", "digg_count"]),
        raw_json: item.clone(),
    })
}

fn text_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for part in path {
        if let Ok(index) = part.parse::<usize>() {
            current = current.as_array()?.get(index)?;
        } else {
            current = current.get(*part)?;
        }
    }
    current.as_str().map(str::to_string)
}

fn number_at(value: &Value, path: &[&str]) -> Option<u64> {
    let mut current = value;
    for part in path {
        current = current.get(*part)?;
    }
    current.as_u64()
}

fn unix_seconds_to_iso(value: u64) -> String {
    let millis = value as f64 * 1000.0;
    js_sys::Date::new(&JsValue::from_f64(millis))
        .to_iso_string()
        .into()
}

fn url_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['%', '2', '0'],
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}
```

Modify `workers/product/src/providers/mod.rs`:

```rust
pub mod higgsfield_auth;
pub mod higgsfield_mcp;
pub mod scrapecreators;
```

- [ ] **Step 4: Add `web-sys` support if needed**

If `cargo test` reports missing `Fetch` or request types, keep the provider implementation using the same imports as `workers/product/src/providers/higgsfield_mcp.rs`. Do not introduce a new HTTP client dependency.

- [ ] **Step 5: Run tests**

Run:

```bash
npm run product:test
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/providers/scrapecreators.rs workers/product/src/providers/mod.rs workers/product/tests/domain_tests.rs
git commit -m "feat: normalize scrapecreators research results"
```

---

## Task 4: Workers AI Kimi Client And Prompt Builders

**Order:** Run after Task 1.

**Can parallelize:** Yes, with Tasks 2 and 3.

**Files:**
- Create: `workers/product/src/ai/workers_ai.rs`
- Modify: `workers/product/src/ai/tasks.rs`
- Modify: `workers/product/src/ai/model_router.rs`
- Modify: `workers/product/src/ai/mod.rs`
- Modify: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- `choose_model` selects Workers AI Kimi K2.6 for every app-analysis task.
- Prompt builders include the synthetic-term and organic-photo constraints from the spec.
- Worker AI calls use the pinned `workers-rs` `Ai::run(model, input)` API.

- [ ] **Step 1: Write failing AI tests**

Replace the existing `deepseek_can_handle_text_tasks` test in `workers/product/tests/domain_tests.rs` with:

```rust
#[test]
fn kimi_is_the_only_analysis_model_for_text_tasks() {
    let models = vec![
        ModelConfig {
            provider: "deepseek".to_string(),
            model: "deepseek-v4-pro".to_string(),
            supports_vision: false,
            supports_structured_json: true,
        },
        ModelConfig {
            provider: "workers_ai".to_string(),
            model: "@cf/moonshotai/kimi-k2.6".to_string(),
            supports_vision: true,
            supports_structured_json: true,
        },
    ];

    let selected = choose_model(AiTask::NicheSeedExtraction, &models).unwrap();

    assert_eq!(selected.provider, "workers_ai");
    assert_eq!(selected.model, "@cf/moonshotai/kimi-k2.6");
}
```

Append this import:

```rust
use mirai_product_worker::ai::workers_ai::{
    human_presence_prompt, knowledge_extraction_prompt, seed_extraction_prompt,
};
```

Append this test:

```rust
#[test]
fn workers_ai_prompts_include_research_guardrails() {
    let seed = seed_extraction_prompt("Clean Girl Street", &["minimal outfit".to_string()]);
    assert!(seed.contains("TikTok and Instagram"));
    assert!(seed.contains("Do not include synthetic/generation topics"));

    let knowledge = knowledge_extraction_prompt("Clean Girl Street");
    assert!(knowledge.contains("Do not extract from known-stale source items"));

    let human = human_presence_prompt();
    assert!(human.contains("exactly one human person"));
    assert!(human.contains("organic creator content"));
    assert!(human.contains("render_like"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm run product:test
```

Expected: FAIL because the router still allows DeepSeek and `workers_ai` module does not exist.

- [ ] **Step 3: Update AI task routing**

Modify `workers/product/src/ai/model_router.rs` so `choose_model` ignores non-Workers AI providers:

```rust
pub fn choose_model(task: AiTask, models: &[ModelConfig]) -> Option<ModelConfig> {
    models
        .iter()
        .find(|model| {
            model.provider == "workers_ai"
                && model.model == "@cf/moonshotai/kimi-k2.6"
                && model.supports_structured_json
                && (!task.requires_vision() || model.supports_vision)
        })
        .cloned()
}
```

Modify `workers/product/src/ai/tasks.rs` to add the missing knowledge task:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiTask {
    PhotoQualityReview,
    HumanPresenceDetection,
    BubbleGeneration,
    NicheSeedExtraction,
    NicheKnowledgeExtraction,
    NicheClusterExpansion,
    VisualReferenceSelection,
    Moderation,
}
```

Keep `requires_vision` unchanged except for formatting.

- [ ] **Step 4: Add Workers AI helper module**

Create `workers/product/src/ai/workers_ai.rs`:

```rust
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use worker::{Ai, Result as WorkerResult};

pub const KIMI_K2_6_MODEL: &str = "@cf/moonshotai/kimi-k2.6";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkersAiInput<'a> {
    messages: Vec<WorkersAiMessage<'a>>,
    response_format: WorkersAiResponseFormat,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkersAiMessage<'a> {
    role: &'a str,
    content: WorkersAiContent<'a>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum WorkersAiContent<'a> {
    Text(&'a str),
    Parts(Vec<WorkersAiContentPart<'a>>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkersAiContentPart<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    text: Option<&'a str>,
    image: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkersAiResponseFormat {
    JsonObject,
}

#[derive(Debug, Deserialize)]
struct WorkersAiTextResponse {
    response: Option<String>,
    result: Option<Value>,
}

pub async fn run_text_json<T: DeserializeOwned>(
    ai: &Ai,
    prompt: &str,
) -> WorkerResult<T> {
    let response = ai
        .run::<_, WorkersAiTextResponse>(
            KIMI_K2_6_MODEL,
            WorkersAiInput {
                messages: vec![WorkersAiMessage {
                    role: "user",
                    content: WorkersAiContent::Text(prompt),
                }],
                response_format: WorkersAiResponseFormat::JsonObject,
            },
        )
        .await?;
    decode_structured_response(response)
}

pub async fn run_vision_json<T: DeserializeOwned>(
    ai: &Ai,
    prompt: &str,
    image_url: &str,
) -> WorkerResult<T> {
    let response = ai
        .run::<_, WorkersAiTextResponse>(
            KIMI_K2_6_MODEL,
            WorkersAiInput {
                messages: vec![WorkersAiMessage {
                    role: "user",
                    content: WorkersAiContent::Parts(vec![
                        WorkersAiContentPart {
                            kind: "text",
                            text: Some(prompt),
                            image: None,
                        },
                        WorkersAiContentPart {
                            kind: "image_url",
                            text: None,
                            image: Some(image_url),
                        },
                    ]),
                }],
                response_format: WorkersAiResponseFormat::JsonObject,
            },
        )
        .await?;
    decode_structured_response(response)
}

fn decode_structured_response<T: DeserializeOwned>(
    response: WorkersAiTextResponse,
) -> WorkerResult<T> {
    if let Some(result) = response.result {
        return serde_json::from_value(result)
            .map_err(|error| worker::Error::RustError(error.to_string()));
    }
    let Some(text) = response.response else {
        return Err(worker::Error::RustError(
            "workers_ai_empty_response".to_string(),
        ));
    };
    serde_json::from_str(&text).map_err(|error| worker::Error::RustError(error.to_string()))
}

pub fn seed_extraction_prompt(active_niche: &str, bubble_summaries: &[String]) -> String {
    format!(
        "Given these aesthetic directions for a creator clone:\n{}\n\nGenerate 15-25 search queries for finding recent organic creator visual content on TikTok and Instagram.\nFocus on: single-person outfit/lifestyle inspiration, creator aesthetics, phone-camera photos, compact-digital-camera photos, mirror shots, casual photoshoot aesthetics, and trending visual styles.\nDo not include synthetic/generation topics or terms.\nAvoid outdated era labels unless the selected bubble explicitly asks for a revival aesthetic; even then, search for current creators posting that look within the last 5 years.\nActive niche: {active_niche}\nReturn JSON: {{ \"queries\": [{{ \"query\": \"...\", \"platforms\": [\"tiktok\", \"instagram\"] }}] }}",
        bubble_summaries.join("\n")
    )
}

pub fn knowledge_extraction_prompt(active_niche: &str) -> String {
    format!(
        "You are analyzing social media content about \"{active_niche}\".\nExtract two things:\n\n1. QUERIES: Recurring questions or subtopics people search for. Format: short, searchable phrases. Extract 15-30 unique queries.\n\n2. KNOWLEDGE BITS: Specific, actionable tips or insights. Deduplicate similar advice. Keep each under 30 words. Include source type. Do not extract from known-stale source items.\n\nReject synthetic/generation terms and content outside the active niche.\nReturn JSON: {{ \"queries\": [{{ \"query\": \"...\", \"source\": \"tiktok|instagram\" }}], \"knowledge\": [{{ \"bit\": \"...\", \"source_platform\": \"...\" }}] }}"
    )
}

pub fn clustering_prompt(
    active_niche: &str,
    focus_keywords: &[String],
    negative_focus_keywords: &[String],
) -> String {
    format!(
        "Active niche: {active_niche}\nFocus keywords: {}\nNegative focus keywords: {}\n\nGroup these knowledge bits and search queries into coherent subtopic clusters that stay inside the active niche. Name each with a short kebab-case label. Score each cluster's relevance to the active niche from 0.0 to 1.0. Penalize broad lifestyle/fashion clusters that do not clearly connect to the focus keywords, and penalize clusters that match negative focus keywords.\n\nReturn JSON: {{ \"clusters\": [{{ \"name\": \"kebab-case-name\", \"bit_ids\": [1, 2, 3], \"query_ids\": [1, 2], \"description\": \"what this cluster covers\", \"cluster_relevance_score\": 0.0, \"cluster_relevance_reason\": \"why this belongs or does not belong\" }}] }}",
        focus_keywords.join(", "),
        negative_focus_keywords.join(", ")
    )
}

pub fn human_presence_prompt() -> &'static str {
    "Analyze this image. Does it contain exactly one human person, does it look like organic creator content captured on a phone, compact digital camera, or casual photoshoot setup, and does it avoid stale visual trends?\n\nReturn JSON: { \"has_human\": true, \"human_count\": 1, \"human_type\": \"full_body\", \"confidence\": 0.0, \"organic_photo_score\": 0.0, \"freshness_visual_score\": 0.0, \"capture_style\": \"phone\", \"aesthetic_tags\": [\"minimalist\"], \"rejection_reason\": null }\nAllowed capture_style values: phone, compact_digital_camera, casual_photoshoot, professional_studio, stock_campaign, render_like, unknown."
}
```

Modify `workers/product/src/ai/mod.rs`:

```rust
pub mod model_router;
pub mod tasks;
pub mod workers_ai;
```

- [ ] **Step 5: Run tests**

Run:

```bash
npm run product:test
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/ai/workers_ai.rs workers/product/src/ai/tasks.rs workers/product/src/ai/model_router.rs workers/product/src/ai/mod.rs workers/product/tests/domain_tests.rs
git commit -m "feat: route research analysis through workers ai kimi"
```

---

## Task 5: Niche Research Queue Pipeline

**Order:** Run after Tasks 2, 3, and 4.

**Can parallelize:** No.

**Files:**
- Modify: `workers/product/src/queues/niche_research.rs`
- Modify: `workers/product/src/queues/messages.rs`
- Modify: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- Queue message enum supports `seed_from_bubbles` and `refresh_pool`.
- `seed_from_bubbles` carries `platforms`.
- Handler loads selected bubbles for the clone only.
- Handler inserts clone-scoped search queries, discovery sources/items, knowledge, candidates, visual references, and inspiration pool rows.
- Handler rejects synthetic source text, disallowed platforms, known-stale sources, non-organic images, and no/multiple-human images.
- If the pool reaches `min_visual_refs` and the Soul is ready, the first Blitz batch is created and a generation message is enqueued.
- If the Soul is not ready, `clone_profiles.provider_config_json` records `nicheResearchStatus = pool_ready_awaiting_soul`.
- If accepted refs are below minimum, `provider_config_json` records `nicheResearchStatus = insufficient_refs`.

- [ ] **Step 1: Write queue message tests**

Append this import to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::queues::niche_research::NicheResearchMessage;
```

Append this test:

```rust
#[test]
fn niche_research_messages_serialize_platform_allowlist() {
    let seed = NicheResearchMessage::SeedFromBubbles {
        user_id: "user_1".to_string(),
        clone_id: "clone_1".to_string(),
        bubble_ids: vec!["bubble_1".to_string()],
        moderation_level: 4,
        platforms: vec!["tiktok".to_string(), "instagram".to_string()],
    };
    assert_eq!(
        serde_json::to_value(seed).unwrap(),
        serde_json::json!({
            "type": "seed_from_bubbles",
            "userId": "user_1",
            "cloneId": "clone_1",
            "bubbleIds": ["bubble_1"],
            "moderationLevel": 4,
            "platforms": ["tiktok", "instagram"]
        })
    );

    let refresh = NicheResearchMessage::RefreshPool {
        user_id: "user_1".to_string(),
        clone_id: "clone_1".to_string(),
        reason: "pool_depleted".to_string(),
    };
    assert_eq!(
        serde_json::to_value(refresh).unwrap()["type"],
        serde_json::json!("refresh_pool")
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm run product:test
```

Expected: FAIL because `platforms` and `RefreshPool` are missing.

- [ ] **Step 3: Extend niche research messages**

Modify the enum in `workers/product/src/queues/niche_research.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum NicheResearchMessage {
    SeedFromBubbles {
        user_id: String,
        clone_id: String,
        bubble_ids: Vec<String>,
        moderation_level: u8,
        platforms: Vec<String>,
    },
    RefreshPool {
        user_id: String,
        clone_id: String,
        reason: String,
    },
}
```

Update the queue handler match arms to call:

```rust
handle_seed_from_bubbles(&db, &env, user_id, clone_id, bubble_ids, moderation_level, platforms).await
```

and:

```rust
handle_refresh_pool(&db, &env, user_id, clone_id, reason).await
```

Both functions return `WorkerResult<()>`.

- [ ] **Step 4: Implement pipeline row structs**

Add these row structs in `workers/product/src/queues/niche_research.rs`:

```rust
#[derive(Debug, Deserialize)]
struct BubbleRow {
    id: String,
    title: String,
    vibe_summary: String,
    search_queries_json: String,
}

#[derive(Debug, Deserialize)]
struct CloneResearchRow {
    user_id: String,
    soul_status: String,
    provider_soul_id: Option<String>,
    provider_config_json: String,
}

#[derive(Debug, Deserialize)]
struct ConfigRow {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u32,
}
```

- [ ] **Step 5: Implement config and status helpers**

Add these helpers:

```rust
async fn load_config_map(db: &D1Database) -> WorkerResult<std::collections::HashMap<String, String>> {
    let rows = db::all::<ConfigRow>(
        db,
        "SELECT key, value FROM blitz_config",
        vec![],
    )
    .await?;
    Ok(rows.into_iter().map(|row| (row.key, row.value)).collect())
}

fn config_u32(config: &std::collections::HashMap<String, String>, key: &str, default: u32) -> u32 {
    config.get(key).and_then(|value| value.parse().ok()).unwrap_or(default)
}

fn config_bool(config: &std::collections::HashMap<String, String>, key: &str, default: bool) -> bool {
    config
        .get(key)
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

async fn set_clone_research_status(
    db: &D1Database,
    clone_id: &str,
    status: &str,
    detail: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE clone_profiles
        SET provider_config_json = json_set(
              COALESCE(NULLIF(provider_config_json, ''), '{}'),
              '$.nicheResearchStatus',
              ?,
              '$.nicheResearchDetail',
              ?,
              '$.nicheResearchUpdatedAt',
              ?
            ),
            updated_at = ?
        WHERE id = ?
        "#,
        vec![json!(status), json!(detail), json!(now), json!(now), json!(clone_id)],
    )
    .await
}
```

- [ ] **Step 6: Implement Stage 1 seed extraction**

In `handle_seed_from_bubbles`, load and validate clone/bubbles:

```rust
let db = env.d1("DB")?;
let ai = env.ai("AI")?;
let clone = db::first::<CloneResearchRow>(
    db,
    r#"
    SELECT user_id, soul_status, provider_soul_id, provider_config_json
    FROM clone_profiles
    WHERE id = ?
      AND user_id = ?
      AND deleted_at IS NULL
    "#,
    vec![json!(clone_id), json!(user_id)],
)
.await?;
let Some(clone) = clone else {
    return Ok(());
};

let selected_json = serde_json::to_string(&bubble_ids)?;
let bubbles = db::all::<BubbleRow>(
    db,
    r#"
    SELECT id, title, vibe_summary, search_queries_json
    FROM inspiration_bubbles
    WHERE user_id = ?
      AND clone_id = ?
      AND EXISTS (
        SELECT 1 FROM json_each(?) WHERE json_each.value = inspiration_bubbles.id
      )
    ORDER BY sort_order ASC, created_at ASC
    "#,
    vec![json!(user_id), json!(clone_id), json!(selected_json)],
)
.await?;
if bubbles.len() < 5 {
    set_clone_research_status(db, &clone_id, "insufficient_bubbles", "At least 5 selected bubbles are required.").await?;
    return Ok(());
}

let active_niche = bubbles
    .iter()
    .map(|bubble| bubble.title.as_str())
    .collect::<Vec<_>>()
    .join(" + ");
let summaries = bubbles
    .iter()
    .map(|bubble| format!("{}: {}", bubble.title, bubble.vibe_summary))
    .collect::<Vec<_>>();
```

Call `run_text_json` using `seed_extraction_prompt`, then insert queries with `clone_id`. The accepted query insertion SQL must be:

```rust
db::exec(
    db,
    r#"
    INSERT OR IGNORE INTO niche_research_queries (
      id, user_id, clone_id, bubble_id, query, source, status, raw_json, created_at
    )
    VALUES (?, ?, ?, NULL, ?, ?, 'new', ?, ?)
    "#,
    vec![
        json!(prefixed_id("nq")),
        json!(user_id),
        json!(clone_id),
        json!(query.query),
        json!(platform),
        json!(serde_json::to_string(&query)?),
        json!(now_iso_string()),
    ],
)
.await?;
```

Before inserting, run `filter_synthetic_terms(&query.query)` and skip the query when it returns an error.

- [ ] **Step 7: Implement Stage 2 scraping**

For each accepted query/platform pair:

- Build only `ScrapePlatform::TikTokKeyword`, `ScrapePlatform::TikTokHashtag`, or `ScrapePlatform::InstagramReels`.
- Load `SCRAPECREATORS_BASE_URL`, `DISCOVERY_DEFAULT_REGION`, and `SCRAPECREATORS_API_KEY`.
- Fetch JSON through `fetch_scrapecreators_json`.
- Normalize results with the provider functions from Task 3.
- Skip items where `platform` is not `tiktok` or `instagram`.
- Skip items where `filter_synthetic_terms(&item.title)` returns an error.
- Skip known stale items where `classify_freshness(...) == FreshnessDecision::TooOld`.
- Insert one `discovery_sources` row per request and `discovery_items` rows using `INSERT OR IGNORE`.

Use this `discovery_items` SQL:

```rust
INSERT OR IGNORE INTO discovery_items (
  id,
  source_id,
  external_id,
  platform,
  media_type,
  title,
  author_handle,
  thumbnail_url,
  image_url,
  source_url,
  source_published_at,
  metrics_json,
  raw_json,
  discovered_at,
  created_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
```

The `metrics_json` value must be:

```rust
json!({ "likes": item.like_count }).to_string()
```

Apply the configured scrape delay after each HTTP call:

```rust
worker::Delay::from(std::time::Duration::from_millis(scrape_delay_ms as u64)).await;
```

- [ ] **Step 8: Implement Stages 3 and 4 knowledge extraction and clustering**

Batch current clone discovery titles/captions with:

```sql
SELECT id, platform, title, source_url, source_published_at
FROM discovery_items
WHERE platform IN ('tiktok', 'instagram')
ORDER BY
  CASE WHEN source_published_at IS NULL THEN 1 ELSE 0 END,
  source_published_at DESC,
  discovered_at DESC
LIMIT 120
```

For each item:

- Skip synthetic text with `filter_synthetic_terms`.
- Skip known stale dates.
- Pass remaining text to `knowledge_extraction_prompt(&active_niche)`.
- Insert extracted knowledge and deeper queries with `clone_id`.

After knowledge insertion:

- Call `clustering_prompt`.
- Update `niche_knowledge.cluster`, `cluster_relevance_score`, `cluster_relevance_reason`.
- Update `niche_research_queries.cluster`, `cluster_relevance_score`, `cluster_relevance_reason`.
- Expand at most `expand_clusters_per_run` clusters with score >= `cluster_relevance_threshold`.
- Run one additional Stage 2 scrape pass for generated deeper queries.

- [ ] **Step 9: Implement Stage 5 visual reference selection**

Use this discovery query:

```sql
SELECT id, platform, title, image_url, thumbnail_url, source_url, source_published_at, metrics_json, raw_json
FROM discovery_items
WHERE platform IN ('tiktok', 'instagram')
  AND COALESCE(image_url, thumbnail_url) IS NOT NULL
ORDER BY
  CASE WHEN source_published_at IS NULL THEN 1 ELSE 0 END,
  source_published_at DESC,
  discovered_at DESC
LIMIT 200
```

For each candidate:

- Require likes >= configured threshold when likes are present.
- Insert `visual_reference_candidates` with `clone_id`, `source_published_at`, and `freshness_status`.
- Run `run_vision_json::<HumanPresenceReview>(&ai, human_presence_prompt(), image_url)`.
- Reject when `can_accept_human_presence` returns an error.
- Reject when source freshness is `TooOld` or `UnknownRejected`.
- Insert accepted `visual_references` with `clone_id`, `aesthetic_tags_json`, `niche_cluster`, `human_presence_type`, `human_presence_score`, `organic_photo_score`, and `freshness_visual_score`.
- Insert `user_inspiration_pool` with `clone_id`.

- [ ] **Step 10: Create first Blitz batch when pool is ready**

After Stage 5:

```rust
let active_refs = db::first::<CountRow>(
    db,
    r#"
    SELECT COUNT(*) AS count
    FROM visual_references
    WHERE clone_id = ?
      AND status = 'active'
    "#,
    vec![json!(clone_id)],
)
.await?
.map(|row| row.count)
.unwrap_or(0);
```

If `active_refs < min_visual_refs`, set clone status to `insufficient_refs`.

If `active_refs >= min_visual_refs` and `clone.soul_status == "ready"` and `clone.provider_soul_id.is_some()`, call `crate::services::blitz::create_next_batch(...)` and send a `GenerationMessage::GenerateBlitzBatch`.

If `active_refs >= min_visual_refs` but the Soul is not ready, set clone status to `pool_ready_awaiting_soul`.

- [ ] **Step 11: Run tests and check**

Run:

```bash
npm run product:test
npm run product:check
```

Expected: both PASS.

- [ ] **Step 12: Commit**

```bash
git add workers/product/src/queues/niche_research.rs workers/product/src/queues/messages.rs workers/product/tests/domain_tests.rs
git commit -m "feat: implement clone scoped niche research queue"
```

---

## Task 6: Generation Usage Service And Generation Queue Handler

**Order:** Run after Tasks 2 and 5.

**Can parallelize:** No.

**Files:**
- Create: `workers/product/src/services/generation_usage.rs`
- Create: `workers/product/src/queues/generation.rs`
- Modify: `workers/product/src/queues/messages.rs`
- Modify: `workers/product/src/queues/mod.rs`
- Modify: `workers/product/src/services/mod.rs`
- Modify: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- Daily usage reserves before provider submission, settles on success, and refunds on terminal failure.
- The generation queue submits one Higgsfield job per selected visual reference.
- Poll messages can be delayed with `MessageBuilder::new(...).delay_seconds(...)`.
- Successful outputs insert `generation_outputs`, increment `blitz_batches.generation_count`, increment `visual_references.generation_use_count`, and mark the batch `ready` when enough outputs exist.
- Partial success can mark the batch `ready` with fewer images when all jobs are terminal.

- [ ] **Step 1: Write message serialization tests**

Append this import:

```rust
use mirai_product_worker::queues::messages::GenerationMessage;
```

Append this test:

```rust
#[test]
fn generation_messages_serialize_blitz_fields_as_camel_case() {
    let message = GenerationMessage::GenerateBlitzBatch {
        batch_id: "batch_1".to_string(),
        clone_id: "clone_1".to_string(),
        user_id: "user_1".to_string(),
        idempotency_key: "blitz_gen:batch_1".to_string(),
        visual_reference_ids: vec!["vref_1".to_string()],
        provider_soul_id: "soul_1".to_string(),
    };
    assert_eq!(
        serde_json::to_value(message).unwrap(),
        serde_json::json!({
            "type": "generate_blitz_batch",
            "batchId": "batch_1",
            "cloneId": "clone_1",
            "userId": "user_1",
            "idempotencyKey": "blitz_gen:batch_1",
            "visualReferenceIds": ["vref_1"],
            "providerSoulId": "soul_1"
        })
    );

    let poll = GenerationMessage::PollGeneration {
        job_id: "gen_1".to_string(),
        batch_id: "batch_1".to_string(),
        attempt: 1,
        max_attempts: 30,
    };
    assert_eq!(
        serde_json::to_value(poll).unwrap()["type"],
        serde_json::json!("poll_generation")
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm run product:test
```

Expected: FAIL because `GenerationMessage` does not exist.

- [ ] **Step 3: Add generation messages**

Modify `workers/product/src/queues/messages.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GenerationMessage {
    GenerateBlitzBatch {
        batch_id: String,
        clone_id: String,
        user_id: String,
        idempotency_key: String,
        visual_reference_ids: Vec<String>,
        provider_soul_id: String,
    },
    PollGeneration {
        job_id: String,
        batch_id: String,
        attempt: u8,
        max_attempts: u8,
    },
}
```

- [ ] **Step 4: Implement generation usage service**

Create `workers/product/src/services/generation_usage.rs`:

```rust
use crate::db;
use crate::domain::blitz::daily_generation_limit;
use serde::{Deserialize, Serialize};
use serde_json::json;
use worker::{D1Database, Result as WorkerResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationUsageSnapshot {
    pub images_today: u32,
    pub daily_limit: u32,
    pub remaining: u32,
    pub limit_resets_at: String,
}

#[derive(Debug, Deserialize)]
struct UsageRow {
    images_generated: u32,
    images_limit: u32,
}

pub async fn usage_snapshot(
    db: &D1Database,
    user_id: &str,
    plan: &str,
    free_daily_limit: u32,
    pro_daily_limit: u32,
) -> WorkerResult<GenerationUsageSnapshot> {
    let usage_date = current_utc_date();
    let limit = daily_generation_limit(plan, free_daily_limit, pro_daily_limit);
    let row = db::first::<UsageRow>(
        db,
        r#"
        SELECT images_generated, images_limit
        FROM generation_daily_usage
        WHERE user_id = ?
          AND usage_date = ?
        "#,
        vec![json!(user_id), json!(usage_date)],
    )
    .await?;
    let images_today = row.as_ref().map(|row| row.images_generated).unwrap_or(0);
    let daily_limit = row.as_ref().map(|row| row.images_limit).unwrap_or(limit);
    Ok(GenerationUsageSnapshot {
        images_today,
        daily_limit,
        remaining: daily_limit.saturating_sub(images_today),
        limit_resets_at: next_midnight_utc_iso(),
    })
}

pub async fn reserve_image(
    db: &D1Database,
    user_id: &str,
    plan: &str,
    free_daily_limit: u32,
    pro_daily_limit: u32,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let usage_date = current_utc_date();
    let limit = daily_generation_limit(plan, free_daily_limit, pro_daily_limit);
    db::exec(
        db,
        r#"
        INSERT INTO generation_daily_usage (
          user_id, usage_date, images_generated, images_limit, created_at, updated_at
        )
        VALUES (?, ?, 0, ?, ?, ?)
        ON CONFLICT(user_id, usage_date) DO UPDATE SET
          images_limit = excluded.images_limit,
          updated_at = excluded.updated_at
        "#,
        vec![json!(user_id), json!(usage_date), json!(limit), json!(now), json!(now)],
    )
    .await?;

    let result = db::run(
        db,
        r#"
        UPDATE generation_daily_usage
        SET images_generated = images_generated + 1,
            updated_at = ?
        WHERE user_id = ?
          AND usage_date = ?
          AND images_generated < images_limit
        "#,
        vec![json!(now), json!(user_id), json!(usage_date)],
    )
    .await?;
    Ok(result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or(0)
        > 0)
}

pub async fn refund_image(db: &D1Database, user_id: &str) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE generation_daily_usage
        SET images_generated = CASE
              WHEN images_generated > 0 THEN images_generated - 1
              ELSE 0
            END,
            updated_at = ?
        WHERE user_id = ?
          AND usage_date = ?
        "#,
        vec![json!(now), json!(user_id), json!(current_utc_date())],
    )
    .await
}

fn current_utc_date() -> String {
    now_iso_string().chars().take(10).collect()
}

fn next_midnight_utc_iso() -> String {
    let today_midnight = format!("{}T00:00:00.000Z", current_utc_date());
    let millis = js_sys::Date::parse(&today_midnight) + 86_400_000.0;
    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(millis))
        .to_iso_string()
        .into()
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}
```

Modify `workers/product/src/services/mod.rs`:

```rust
pub mod accounts;
pub mod blitz;
pub mod clones;
pub mod generation_usage;
pub mod media;
pub mod provider_accounts;
```

- [ ] **Step 5: Implement generation queue module**

Create `workers/product/src/queues/generation.rs` with these functions:

```rust
use crate::db;
use crate::providers::higgsfield_auth::{refresh_access_token, validate_access_token};
use crate::providers::higgsfield_mcp::call_tool;
use crate::queues::messages::GenerationMessage;
use crate::services::generation_usage::{refund_image, reserve_image};
use serde::Deserialize;
use serde_json::{json, Value};
use worker::{D1Database, Env, MessageBatch, MessageBuilder, MessageExt, Result as WorkerResult};

const HIGGSFIELD_REFRESH_SECRET_NAME: &str = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FUFU";
const HIGGSFIELD_GENERATION_TOOL_VAR: &str = "HIGGSFIELD_MCP_GENERATION_TOOL";

#[derive(Debug, Deserialize)]
struct CloneGenerationRow {
    plan: String,
    soul_status: String,
    provider_soul_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VisualReferenceRow {
    id: String,
    media_asset_id: Option<String>,
    source_url: Option<String>,
    source_platform: String,
    aesthetic_tags_json: String,
    niche_cluster: Option<String>,
}

pub async fn handle_batch(batch: MessageBatch<Value>, env: Env) -> WorkerResult<()> {
    let db = env.d1("DB")?;
    for raw_message in batch.raw_iter() {
        let body = match serde_wasm_bindgen::from_value::<GenerationMessage>(raw_message.body()) {
            Ok(body) => body,
            Err(error) => {
                web_sys::console::error_1(
                    &format!("failed to deserialize generation queue message: {error:?}").into(),
                );
                raw_message.ack();
                continue;
            }
        };
        let result = match body {
            GenerationMessage::GenerateBlitzBatch {
                batch_id,
                clone_id,
                user_id,
                idempotency_key,
                visual_reference_ids,
                provider_soul_id,
            } => {
                generate_blitz_batch(
                    &db,
                    &env,
                    &batch_id,
                    &clone_id,
                    &user_id,
                    &idempotency_key,
                    &visual_reference_ids,
                    &provider_soul_id,
                )
                .await
            }
            GenerationMessage::PollGeneration {
                job_id,
                batch_id,
                attempt,
                max_attempts,
            } => poll_generation(&db, &env, &job_id, &batch_id, attempt, max_attempts).await,
        };
        match result {
            Ok(()) => raw_message.ack(),
            Err(error) => {
                web_sys::console::error_1(&format!("generation queue failed: {error:?}").into());
                raw_message.retry();
            }
        }
    }
    Ok(())
}
```

In `generate_blitz_batch`:

- Verify clone belongs to user and `soul_status = 'ready'`.
- Load `free_daily_limit` and `pro_daily_limit` from `blitz_config`.
- For each visual reference id:
  - Call `reserve_image`.
  - Insert `generation_jobs` with `blitz_batch_id`, `input_visual_reference_id`, `status = 'queued'`, and request JSON containing `idempotencyKey`, `providerSoulId`, and `visualReferenceId`.
  - Refresh and validate Higgsfield auth token.
  - Call `call_tool` with `HIGGSFIELD_MCP_GENERATION_TOOL` and arguments:

```rust
json!({
  "jobId": job_id,
  "batchId": batch_id,
  "cloneId": clone_id,
  "userId": user_id,
  "idempotencyKey": format!("{idempotency_key}:{visual_reference_id}"),
  "providerSoulId": provider_soul_id,
  "inputImageUrl": materialized_reference_url,
  "prompt": ""
})
```

- If the provider response contains a final image URL at `$.result.image_url`, `$.result.url`, `$.image_url`, or `$.url`, call `complete_generation_job`.
- If no final URL is present, persist `provider_job_ids_json` and send `PollGeneration` with:

```rust
env.queue("GENERATION_QUEUE")?
    .send(MessageBuilder::new(GenerationMessage::PollGeneration {
        job_id: job_id.clone(),
        batch_id: batch_id.to_string(),
        attempt: 1,
        max_attempts: 30,
    })
    .delay_seconds(10)
    .build())
    .await?;
```

In `complete_generation_job`:

- Download generated image from the provider URL.
- Store bytes in R2 under `media_storage_key(user_id, clone_id, media_id, content_type)`.
- Insert a `media_assets` row with `kind = 'generation'` and `source = 'higgsfield'`.
- Insert `generation_outputs`.
- Mark `generation_jobs.status = 'completed'`.
- Increment `visual_references.generation_use_count` and set `last_used_batch_id`.
- Increment `blitz_batches.generation_count`.
- Call `mark_batch_ready_if_complete`.

In `fail_generation_job`:

- Mark `generation_jobs.status = 'failed'`.
- Call `refund_image`.
- If every job for the batch is terminal and `generation_count > 0`, mark the batch `ready`.
- If every job is terminal and `generation_count = 0`, mark the batch `failed`.

- [ ] **Step 6: Register generation queue**

Modify `workers/product/src/queues/mod.rs`:

```rust
pub mod clone_training;
pub mod generation;
pub mod messages;
pub mod niche_research;

const GENERATION_QUEUE_NAME: &str = "mirai-generation";
```

Add the match arm:

```rust
GENERATION_QUEUE_NAME => generation::handle_batch(batch, env).await,
```

- [ ] **Step 7: Run tests and check**

Run:

```bash
npm run product:test
npm run product:check
```

Expected: both PASS.

- [ ] **Step 8: Commit**

```bash
git add workers/product/src/services/generation_usage.rs workers/product/src/queues/generation.rs workers/product/src/queues/messages.rs workers/product/src/queues/mod.rs workers/product/src/services/mod.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add blitz generation queue"
```

---

## Task 7: Blitz Service For Batch Creation, Current Batch, History, And Swipes

**Order:** Run after Tasks 2 and 6.

**Can parallelize:** No.

**Files:**
- Create: `workers/product/src/services/blitz.rs`
- Modify: `workers/product/src/services/mod.rs`
- Modify: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- `create_next_batch` computes batch number, influence, selected visual references, and enqueues generation.
- `current_batch` returns the oldest ready/active batch for a clone and marks ready batches active on first serve.
- `record_swipe` records metadata snapshot, rejects duplicates, updates like/dislike counts, updates `last_liked_at`, completes the batch on last swipe, and triggers next batch on the first swipe.
- Batch N+1 influence uses completed batches before the current batch only.

- [ ] **Step 1: Write service contract tests**

Append these imports:

```rust
use mirai_product_worker::services::blitz::{
    next_batch_should_trigger, swipe_action_to_db_value, trigger_influence_cutoff_batch_number,
};
```

Append these tests:

```rust
#[test]
fn blitz_swipe_actions_accept_like_and_dislike_only() {
    assert_eq!(swipe_action_to_db_value("like").unwrap(), "like");
    assert_eq!(swipe_action_to_db_value("dislike").unwrap(), "dislike");
    assert_eq!(swipe_action_to_db_value("pass").unwrap_err(), "invalid_swipe_action");
}

#[test]
fn first_swipe_triggers_prefetch_once() {
    assert!(next_batch_should_trigger(0));
    assert!(!next_batch_should_trigger(1));
    assert!(!next_batch_should_trigger(4));
}

#[test]
fn influence_for_next_batch_skips_current_batch_feedback() {
    assert_eq!(trigger_influence_cutoff_batch_number(1), 0);
    assert_eq!(trigger_influence_cutoff_batch_number(2), 0);
    assert_eq!(trigger_influence_cutoff_batch_number(3), 1);
    assert_eq!(trigger_influence_cutoff_batch_number(4), 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm run product:test
```

Expected: FAIL because `services::blitz` does not exist.

- [ ] **Step 3: Implement service public structs**

Create `workers/product/src/services/blitz.rs` with:

```rust
use crate::db;
use crate::domain::blitz::{
    accumulate_influence, select_visual_references, Influence, SwipeMetadata,
    VisualReferenceForSelection,
};
use crate::queues::messages::GenerationMessage;
use crate::services::generation_usage::GenerationUsageSnapshot;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use worker::{D1Database, Env, Result as WorkerResult};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzCurrentResponse {
    pub batch: Option<BlitzBatchResponse>,
    pub status: Option<String>,
    pub progress: Option<BlitzProgressResponse>,
    pub usage: GenerationUsageSnapshot,
    pub next_batch_status: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzBatchResponse {
    pub id: String,
    pub batch_number: u32,
    pub status: String,
    pub images: Vec<BlitzImageResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzImageResponse {
    pub output_id: String,
    pub media_url: String,
    pub visual_reference_id: Option<String>,
    pub swipe_index: u32,
    pub swiped: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzProgressResponse {
    pub phase: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwipeResponse {
    pub swipe_index: u32,
    pub batch_progress: String,
    pub batch_complete: bool,
    pub next_batch_triggered: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzHistoryResponse {
    pub batches: Vec<BlitzHistoryBatch>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzHistoryBatch {
    pub id: String,
    pub batch_number: u32,
    pub like_count: u32,
    pub dislike_count: u32,
    pub completed_at: Option<String>,
}
```

- [ ] **Step 4: Implement pure helpers**

Add:

```rust
pub fn swipe_action_to_db_value(action: &str) -> Result<&'static str, &'static str> {
    match action {
        "like" => Ok("like"),
        "dislike" => Ok("dislike"),
        _ => Err("invalid_swipe_action"),
    }
}

pub fn next_batch_should_trigger(existing_swipes_in_batch: u32) -> bool {
    existing_swipes_in_batch == 0
}

pub fn trigger_influence_cutoff_batch_number(current_batch_number: u32) -> u32 {
    current_batch_number.saturating_sub(2)
}

fn prefixed_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}
```

- [ ] **Step 5: Implement `create_next_batch`**

The function signature:

```rust
pub async fn create_next_batch(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    provider_soul_id: &str,
) -> WorkerResult<Option<String>>
```

Implementation requirements:

- Return the existing `pending`/`generating`/`ready` batch id when one already exists for the clone.
- Load `batch_size` and `max_reference_generation_uses` from `blitz_config`.
- Compute `next_batch_number = COALESCE(MAX(batch_number), 0) + 1`.
- Compute influence from swipes where `blitz_batches.status = 'completed'` and `batch_number <= trigger_influence_cutoff_batch_number(next_batch_number)`.
- Load `visual_references` for the clone into `VisualReferenceForSelection`.
- Select references using `select_visual_references`.
- If fewer than one reference is selected, send `NicheResearchMessage::RefreshPool` and return `Ok(None)`.
- Insert a `blitz_batches` row with status `generating`.
- Send `GenerationMessage::GenerateBlitzBatch` with selected ids.
- Return `Ok(Some(batch_id))`.

- [ ] **Step 6: Implement `current_batch`**

The function signature:

```rust
pub async fn current_batch(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    usage: GenerationUsageSnapshot,
) -> WorkerResult<BlitzCurrentResponse>
```

Implementation requirements:

- Verify the clone belongs to the user.
- Select the oldest `ready` or `active` batch for the clone.
- If status is `ready`, update it to `active` and set `served_at`.
- Load outputs for that batch ordered by `output_index`.
- Return `media_url` as `/api/media/{media_asset_id}`.
- Set `swiped = true` when a matching `blitz_swipes.generation_output_id` exists.
- If no batch exists, read `clone_profiles.provider_config_json` for `nicheResearchStatus` and return:

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

- [ ] **Step 7: Implement `record_swipe`**

The function signature:

```rust
pub async fn record_swipe(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    batch_id: &str,
    output_id: &str,
    action: &str,
) -> WorkerResult<SwipeResponse>
```

Implementation requirements:

- Validate action with `swipe_action_to_db_value`.
- Load the batch by id and user id.
- Load the output, generation job, and linked visual reference.
- Compute `swipe_index` as the output's zero-based position in the batch ordered by `generation_outputs.output_index, generation_outputs.created_at`.
- Insert `blitz_swipes` with `output_metadata_json`:

```rust
json!({
  "aestheticTags": tags,
  "nicheCluster": niche_cluster,
  "sourcePlatform": source_platform,
  "visualReferenceId": visual_reference_id
}).to_string()
```

- Use `INSERT OR IGNORE`; if changed rows is zero, return `duplicate_swipe`.
- Increment `like_count` or `dislike_count` on `blitz_batches`.
- On `like`, set `visual_references.last_liked_at = now`.
- If this is the first swipe in the batch, call `create_next_batch` for prefetch.
- If swipe count reaches `batch_size`, mark the batch `completed`.

- [ ] **Step 8: Implement `history`**

The function signature:

```rust
pub async fn history(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    limit: u32,
) -> WorkerResult<BlitzHistoryResponse>
```

Use SQL:

```sql
SELECT id, batch_number, like_count, dislike_count, completed_at
FROM blitz_batches
WHERE user_id = ?
  AND clone_id = ?
  AND status = 'completed'
ORDER BY batch_number DESC
LIMIT ?
```

- [ ] **Step 9: Run tests and check**

Run:

```bash
npm run product:test
npm run product:check
```

Expected: both PASS.

- [ ] **Step 10: Commit**

```bash
git add workers/product/src/services/blitz.rs workers/product/src/services/mod.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add blitz batch service"
```

---

## Task 8: Blitz API Routes

**Order:** Run after Task 7.

**Can parallelize:** No.

**Files:**
- Create: `workers/product/src/routes/blitz.rs`
- Modify: `workers/product/src/routes/mod.rs`
- Modify: `workers/product/src/http/router.rs`
- Modify: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- `GET /api/blitz/current?clone_id={clone_id}` returns current batch or progress.
- `POST /api/blitz/swipe` records a like/dislike.
- `GET /api/blitz/history?clone_id={clone_id}&limit=10` returns completed batch summaries.
- Missing clone id and invalid body responses use existing `ApiError`.

- [ ] **Step 1: Write route helper tests**

Append this import:

```rust
use mirai_product_worker::routes::blitz::{parse_history_limit, read_required_query_param};
```

Append these tests:

```rust
#[test]
fn blitz_route_query_helpers_validate_required_values() {
    let url = worker::Url::parse("https://mirai.test/api/blitz/current?clone_id=clone_1").unwrap();
    assert_eq!(
        read_required_query_param(&url, "clone_id").unwrap(),
        "clone_1".to_string()
    );

    let missing = worker::Url::parse("https://mirai.test/api/blitz/current").unwrap();
    assert_eq!(
        read_required_query_param(&missing, "clone_id").unwrap_err(),
        "missing_clone_id"
    );
}

#[test]
fn blitz_history_limit_is_bounded() {
    assert_eq!(parse_history_limit(None), 10);
    assert_eq!(parse_history_limit(Some("2")), 2);
    assert_eq!(parse_history_limit(Some("500")), 50);
    assert_eq!(parse_history_limit(Some("bad")), 10);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
npm run product:test
```

Expected: FAIL because route module does not exist.

- [ ] **Step 3: Implement route module**

Create `workers/product/src/routes/blitz.rs`:

```rust
use crate::auth_client::verify_session;
use crate::db;
use crate::http::error::ApiError;
use crate::services::blitz;
use crate::services::generation_usage::usage_snapshot;
use serde::Deserialize;
use serde_json::json;
use worker::{Request, Response, Result as WorkerResult, RouteContext, Url};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwipeRequest {
    batch_id: String,
    output_id: String,
    action: String,
}

pub async fn current(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let url = req.url()?;
    let clone_id = match read_required_query_param(&url, "clone_id") {
        Ok(value) => value,
        Err(_) => return ApiError::bad_request("missing_clone_id", "clone_id is required.").to_response(),
    };
    let db = ctx.env.d1("DB")?;
    let usage = usage_snapshot(&db, &auth.user_id, &auth.plan, 10, 50).await?;
    let response = blitz::current_batch(&db, &auth.user_id, &clone_id, usage).await?;
    Response::from_json(&response)
}

pub async fn swipe(mut req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let input = match req.json::<SwipeRequest>().await {
        Ok(input) => input,
        Err(_) => {
            return ApiError::bad_request(
                "invalid_blitz_swipe_request",
                "Expected batchId, outputId, and action.",
            )
            .to_response()
        }
    };
    let db = ctx.env.d1("DB")?;
    match blitz::record_swipe(&db, &ctx.env, &auth.user_id, &input.batch_id, &input.output_id, &input.action).await {
        Ok(response) => Response::from_json(&response),
        Err(error) if error.to_string().contains("duplicate_swipe") => {
            ApiError::bad_request("duplicate_swipe", "This Blitz card was already swiped.").to_response()
        }
        Err(error) if error.to_string().contains("invalid_swipe_action") => {
            ApiError::bad_request("invalid_swipe_action", "Swipe action must be like or dislike.").to_response()
        }
        Err(error) => Err(error),
    }
}

pub async fn history(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let url = req.url()?;
    let clone_id = match read_required_query_param(&url, "clone_id") {
        Ok(value) => value,
        Err(_) => return ApiError::bad_request("missing_clone_id", "clone_id is required.").to_response(),
    };
    let limit = parse_history_limit(
        url.query_pairs()
            .find(|(key, _)| key == "limit")
            .map(|(_, value)| value.to_string())
            .as_deref(),
    );
    let db = ctx.env.d1("DB")?;
    Response::from_json(&blitz::history(&db, &auth.user_id, &clone_id, limit).await?)
}

pub fn read_required_query_param(url: &Url, key: &str) -> Result<String, &'static str> {
    url.query_pairs()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or("missing_clone_id")
}

pub fn parse_history_limit(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(10)
        .clamp(1, 50)
}
```

- [ ] **Step 4: Register routes**

Modify `workers/product/src/routes/mod.rs`:

```rust
pub mod account;
pub mod blitz;
pub mod clones;
pub mod discovery;
pub mod generations;
pub mod media;
pub mod onboarding;
pub mod telemetry;
```

Modify `workers/product/src/http/router.rs` and add before health:

```rust
.get_async("/api/blitz/current", crate::routes::blitz::current)
.post_async("/api/blitz/swipe", crate::routes::blitz::swipe)
.get_async("/api/blitz/history", crate::routes::blitz::history)
```

- [ ] **Step 5: Run tests and check**

Run:

```bash
npm run product:test
npm run product:check
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/routes/blitz.rs workers/product/src/routes/mod.rs workers/product/src/http/router.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add blitz api routes"
```

---

## Task 9: Onboarding Bubbles And Account Usage Integration

**Order:** Run after Tasks 1, 5, and 8.

**Can parallelize:** No.

**Files:**
- Modify: `workers/product/src/routes/onboarding.rs`
- Modify: `workers/product/src/routes/account.rs`
- Modify: `workers/product/tests/domain_tests.rs`

**Acceptance Criteria:**
- `POST /api/onboarding/bubbles` requires exactly 5 selected bubbles for this slice.
- Saved bubbles always have `clone_id`.
- Queue message includes `platforms: ["tiktok", "instagram"]`.
- `GET /api/account/usage` includes `generationUsage`.

- [ ] **Step 1: Update onboarding tests**

Modify the existing `save_bubbles_request_accepts_existing_bubble_ids_contract` test to use 5 ids:

```rust
#[test]
fn save_bubbles_request_accepts_existing_bubble_ids_contract() {
    let request = serde_json::from_value::<SaveBubblesRequest>(json!({
        "cloneId": "clone_1",
        "bubbleIds": ["bubble_1", "bubble_2", "bubble_3", "bubble_4", "bubble_5"],
        "moderationLevel": 7
    }))
    .unwrap();

    assert_eq!(request.clone_id.as_deref(), Some("clone_1"));
    assert_eq!(request.selected_bubble_ids.len(), 5);
    assert_eq!(request.moderation_level, Some(7));
}
```

Append this test to the onboarding module tests:

```rust
#[test]
fn bubble_selection_requires_five_unique_ids_for_research() {
    assert_eq!(
        unique_selected_bubble_ids(vec![
            "bubble_1".to_string(),
            "bubble_2".to_string(),
            "bubble_3".to_string(),
            "bubble_4".to_string(),
            "bubble_5".to_string(),
        ])
        .len(),
        5
    );
}
```

- [ ] **Step 2: Run test to verify current behavior fails product requirement**

Run:

```bash
npm run product:test
```

Expected: PASS may still occur because the route validation is not directly unit-tested. Continue with implementation because the current route accepts 1-5 and the spec requires 5+; this plan uses exactly 5 because the existing UI limits selection to 5.

- [ ] **Step 3: Change save-bubbles validation**

Modify `workers/product/src/routes/onboarding.rs`:

```rust
if requested_bubble_ids.len() != 5 {
    return ApiError::bad_request(
        "invalid_bubble_selection",
        "Choose exactly 5 inspiration bubbles.",
    )
    .to_response();
}
```

Update the queue send:

```rust
ctx.env
    .queue("NICHE_RESEARCH_QUEUE")?
    .send(NicheResearchMessage::SeedFromBubbles {
        user_id: auth.user_id.clone(),
        clone_id: active_clone.id.clone(),
        bubble_ids: selected_bubble_ids,
        moderation_level,
        platforms: vec!["tiktok".to_string(), "instagram".to_string()],
    })
    .await?;
```

Keep `load_matching_bubble_ids` clone-scoped exactly as it is.

- [ ] **Step 4: Add generation usage to account usage**

Modify `workers/product/src/routes/account.rs` response types:

```rust
use crate::services::generation_usage::{usage_snapshot, GenerationUsageSnapshot};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountUsageResponse {
    clones: Vec<UsageBucket>,
    generations: Vec<UsageBucket>,
    media: Vec<UsageBucket>,
    generation_usage: GenerationUsageSnapshot,
}
```

Inside `get_usage`, after loading `media`, add:

```rust
let generation_usage = usage_snapshot(&db, user_id, &auth.plan, 10, 50).await?;
```

Return:

```rust
Response::from_json(&AccountUsageResponse {
    clones,
    generations,
    media,
    generation_usage,
})
```

- [ ] **Step 5: Run tests and check**

Run:

```bash
npm run product:test
npm run product:check
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/routes/onboarding.rs workers/product/src/routes/account.rs workers/product/tests/domain_tests.rs
git commit -m "feat: start clone scoped research from bubbles"
```

---

## Task 10: Soul Readiness Gate For Waiting Research Pools

**Order:** Run after Tasks 5, 6, and 7.

**Can parallelize:** No.

**Files:**
- Modify: `workers/product/src/services/blitz.rs`
- Modify: `workers/product/src/queues/clone_training.rs`

**Acceptance Criteria:**
- Ready Souls with `provider_config_json.nicheResearchStatus = 'pool_ready_awaiting_soul'` can start their first Blitz batch through a reusable service function.
- If this repository has a clone-training ready-status update path at execution time, that path calls the service function immediately after persisting readiness.
- Task 12 scheduled reconciliation also calls the service function so ready Souls are picked up even when readiness is changed outside the queue path.

- [ ] **Step 1: Add waiting-pool scanner to Blitz service**

Add to `workers/product/src/services/blitz.rs`:

```rust
#[derive(Debug, Deserialize)]
struct WaitingReadyPoolRow {
    clone_id: String,
    user_id: String,
    provider_soul_id: String,
}

pub async fn start_waiting_ready_pools(env: &Env) -> WorkerResult<u32> {
    let db = env.d1("DB")?;
    let rows = db::all::<WaitingReadyPoolRow>(
        &db,
        r#"
        SELECT id AS clone_id, user_id, provider_soul_id
        FROM clone_profiles
        WHERE soul_status = 'ready'
          AND provider_soul_id IS NOT NULL
          AND deleted_at IS NULL
          AND json_extract(provider_config_json, '$.nicheResearchStatus') = 'pool_ready_awaiting_soul'
        ORDER BY updated_at ASC
        LIMIT 20
        "#,
        vec![],
    )
    .await?;

    let mut started = 0;
    for row in rows {
        if create_next_batch(
            &db,
            env,
            &row.user_id,
            &row.clone_id,
            &row.provider_soul_id,
        )
        .await?
        .is_some()
        {
            db::exec(
                &db,
                r#"
                UPDATE clone_profiles
                SET provider_config_json = json_set(
                      COALESCE(NULLIF(provider_config_json, ''), '{}'),
                      '$.nicheResearchStatus',
                      'batch_generation_started',
                      '$.nicheResearchDetail',
                      'First Blitz batch queued after Soul readiness.'
                    ),
                    updated_at = ?
                WHERE id = ?
                "#,
                vec![json!(now_iso_string()), json!(row.clone_id)],
            )
            .await?;
            started += 1;
        }
    }

    Ok(started)
}
```

- [ ] **Step 2: Find terminal-ready update path**

Run:

```bash
rg -n "soul_status = 'ready'|completed_at|provider_soul_id" workers/product/src/queues/clone_training.rs
```

Expected: output shows a ready-status write if clone training completion is already implemented. If the only matches are provider submission fields and no ready-status write exists, skip Step 3 and rely on the scheduled call added in Task 12.

- [ ] **Step 3: Call the scanner after ready status is persisted when that path exists**

If Step 2 found a ready-status write, add this call immediately after the database update succeeds:

```rust
crate::services::blitz::start_waiting_ready_pools(env).await?;
```

- [ ] **Step 4: Run checks**

Run:

```bash
npm run product:test
npm run product:check
```

Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add workers/product/src/services/blitz.rs workers/product/src/queues/clone_training.rs
git commit -m "feat: start blitz when soul becomes ready"
```

---

## Task 11: React Blitz Client Integration

**Order:** Run after Task 8.

**Can parallelize:** Yes, after API response shapes from Task 8 are stable.

**Files:**
- Modify: `src/client/types.ts`
- Modify: `src/client/router.tsx`
- Modify: `src/client/components/SwipeDeck.tsx`
- Modify: `src/client/screens/BlitzScreen.tsx`
- Modify: `src/client/screens/MeScreen.tsx`

**Acceptance Criteria:**
- Blitz screen loads `/api/blitz/current?clone_id=...` for the selected clone.
- Like button posts `action: "like"` and pass/dislike button posts `action: "dislike"`.
- Usage remaining and no-ready-batch progress are displayed.
- Account screen reads `generationUsage` without breaking existing usage buckets.

- [ ] **Step 1: Add TypeScript Blitz types**

Modify `src/client/types.ts`:

```ts
export type BlitzImage = {
  outputId: string;
  mediaUrl: string;
  visualReferenceId: string | null;
  swipeIndex: number;
  swiped: boolean;
};

export type BlitzBatch = {
  id: string;
  batchNumber: number;
  status: string;
  images: BlitzImage[];
};

export type GenerationUsage = {
  imagesToday: number;
  dailyLimit: number;
  remaining: number;
  limitResetsAt: string;
};

export type BlitzCurrent = {
  batch: BlitzBatch | null;
  status?: string | null;
  progress?: { phase: string; detail: string } | null;
  usage: GenerationUsage;
  nextBatchStatus?: string | null;
};
```

- [ ] **Step 2: Pass selected clone id into Blitz screen**

Modify `src/client/router.tsx`:

```tsx
{effectiveRoute === "blitz" && (
  <BlitzScreen
    clones={data.clones}
    selectedCloneId={selectedCloneId}
  />
)}
```

- [ ] **Step 3: Update `SwipeDeck` action contract**

Modify `src/client/components/SwipeDeck.tsx`:

```ts
onSwipe?: (card: SwipeCard, verdict: "like" | "dislike") => void;
```

Change the local `swipe` signature:

```ts
function swipe(verdict: "like" | "dislike") {
```

Change the pass button:

```tsx
<button className="pass" title="Dislike" onClick={() => swipe("dislike")}>
```

- [ ] **Step 4: Replace Blitz screen data source**

Replace `src/client/screens/BlitzScreen.tsx` with:

```tsx
import { Loader2, Sparkles } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { SwipeDeck, type SwipeCard } from "../components/SwipeDeck";
import { api } from "../lib/api";
import { track } from "../lib/analytics";
import type { BlitzCurrent, Clone } from "../types";

export function BlitzScreen({
  clones,
  selectedCloneId
}: {
  clones: Clone[];
  selectedCloneId: string;
}) {
  const selectedClone = useMemo(
    () => clones.find((clone) => clone.id === selectedCloneId) || clones[0],
    [clones, selectedCloneId]
  );
  const [state, setState] = useState<BlitzCurrent | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function load() {
    if (!selectedClone?.id) return;
    setBusy(true);
    setError("");
    try {
      const next = await api<BlitzCurrent>(`/api/blitz/current?clone_id=${encodeURIComponent(selectedClone.id)}`);
      setState(next);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not load Blitz.");
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void load();
  }, [selectedClone?.id]);

  const cards: SwipeCard[] = (state?.batch?.images || [])
    .filter((image) => !image.swiped)
    .map((image) => ({
      id: image.outputId,
      title: selectedClone?.display_name || "Mirai Soul",
      subtitle: `Batch ${state?.batch?.batchNumber || 1}`,
      imageUrl: image.mediaUrl
    }));

  async function swipe(card: SwipeCard, verdict: "like" | "dislike") {
    if (!state?.batch) return;
    try {
      await api("/api/blitz/swipe", {
        method: "POST",
        body: JSON.stringify({
          batchId: state.batch.id,
          outputId: card.id,
          action: verdict
        })
      });
      track("blitz_swipe", { verdict, cloneId: selectedClone?.id });
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not save swipe.");
    }
  }

  const readyCount = cards.length;
  const usage = state?.usage;
  const emptyLabel = state?.progress?.detail || "Blitz deck warming up";

  return (
    <div className="screen-stack">
      <section className="app-hero compact">
        <div>
          <span className="app-kicker">Daily Blitz</span>
          <h2>{selectedClone ? `${selectedClone.display_name}'s fresh batch` : "Choose a Soul to Blitz."}</h2>
          <p>{usage ? `${usage.remaining} generations left today.` : "Researching your visual references."}</p>
        </div>
      </section>
      <section className="daily-strip">
        {busy ? <Loader2 className="spin" size={18} /> : <Sparkles size={18} />}
        <span>{readyCount} ready</span>
        {state?.nextBatchStatus && <span>{state.nextBatchStatus}</span>}
      </section>
      {error && <p className="error">{error}</p>}
      <SwipeDeck cards={cards} emptyLabel={emptyLabel} onSwipe={swipe} />
    </div>
  );
}
```

- [ ] **Step 5: Update account usage type**

Modify `src/client/screens/MeScreen.tsx`:

```ts
type AccountUsage = {
  clones: UsageBucket[];
  generations: UsageBucket[];
  media: UsageBucket[];
  generationUsage?: {
    imagesToday: number;
    dailyLimit: number;
    remaining: number;
    limitResetsAt: string;
  };
};
```

Change the generation meter:

```tsx
<AccountMeter
  label="Daily generations"
  value={usage?.generationUsage?.imagesToday || 0}
  max={usage?.generationUsage?.dailyLimit || 10}
/>
```

- [ ] **Step 6: Run frontend checks**

Run:

```bash
npm run typecheck
npm test
```

Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add src/client/types.ts src/client/router.tsx src/client/components/SwipeDeck.tsx src/client/screens/BlitzScreen.tsx src/client/screens/MeScreen.tsx
git commit -m "feat: connect blitz screen to batch api"
```

---

## Task 12: Stale Batch Reconciliation And Final Verification

**Order:** Run last.

**Can parallelize:** No.

**Files:**
- Modify: `workers/product/src/lib.rs`
- Modify: `workers/product/src/services/blitz.rs`
- Modify: `workers/product/wrangler.product.jsonc`

**Acceptance Criteria:**
- Stuck generating batches older than configured timeout are marked failed or retried.
- All Rust tests, wasm check, TypeScript typecheck, Vitest suite, and full build pass.

- [ ] **Step 1: Add scheduled Worker config**

Modify `workers/product/wrangler.product.jsonc`:

```jsonc
"triggers": {
  "crons": ["*/15 * * * *"]
}
```

Place it as a top-level key near `observability`.

- [ ] **Step 2: Add scheduled event**

Modify `workers/product/src/lib.rs`:

```rust
use worker::{event, Context, Env, MessageBatch, Request, Response, Result as WorkerResult, ScheduledEvent};
```

Add:

```rust
#[event(scheduled)]
pub async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: Context) -> WorkerResult<()> {
    services::blitz::reconcile_stale_batches(&env).await
}
```

- [ ] **Step 3: Implement reconciliation**

Add to `workers/product/src/services/blitz.rs`:

```rust
pub async fn reconcile_stale_batches(env: &Env) -> WorkerResult<()> {
    let db = env.d1("DB")?;
    start_waiting_ready_pools(env).await?;
    let stale_minutes = env
        .var("BLITZ_BATCH_STALE_MINUTES")
        .ok()
        .and_then(|value| value.to_string().parse::<i64>().ok())
        .unwrap_or(45);
    let cutoff = stale_cutoff_iso(stale_minutes);
    db::exec(
        &db,
        r#"
        UPDATE blitz_batches
        SET status = CASE
              WHEN generation_count > 0 THEN 'ready'
              ELSE 'failed'
            END,
            ready_at = CASE
              WHEN generation_count > 0 THEN COALESCE(ready_at, ?)
              ELSE ready_at
            END,
            error_code = CASE
              WHEN generation_count > 0 THEN error_code
              ELSE 'stale_generation_batch'
            END,
            error_message = CASE
              WHEN generation_count > 0 THEN error_message
              ELSE 'Batch was generating beyond the configured timeout.'
            END
        WHERE status = 'generating'
          AND created_at < ?
        "#,
        vec![json!(now_iso_string()), json!(cutoff)],
    )
    .await
}

fn stale_cutoff_iso(minutes: i64) -> String {
    let millis = js_sys::Date::now() - (minutes as f64 * 60_000.0);
    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(millis))
        .to_iso_string()
        .into()
}
```

- [ ] **Step 4: Run full verification**

Run:

```bash
npm run product:test
npm run product:check
npm run typecheck
npm test
npm run build
```

Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add workers/product/src/lib.rs workers/product/src/services/blitz.rs workers/product/wrangler.product.jsonc
git commit -m "feat: reconcile stale blitz batches"
```

---

## Manual Staging Checks

Run these only after all automated verification passes and required secrets are configured:

```bash
wrangler secret put SCRAPECREATORS_API_KEY -c workers/product/wrangler.product.jsonc
wrangler secret put HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FUFU -c workers/product/wrangler.product.jsonc
npm run db:migrate:remote
npm run deploy:product
```

Then perform this smoke test:

1. Create a clone from 5-20 manual reference photos.
2. Select exactly 5 bubbles.
3. Confirm `NICHE_RESEARCH_QUEUE` receives a `seed_from_bubbles` message.
4. Confirm accepted `visual_references` rows have the clone id.
5. Set or confirm `clone_profiles.soul_status = 'ready'` and `provider_soul_id` is populated.
6. Confirm a `blitz_batches` row enters `generating`, then `ready`.
7. Open `/blitz`, swipe one image right and one image left.
8. Confirm `blitz_swipes` rows contain metadata snapshots and the next batch is pre-fetched.

## Self-Review Notes

- Spec coverage: schema, clone-scoped bubbles/research, ScrapeCreators TikTok/Instagram allowlist, Workers AI Kimi-only analysis, freshness filtering, single-human filtering, generation queue, credits, Blitz routes, prefetch influence, account usage, frontend Blitz screen, DLQ config, and stale reconciliation are covered.
- Completion-language scan: no intentionally postponed implementation items remain in the task list.
- Type consistency: message fields use serde camelCase, route/client JSON fields use camelCase, and database columns match the migration names used by queue/service tasks.
