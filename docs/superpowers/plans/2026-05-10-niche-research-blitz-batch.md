# Niche Research & Blitz Batch System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the niche research pipeline and Blitz batch swipe system that creates per-clone visual reference pools and delivers pre-generated image batches with taste-influenced selection.

**Architecture:** The niche research queue consumer scrapes social content via ScrapeCreators HTTP API, extracts/clusters knowledge via Kimi K2.6 on OpenRouter, researches high-engagement visuals, and verifies human presence with vision AI. The Blitz system pre-generates batches of 5 images per clone, triggers the next batch on first swipe, and accumulates taste from completed batches with a one-batch delay. Quotas are per-user (10 free, 50 pro) and enforced server-side.

**Tech Stack:** Rust/Wasm on Cloudflare Workers (`workers-rs`), D1, R2, Cloudflare Queues, OpenRouter (Kimi K2.6), ScrapeCreators HTTP API, `serde_json`, `uuid`, `sha2`.

---

## Scope And Execution Rules

This plan implements the approved design at `docs/superpowers/specs/2026-05-10-niche-research-blitz-batch-design.md`.

Working assumptions:

- The merged Rust Product Worker is the starting point (40 commits, 42 tests passing).
- All new Rust code follows existing patterns in `workers/product/src/`.
- Commit after each task.
- Do not modify files outside the ownership map unless explicitly stated.

## File Ownership Map

**Task 1** (schema): `config/d1/migrations/1002_blitz_niche_research.sql`

**Task 2** (quota domain): `workers/product/src/domain/quota.rs`, `workers/product/src/domain/mod.rs`, `workers/product/tests/domain_tests.rs`

**Task 3** (taste domain): `workers/product/src/services/taste.rs`, `workers/product/src/services/mod.rs`, `workers/product/tests/domain_tests.rs`

**Task 4** (OpenRouter client): `workers/product/src/services/openrouter.rs`, `workers/product/src/services/mod.rs`, `workers/product/tests/domain_tests.rs`

**Task 5** (ScrapeCreators client): `workers/product/src/services/scrape_creators.rs`, `workers/product/src/services/mod.rs`, `workers/product/tests/domain_tests.rs`

**Task 6** (AI tasks + model router): `workers/product/src/ai/tasks.rs`, `workers/product/src/ai/model_router.rs`, `workers/product/tests/domain_tests.rs`

**Task 7** (niche research queue): `workers/product/src/queues/niche_research.rs`, `workers/product/src/queues/messages.rs`, `workers/product/src/queues/mod.rs`

**Task 8** (blitz service): `workers/product/src/services/blitz.rs`, `workers/product/src/services/mod.rs`

**Task 9** (blitz routes): `workers/product/src/routes/blitz.rs`, `workers/product/src/routes/mod.rs`, `workers/product/src/http/router.rs`

**Task 10** (onboarding bubbles update): `workers/product/src/routes/onboarding.rs`

**Task 11** (wrangler config): `workers/product/wrangler.product.jsonc`

**Task 12** (entitlements update): `workers/product/src/domain/entitlements.rs`, `workers/product/tests/domain_tests.rs`

---

## Task 1: D1 Migration For Blitz And Niche Research Schema

**Files:**
- Create: `config/d1/migrations/1002_blitz_niche_research.sql`

- [ ] **Step 1: Create the migration file**

Create `config/d1/migrations/1002_blitz_niche_research.sql` with this content:

```sql
PRAGMA foreign_keys = ON;

-- Blitz batches: pre-generated image sets per clone
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

-- Blitz swipes: per-card feedback
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

-- Indexes for blitz tables
CREATE INDEX IF NOT EXISTS idx_blitz_batches_clone_status
  ON blitz_batches(clone_id, status, batch_number);
CREATE INDEX IF NOT EXISTS idx_blitz_batches_user_date
  ON blitz_batches(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_blitz_swipes_batch
  ON blitz_swipes(batch_id, created_at);
CREATE INDEX IF NOT EXISTS idx_blitz_swipes_user_clone
  ON blitz_swipes(user_id, clone_id, created_at DESC);

-- Link generation jobs to blitz batches
ALTER TABLE generation_jobs ADD COLUMN blitz_batch_id TEXT
  REFERENCES blitz_batches(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_generation_jobs_blitz_batch
  ON generation_jobs(blitz_batch_id);

-- Generation quota on accounts
ALTER TABLE accounts ADD COLUMN daily_generation_limit INTEGER NOT NULL DEFAULT 10;
ALTER TABLE accounts ADD COLUMN generation_quota_reset_at TEXT;
```

- [ ] **Step 2: Verify migration applies locally**

Run:

```bash
npm run db:migrate:local
```

Expected: migration applies without errors. If local D1 is not configured, confirm the SQL is syntactically valid with:

```bash
sqlite3 :memory: < config/d1/migrations/1002_blitz_niche_research.sql
echo $?
```

Expected: exit code 0 (the ALTER TABLE will fail on empty DB but the CREATE TABLE statements succeed).

- [ ] **Step 3: Commit**

```bash
git add config/d1/migrations/1002_blitz_niche_research.sql
git commit -m "feat: add D1 migration for blitz batches and niche research"
```

---

## Task 2: Quota Domain Module

**Files:**
- Create: `workers/product/src/domain/quota.rs`
- Modify: `workers/product/src/domain/mod.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write the failing tests**

Add to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::domain::quota::{
    daily_generation_limit, generations_remaining, QuotaCheck,
    FREE_DAILY_GENERATION_LIMIT, PRO_DAILY_GENERATION_LIMIT,
};

#[test]
fn free_plan_gets_default_generation_limit() {
    assert_eq!(daily_generation_limit("free", None), FREE_DAILY_GENERATION_LIMIT);
}

#[test]
fn pro_plan_gets_pro_generation_limit() {
    assert_eq!(daily_generation_limit("pro", None), PRO_DAILY_GENERATION_LIMIT);
}

#[test]
fn custom_limit_overrides_plan_default() {
    assert_eq!(daily_generation_limit("free", Some(25)), 25);
}

#[test]
fn quota_remaining_subtracts_used_from_limit() {
    let check = generations_remaining(10, 3);
    assert_eq!(check, QuotaCheck { limit: 10, used: 3, remaining: 7, exhausted: false });
}

#[test]
fn quota_exhausted_when_used_equals_limit() {
    let check = generations_remaining(10, 10);
    assert_eq!(check, QuotaCheck { limit: 10, used: 10, remaining: 0, exhausted: true });
}

#[test]
fn quota_exhausted_when_used_exceeds_limit() {
    let check = generations_remaining(10, 15);
    assert_eq!(check, QuotaCheck { limit: 10, used: 15, remaining: 0, exhausted: true });
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: FAIL -- `quota` module not found.

- [ ] **Step 3: Create `workers/product/src/domain/quota.rs`**

```rust
use serde::Serialize;

pub const FREE_DAILY_GENERATION_LIMIT: u32 = 10;
pub const PRO_DAILY_GENERATION_LIMIT: u32 = 50;
pub const DEFAULT_BATCH_SIZE: u32 = 5;

/// Returns the daily generation limit for a plan, with optional per-account override.
pub fn daily_generation_limit(plan: &str, custom_limit: Option<u32>) -> u32 {
    if let Some(limit) = custom_limit {
        return limit;
    }
    match plan {
        "pro" | "studio" => PRO_DAILY_GENERATION_LIMIT,
        _ => FREE_DAILY_GENERATION_LIMIT,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCheck {
    pub limit: u32,
    pub used: u32,
    pub remaining: u32,
    pub exhausted: bool,
}

/// Compute remaining quota given limit and today's usage count.
pub fn generations_remaining(limit: u32, used: u32) -> QuotaCheck {
    let remaining = limit.saturating_sub(used);
    QuotaCheck {
        limit,
        used,
        remaining,
        exhausted: remaining == 0,
    }
}

/// Parse batch size from env var string, falling back to default.
pub fn parse_batch_size(env_value: Option<&str>) -> u32 {
    env_value
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|&v| v >= 1 && v <= 20)
        .unwrap_or(DEFAULT_BATCH_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_batch_size_defaults_to_5() {
        assert_eq!(parse_batch_size(None), 5);
        assert_eq!(parse_batch_size(Some("")), 5);
        assert_eq!(parse_batch_size(Some("abc")), 5);
    }

    #[test]
    fn parse_batch_size_clamps_to_valid_range() {
        assert_eq!(parse_batch_size(Some("0")), 5);
        assert_eq!(parse_batch_size(Some("21")), 5);
        assert_eq!(parse_batch_size(Some("7")), 7);
    }
}
```

- [ ] **Step 4: Add `quota` to `workers/product/src/domain/mod.rs`**

Add this line after the existing module declarations:

```rust
pub mod quota;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: all new quota tests PASS, all existing tests PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/domain/quota.rs workers/product/src/domain/mod.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add quota domain module with generation limits"
```

---

## Task 3: Taste Snapshot Service

**Files:**
- Create: `workers/product/src/services/taste.rs`
- Modify: `workers/product/src/services/mod.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write the failing tests**

Add to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::services::taste::{
    build_taste_snapshot, score_reference, SwipeRecord, TasteSnapshot, VisualRefCandidate,
};

#[test]
fn empty_swipes_produce_empty_snapshot() {
    let snapshot = build_taste_snapshot(&[]);
    assert_eq!(snapshot.total_swipes, 0);
    assert_eq!(snapshot.likes, 0);
    assert_eq!(snapshot.dislikes, 0);
    assert!(snapshot.liked_tags.is_empty());
    assert!(snapshot.disliked_tags.is_empty());
}

#[test]
fn snapshot_counts_likes_and_dislikes() {
    let swipes = vec![
        SwipeRecord { direction: "like".to_string(), aesthetic_tags: vec!["neon".to_string()], visual_reference_id: Some("vr1".to_string()) },
        SwipeRecord { direction: "dislike".to_string(), aesthetic_tags: vec!["pastel".to_string()], visual_reference_id: Some("vr2".to_string()) },
        SwipeRecord { direction: "like".to_string(), aesthetic_tags: vec!["neon".to_string(), "urban".to_string()], visual_reference_id: Some("vr3".to_string()) },
    ];
    let snapshot = build_taste_snapshot(&swipes);
    assert_eq!(snapshot.total_swipes, 3);
    assert_eq!(snapshot.likes, 2);
    assert_eq!(snapshot.dislikes, 1);
    assert_eq!(*snapshot.liked_tags.get("neon").unwrap(), 2);
    assert_eq!(*snapshot.liked_tags.get("urban").unwrap(), 1);
    assert_eq!(*snapshot.disliked_tags.get("pastel").unwrap(), 1);
    assert_eq!(snapshot.liked_visual_ref_ids.len(), 2);
    assert_eq!(snapshot.disliked_visual_ref_ids.len(), 1);
}

#[test]
fn reference_scoring_boosts_liked_tags() {
    let snapshot = TasteSnapshot {
        total_swipes: 5,
        likes: 3,
        dislikes: 2,
        liked_tags: [("neon".to_string(), 3), ("urban".to_string(), 1)].into_iter().collect(),
        disliked_tags: [("pastel".to_string(), 2)].into_iter().collect(),
        liked_visual_ref_ids: vec![],
        disliked_visual_ref_ids: vec![],
    };

    let liked_ref = VisualRefCandidate {
        id: "a".to_string(),
        aesthetic_tags: vec!["neon".to_string()],
        base_score: 1.0,
        used: false,
    };
    let disliked_ref = VisualRefCandidate {
        id: "b".to_string(),
        aesthetic_tags: vec!["pastel".to_string()],
        base_score: 1.0,
        used: false,
    };

    assert!(score_reference(&liked_ref, &snapshot) > score_reference(&disliked_ref, &snapshot));
}

#[test]
fn unused_references_get_freshness_bonus() {
    let snapshot = build_taste_snapshot(&[]);

    let used = VisualRefCandidate { id: "a".to_string(), aesthetic_tags: vec![], base_score: 1.0, used: true };
    let unused = VisualRefCandidate { id: "b".to_string(), aesthetic_tags: vec![], base_score: 1.0, used: false };

    assert!(score_reference(&unused, &snapshot) > score_reference(&used, &snapshot));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: FAIL -- `taste` module not found.

- [ ] **Step 3: Create `workers/product/src/services/taste.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct SwipeRecord {
    pub direction: String,
    pub aesthetic_tags: Vec<String>,
    pub visual_reference_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TasteSnapshot {
    pub total_swipes: u32,
    pub likes: u32,
    pub dislikes: u32,
    pub liked_tags: HashMap<String, u32>,
    pub disliked_tags: HashMap<String, u32>,
    pub liked_visual_ref_ids: Vec<String>,
    pub disliked_visual_ref_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct VisualRefCandidate {
    pub id: String,
    pub aesthetic_tags: Vec<String>,
    pub base_score: f64,
    pub used: bool,
}

pub fn build_taste_snapshot(swipes: &[SwipeRecord]) -> TasteSnapshot {
    let mut liked_tags: HashMap<String, u32> = HashMap::new();
    let mut disliked_tags: HashMap<String, u32> = HashMap::new();
    let mut liked_refs = Vec::new();
    let mut disliked_refs = Vec::new();
    let mut likes = 0u32;
    let mut dislikes = 0u32;

    for swipe in swipes {
        let is_like = swipe.direction == "like";
        if is_like {
            likes += 1;
        } else {
            dislikes += 1;
        }

        let tag_map = if is_like { &mut liked_tags } else { &mut disliked_tags };
        for tag in &swipe.aesthetic_tags {
            *tag_map.entry(tag.clone()).or_insert(0) += 1;
        }

        if let Some(ref vr_id) = swipe.visual_reference_id {
            if is_like {
                liked_refs.push(vr_id.clone());
            } else {
                disliked_refs.push(vr_id.clone());
            }
        }
    }

    TasteSnapshot {
        total_swipes: swipes.len() as u32,
        likes,
        dislikes,
        liked_tags,
        disliked_tags,
        liked_visual_ref_ids: liked_refs,
        disliked_visual_ref_ids: disliked_refs,
    }
}

const LIKED_TAG_BOOST: f64 = 0.3;
const DISLIKED_TAG_PENALTY: f64 = 0.2;
const FRESHNESS_BONUS: f64 = 0.5;

pub fn score_reference(candidate: &VisualRefCandidate, taste: &TasteSnapshot) -> f64 {
    let mut score = candidate.base_score;

    for tag in &candidate.aesthetic_tags {
        if let Some(&count) = taste.liked_tags.get(tag) {
            score += LIKED_TAG_BOOST * count as f64;
        }
        if let Some(&count) = taste.disliked_tags.get(tag) {
            score -= DISLIKED_TAG_PENALTY * count as f64;
        }
    }

    if !candidate.used {
        score += FRESHNESS_BONUS;
    }

    score
}

/// Select top N references by taste-influenced score.
pub fn select_references(
    candidates: &[VisualRefCandidate],
    taste: &TasteSnapshot,
    count: usize,
) -> Vec<String> {
    let mut scored: Vec<(&VisualRefCandidate, f64)> = candidates
        .iter()
        .map(|c| (c, score_reference(c, taste)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(count).map(|(c, _)| c.id.clone()).collect()
}
```

- [ ] **Step 4: Add `taste` to `workers/product/src/services/mod.rs`**

Add this line after the existing module declarations:

```rust
pub mod taste;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: all taste tests PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/services/taste.rs workers/product/src/services/mod.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add taste snapshot builder and reference scoring"
```

---

## Task 4: OpenRouter Client

**Files:**
- Create: `workers/product/src/services/openrouter.rs`
- Modify: `workers/product/src/services/mod.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write the failing tests**

Add to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::services::openrouter::{
    build_chat_request, parse_chat_response, ChatMessage, ChatMessageContent,
    OpenRouterRequest,
};

#[test]
fn chat_request_serializes_with_model_and_json_format() {
    let req = build_chat_request(
        "moonshotai/kimi-k2.6",
        vec![
            ChatMessage::system("You are helpful."),
            ChatMessage::user_text("Extract queries."),
        ],
        0.7,
    );
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["model"], "moonshotai/kimi-k2.6");
    assert_eq!(json["response_format"]["type"], "json_object");
    assert_eq!(json["temperature"], 0.7);
    assert_eq!(json["messages"].as_array().unwrap().len(), 2);
}

#[test]
fn chat_request_supports_vision_messages() {
    let req = build_chat_request(
        "moonshotai/kimi-k2.6",
        vec![ChatMessage::user_vision("Analyze this.", "https://example.com/img.jpg")],
        0.7,
    );
    let json = serde_json::to_value(&req).unwrap();
    let content = &json["messages"][0]["content"];
    assert!(content.is_array());
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image_url");
}

#[test]
fn parse_response_extracts_content_text() {
    let body = r#"{
        "choices": [{ "message": { "content": "{\"queries\": []}" } }]
    }"#;
    let result = parse_chat_response(body).unwrap();
    assert_eq!(result, "{\"queries\": []}");
}

#[test]
fn parse_response_returns_error_on_empty_choices() {
    let body = r#"{"choices": []}"#;
    assert!(parse_chat_response(body).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: FAIL -- `openrouter` module not found.

- [ ] **Step 3: Create `workers/product/src/services/openrouter.rs`**

```rust
use serde::{Deserialize, Serialize};

pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_MODEL: &str = "moonshotai/kimi-k2.6";

#[derive(Debug, Clone, Serialize)]
pub struct OpenRouterRequest {
    pub model: String,
    pub messages: Vec<ChatMessagePayload>,
    pub response_format: ResponseFormat,
    pub temperature: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessagePayload {
    pub role: String,
    pub content: ChatMessageContent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ChatMessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrlPart },
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageUrlPart {
    pub url: String,
}

/// High-level message builder.
pub struct ChatMessage;

impl ChatMessage {
    pub fn system(text: &str) -> ChatMessagePayload {
        ChatMessagePayload {
            role: "system".to_string(),
            content: ChatMessageContent::Text(text.to_string()),
        }
    }

    pub fn user_text(text: &str) -> ChatMessagePayload {
        ChatMessagePayload {
            role: "user".to_string(),
            content: ChatMessageContent::Text(text.to_string()),
        }
    }

    pub fn user_vision(text: &str, image_url: &str) -> ChatMessagePayload {
        ChatMessagePayload {
            role: "user".to_string(),
            content: ChatMessageContent::Parts(vec![
                ContentPart::Text { text: text.to_string() },
                ContentPart::ImageUrl {
                    image_url: ImageUrlPart { url: image_url.to_string() },
                },
            ]),
        }
    }
}

pub fn build_chat_request(
    model: &str,
    messages: Vec<ChatMessagePayload>,
    temperature: f64,
) -> OpenRouterRequest {
    OpenRouterRequest {
        model: model.to_string(),
        messages,
        response_format: ResponseFormat {
            format_type: "json_object".to_string(),
        },
        temperature,
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

pub fn parse_chat_response(body: &str) -> Result<String, String> {
    let response: ChatCompletionResponse =
        serde_json::from_str(body).map_err(|e| format!("Failed to parse OpenRouter response: {e}"))?;
    response
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| "OpenRouter response had no choices".to_string())
}
```

- [ ] **Step 4: Add `openrouter` to `workers/product/src/services/mod.rs`**

Add this line:

```rust
pub mod openrouter;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: all openrouter tests PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/services/openrouter.rs workers/product/src/services/mod.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add OpenRouter client for Kimi K2.6"
```

---

## Task 5: ScrapeCreators Client

**Files:**
- Create: `workers/product/src/services/scrape_creators.rs`
- Modify: `workers/product/src/services/mod.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write the failing tests**

Add to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::services::scrape_creators::{
    parse_reddit_search_response, parse_tiktok_search_response,
    RedditPost, TikTokVideo, scrape_endpoint_url,
};

#[test]
fn reddit_search_url_includes_query_params() {
    let url = scrape_endpoint_url("/v1/reddit/search", &[("query", "fashion tips"), ("sort", "top"), ("timeframe", "month")]);
    assert_eq!(url, "https://api.scrapecreators.com/v1/reddit/search?query=fashion+tips&sort=top&timeframe=month");
}

#[test]
fn parse_reddit_search_extracts_posts() {
    let body = r#"{"data": [{"title": "Cool outfit", "selftext": "Check this out", "url": "https://reddit.com/r/fashion/1"}]}"#;
    let posts = parse_reddit_search_response(body).unwrap();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].title, "Cool outfit");
    assert_eq!(posts[0].body, "Check this out");
}

#[test]
fn parse_reddit_handles_missing_data() {
    let body = r#"{}"#;
    let posts = parse_reddit_search_response(body).unwrap();
    assert!(posts.is_empty());
}

#[test]
fn parse_tiktok_search_extracts_videos() {
    let body = r#"{"data": [{"description": "OOTD inspo", "url": "https://tiktok.com/v/1", "cover": "https://img.com/1.jpg", "digg_count": 15000}]}"#;
    let videos = parse_tiktok_search_response(body).unwrap();
    assert_eq!(videos.len(), 1);
    assert_eq!(videos[0].description, "OOTD inspo");
    assert_eq!(videos[0].cover_url.as_deref(), Some("https://img.com/1.jpg"));
    assert_eq!(videos[0].likes, 15000);
}

#[test]
fn parse_tiktok_handles_missing_data() {
    let body = r#"{}"#;
    let videos = parse_tiktok_search_response(body).unwrap();
    assert!(videos.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: FAIL -- `scrape_creators` module not found.

- [ ] **Step 3: Create `workers/product/src/services/scrape_creators.rs`**

```rust
use serde::Deserialize;

pub const SCRAPECREATORS_BASE_URL: &str = "https://api.scrapecreators.com";

pub fn scrape_endpoint_url(path: &str, params: &[(&str, &str)]) -> String {
    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v.replace(' ', "+")))
        .collect::<Vec<_>>()
        .join("&");
    if query.is_empty() {
        format!("{}{}", SCRAPECREATORS_BASE_URL, path)
    } else {
        format!("{}{}?{}", SCRAPECREATORS_BASE_URL, path, query)
    }
}

// --- Reddit ---

#[derive(Debug, Clone)]
pub struct RedditPost {
    pub title: String,
    pub body: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct RedditSearchResponse {
    data: Option<Vec<RedditPostRaw>>,
}

#[derive(Debug, Deserialize)]
struct RedditPostRaw {
    title: Option<String>,
    selftext: Option<String>,
    body: Option<String>,
    url: Option<String>,
    permalink: Option<String>,
}

pub fn parse_reddit_search_response(body: &str) -> Result<Vec<RedditPost>, String> {
    let response: RedditSearchResponse =
        serde_json::from_str(body).map_err(|e| format!("Failed to parse Reddit response: {e}"))?;
    Ok(response
        .data
        .unwrap_or_default()
        .into_iter()
        .map(|raw| RedditPost {
            title: raw.title.unwrap_or_default(),
            body: raw.selftext.or(raw.body).unwrap_or_default(),
            url: raw.url.or(raw.permalink).unwrap_or_default(),
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct RedditCommentsResponse {
    data: Option<Vec<RedditCommentRaw>>,
}

#[derive(Debug, Deserialize)]
struct RedditCommentRaw {
    body: Option<String>,
    text: Option<String>,
}

pub fn parse_reddit_comments_response(body: &str) -> Result<Vec<String>, String> {
    let response: RedditCommentsResponse =
        serde_json::from_str(body).map_err(|e| format!("Failed to parse Reddit comments: {e}"))?;
    Ok(response
        .data
        .unwrap_or_default()
        .into_iter()
        .filter_map(|raw| {
            let text = raw.body.or(raw.text).unwrap_or_default();
            if text.len() > 30 { Some(text) } else { None }
        })
        .collect())
}

// --- TikTok ---

#[derive(Debug, Clone)]
pub struct TikTokVideo {
    pub description: String,
    pub url: String,
    pub cover_url: Option<String>,
    pub likes: u64,
}

#[derive(Debug, Deserialize)]
struct TikTokSearchResponse {
    data: Option<Vec<TikTokVideoRaw>>,
}

#[derive(Debug, Deserialize)]
struct TikTokVideoRaw {
    description: Option<String>,
    title: Option<String>,
    desc: Option<String>,
    url: Option<String>,
    video_url: Option<String>,
    cover: Option<String>,
    origin_cover: Option<String>,
    thumbnail: Option<String>,
    likes: Option<u64>,
    digg_count: Option<u64>,
    #[serde(default)]
    stats: Option<TikTokStatsRaw>,
}

#[derive(Debug, Deserialize)]
struct TikTokStatsRaw {
    #[serde(rename = "diggCount")]
    digg_count: Option<u64>,
}

pub fn parse_tiktok_search_response(body: &str) -> Result<Vec<TikTokVideo>, String> {
    let response: TikTokSearchResponse =
        serde_json::from_str(body).map_err(|e| format!("Failed to parse TikTok response: {e}"))?;
    Ok(response
        .data
        .unwrap_or_default()
        .into_iter()
        .map(|raw| {
            let description = raw.description
                .or(raw.title)
                .or(raw.desc)
                .unwrap_or_default();
            let url = raw.url.or(raw.video_url).unwrap_or_default();
            let cover_url = raw.cover.or(raw.origin_cover).or(raw.thumbnail);
            let likes = raw.likes
                .or(raw.digg_count)
                .or_else(|| raw.stats.and_then(|s| s.digg_count))
                .unwrap_or(0);
            TikTokVideo { description, url, cover_url, likes }
        })
        .collect())
}

/// Filter videos by engagement threshold and deduplicate by URL.
pub fn filter_high_engagement(videos: Vec<TikTokVideo>, threshold: u64) -> Vec<TikTokVideo> {
    let mut seen = std::collections::HashSet::new();
    videos
        .into_iter()
        .filter(|v| {
            v.likes >= threshold && !v.url.is_empty() && seen.insert(v.url.clone())
        })
        .collect()
}
```

- [ ] **Step 4: Add `scrape_creators` to `workers/product/src/services/mod.rs`**

Add this line:

```rust
pub mod scrape_creators;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: all scrape_creators tests PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/services/scrape_creators.rs workers/product/src/services/mod.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add ScrapeCreators HTTP response parsers"
```

---

## Task 6: AI Task And Model Router Updates

**Files:**
- Modify: `workers/product/src/ai/tasks.rs`
- Modify: `workers/product/src/ai/model_router.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `workers/product/tests/domain_tests.rs`:

```rust
#[test]
fn blitz_taste_influence_does_not_require_vision() {
    assert!(!AiTask::BlitzTasteInfluence.requires_vision());
}

#[test]
fn blitz_taste_influence_selects_structured_text_model() {
    let models = vec![
        ModelConfig {
            provider: "openrouter".to_string(),
            model: "moonshotai/kimi-k2.6".to_string(),
            supports_vision: true,
            supports_structured_json: true,
        },
    ];
    let selected = choose_model(AiTask::BlitzTasteInfluence, &models).unwrap();
    assert_eq!(selected.model, "moonshotai/kimi-k2.6");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: FAIL -- `BlitzTasteInfluence` not found.

- [ ] **Step 3: Add `BlitzTasteInfluence` to `workers/product/src/ai/tasks.rs`**

Replace the `AiTask` enum with:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiTask {
    PhotoQualityReview,
    HumanPresenceDetection,
    BubbleGeneration,
    NicheSeedExtraction,
    NicheClusterExpansion,
    VisualReferenceSelection,
    Moderation,
    BlitzTasteInfluence,
}

impl AiTask {
    pub fn requires_vision(self) -> bool {
        matches!(
            self,
            AiTask::PhotoQualityReview
                | AiTask::HumanPresenceDetection
                | AiTask::VisualReferenceSelection
        )
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: all AI task tests PASS, including existing ones.

- [ ] **Step 5: Commit**

```bash
git add workers/product/src/ai/tasks.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add BlitzTasteInfluence AI task"
```

---

## Task 7: Niche Research Queue Messages And Handler Scaffold

**Files:**
- Modify: `workers/product/src/queues/messages.rs`
- Modify: `workers/product/src/queues/niche_research.rs`
- Modify: `workers/product/src/queues/mod.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write the failing tests**

Add to `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::queues::messages::NicheResearchMessage;

#[test]
fn expand_clusters_message_serializes_correctly() {
    let msg = NicheResearchMessage::ExpandClusters {
        user_id: "u1".to_string(),
        clone_id: "c1".to_string(),
    };
    let value = serde_json::to_value(msg).unwrap();
    assert_eq!(value["type"], "expand_clusters");
    assert_eq!(value["userId"], "u1");
    assert_eq!(value["cloneId"], "c1");
}

#[test]
fn research_visuals_message_serializes_correctly() {
    let msg = NicheResearchMessage::ResearchVisuals {
        user_id: "u1".to_string(),
        clone_id: "c1".to_string(),
        search_terms: vec!["fashion tips".to_string()],
        hashtags: vec!["ootd".to_string()],
        engagement_threshold: 10000,
    };
    let value = serde_json::to_value(msg).unwrap();
    assert_eq!(value["type"], "research_visuals");
    assert_eq!(value["searchTerms"].as_array().unwrap().len(), 1);
    assert_eq!(value["engagementThreshold"], 10000);
}

#[test]
fn verify_human_presence_message_serializes_correctly() {
    let msg = NicheResearchMessage::VerifyHumanPresence {
        user_id: "u1".to_string(),
        clone_id: "c1".to_string(),
        candidate_ids: vec!["cand1".to_string(), "cand2".to_string()],
    };
    let value = serde_json::to_value(msg).unwrap();
    assert_eq!(value["type"], "verify_human_presence");
    assert_eq!(value["candidateIds"].as_array().unwrap().len(), 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: FAIL -- `ExpandClusters` variant not found.

- [ ] **Step 3: Update `NicheResearchMessage` in `workers/product/src/queues/niche_research.rs`**

Replace the `NicheResearchMessage` enum:

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

- [ ] **Step 4: Update `handle_batch` to match new variants**

In `workers/product/src/queues/niche_research.rs`, replace the `handle_batch` function:

```rust
pub async fn handle_batch(batch: MessageBatch<Value>, _env: Env) -> WorkerResult<()> {
    for raw_message in batch.raw_iter() {
        let body = match serde_wasm_bindgen::from_value::<NicheResearchMessage>(raw_message.body())
        {
            Ok(body) => body,
            Err(error) => {
                web_sys::console::error_1(
                    &format!("failed to deserialize niche research queue message: {error:?}")
                        .into(),
                );
                raw_message.ack();
                continue;
            }
        };

        match &body {
            NicheResearchMessage::SeedFromBubbles {
                user_id,
                clone_id,
                bubble_ids,
                moderation_level,
            } => {
                web_sys::console::log_1(
                    &format!(
                        "ack niche research seed user={user_id} clone={clone_id} bubbles={} moderation={moderation_level}",
                        bubble_ids.len()
                    )
                    .into(),
                );
            }
            NicheResearchMessage::ExpandClusters { user_id, clone_id } => {
                web_sys::console::log_1(
                    &format!("ack niche research expand user={user_id} clone={clone_id}").into(),
                );
            }
            NicheResearchMessage::ResearchVisuals {
                user_id,
                clone_id,
                search_terms,
                hashtags,
                engagement_threshold,
            } => {
                web_sys::console::log_1(
                    &format!(
                        "ack niche research visuals user={user_id} clone={clone_id} terms={} tags={} threshold={engagement_threshold}",
                        search_terms.len(),
                        hashtags.len()
                    )
                    .into(),
                );
            }
            NicheResearchMessage::VerifyHumanPresence {
                user_id,
                clone_id,
                candidate_ids,
            } => {
                web_sys::console::log_1(
                    &format!(
                        "ack niche research verify user={user_id} clone={clone_id} candidates={}",
                        candidate_ids.len()
                    )
                    .into(),
                );
            }
        }

        raw_message.ack();
    }

    Ok(())
}
```

- [ ] **Step 5: Re-export `NicheResearchMessage` from `workers/product/src/queues/messages.rs`**

Add to `workers/product/src/queues/messages.rs` at the top:

```rust
pub use super::niche_research::NicheResearchMessage;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: all niche research message tests PASS.

- [ ] **Step 7: Commit**

```bash
git add workers/product/src/queues/messages.rs workers/product/src/queues/niche_research.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add niche research queue message variants"
```

---

## Task 8: Blitz Batch Service

**Files:**
- Create: `workers/product/src/services/blitz.rs`
- Modify: `workers/product/src/services/mod.rs`

- [ ] **Step 1: Create `workers/product/src/services/blitz.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchStatus {
    Pending,
    Generating,
    Ready,
    Swiping,
    Completed,
    Failed,
}

impl BatchStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Generating => "generating",
            Self::Ready => "ready",
            Self::Swiping => "swiping",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "generating" => Some(Self::Generating),
            "ready" => Some(Self::Ready),
            "swiping" => Some(Self::Swiping),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

pub fn can_transition_batch(from: BatchStatus, to: BatchStatus) -> bool {
    matches!(
        (from, to),
        (BatchStatus::Pending, BatchStatus::Generating)
            | (BatchStatus::Generating, BatchStatus::Ready)
            | (BatchStatus::Generating, BatchStatus::Failed)
            | (BatchStatus::Ready, BatchStatus::Swiping)
            | (BatchStatus::Swiping, BatchStatus::Completed)
    )
}

pub fn batch_idempotency_key(clone_id: &str, batch_number: u32) -> String {
    format!("blitz:{}:{}", clone_id, batch_number)
}

pub fn swipe_idempotency_key(batch_id: &str, output_id: &str) -> String {
    format!("swipe:{}:{}", batch_id, output_id)
}

/// Check if a next batch should be triggered. Returns true when:
/// - This is the first swipe of the current batch (swiped_count_before == 0)
/// - No next batch exists yet (next_batch_exists == false)
/// - Quota is not exhausted (quota_remaining > 0)
pub fn should_trigger_next_batch(
    swiped_count_before: u32,
    next_batch_exists: bool,
    quota_remaining: u32,
    batch_size: u32,
) -> bool {
    swiped_count_before == 0
        && !next_batch_exists
        && quota_remaining >= batch_size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_transitions_are_valid() {
        assert!(can_transition_batch(BatchStatus::Pending, BatchStatus::Generating));
        assert!(can_transition_batch(BatchStatus::Generating, BatchStatus::Ready));
        assert!(can_transition_batch(BatchStatus::Generating, BatchStatus::Failed));
        assert!(can_transition_batch(BatchStatus::Ready, BatchStatus::Swiping));
        assert!(can_transition_batch(BatchStatus::Swiping, BatchStatus::Completed));
    }

    #[test]
    fn invalid_batch_transitions_are_rejected() {
        assert!(!can_transition_batch(BatchStatus::Pending, BatchStatus::Ready));
        assert!(!can_transition_batch(BatchStatus::Ready, BatchStatus::Pending));
        assert!(!can_transition_batch(BatchStatus::Completed, BatchStatus::Swiping));
        assert!(!can_transition_batch(BatchStatus::Failed, BatchStatus::Generating));
    }

    #[test]
    fn batch_idempotency_key_is_deterministic() {
        assert_eq!(batch_idempotency_key("c1", 3), "blitz:c1:3");
    }

    #[test]
    fn swipe_idempotency_key_is_deterministic() {
        assert_eq!(swipe_idempotency_key("b1", "out1"), "swipe:b1:out1");
    }

    #[test]
    fn trigger_next_batch_on_first_swipe_only() {
        assert!(should_trigger_next_batch(0, false, 10, 5));
        assert!(!should_trigger_next_batch(1, false, 10, 5));
        assert!(!should_trigger_next_batch(0, true, 10, 5));
        assert!(!should_trigger_next_batch(0, false, 3, 5));
    }
}
```

- [ ] **Step 2: Add `blitz` to `workers/product/src/services/mod.rs`**

Add this line:

```rust
pub mod blitz;
```

- [ ] **Step 3: Run tests**

Run: `cd workers/product && cargo test --lib --test domain_tests 2>&1 | tail -5`

Expected: all tests PASS including new blitz service tests.

- [ ] **Step 4: Commit**

```bash
git add workers/product/src/services/blitz.rs workers/product/src/services/mod.rs
git commit -m "feat: add blitz batch service with state transitions"
```

---

## Task 9: Blitz Routes

**Files:**
- Create: `workers/product/src/routes/blitz.rs`
- Modify: `workers/product/src/routes/mod.rs`
- Modify: `workers/product/src/http/router.rs`

- [ ] **Step 1: Create `workers/product/src/routes/blitz.rs`**

```rust
use crate::auth_client::verify_session;
use crate::db;
use crate::domain::quota::{daily_generation_limit, generations_remaining, parse_batch_size};
use crate::http::error::ApiError;
use crate::services::blitz::{batch_idempotency_key, should_trigger_next_batch};
use crate::services::taste::{build_taste_snapshot, SwipeRecord, TasteSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use worker::{Request, Response, Result as WorkerResult, RouteContext};

// --- Response types ---

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BlitzCurrentResponse {
    batch: Option<BatchResponse>,
    next_batch_status: Option<String>,
    quota_remaining: u32,
    quota_total: u32,
    quota_exhausted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchResponse {
    id: String,
    clone_id: String,
    batch_number: u32,
    status: String,
    cards: Vec<BlitzCard>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlitzCard {
    output_id: String,
    media_id: Option<String>,
    media_url: Option<String>,
    visual_reference_id: Option<String>,
    aesthetic_tags: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SwipeResponse {
    swipe_id: String,
    batch_progress: BatchProgress,
    next_batch_triggered: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchProgress {
    swiped: u32,
    total: u32,
}

// --- Request types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwipeRequest {
    batch_id: String,
    output_id: String,
    direction: String,
}

// --- DB row types ---

#[derive(Debug, Deserialize)]
struct BatchRow {
    id: String,
    clone_id: String,
    batch_number: u32,
    status: String,
    batch_size: u32,
}

#[derive(Debug, Deserialize)]
struct CardRow {
    output_id: String,
    media_id: Option<String>,
    visual_reference_id: Option<String>,
    aesthetic_tags_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u32,
}

#[derive(Debug, Deserialize)]
struct AccountQuotaRow {
    plan: String,
    daily_generation_limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SwipeRow {
    direction: String,
    aesthetic_tags_json: String,
    visual_reference_id: Option<String>,
}

// --- Handlers ---

pub async fn blitz_current(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };

    let clone_id = match req.url().ok().and_then(|u| {
        u.query_pairs()
            .find(|(k, _)| k == "cloneId")
            .map(|(_, v)| v.to_string())
    }) {
        Some(id) if !id.trim().is_empty() => id,
        _ => {
            return ApiError::bad_request("missing_clone_id", "cloneId query parameter is required.")
                .to_response()
        }
    };

    let d1 = ctx.env.d1("DB")?;

    // Verify clone belongs to user
    let clone_check = db::first::<CountRow>(
        &d1,
        "SELECT COUNT(*) AS count FROM clone_profiles WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        vec![json!(clone_id), json!(auth.user_id)],
    ).await?;
    if clone_check.map(|r| r.count).unwrap_or(0) == 0 {
        return ApiError::not_found("clone_not_found", "Clone not found.").to_response();
    }

    // Get quota
    let account = db::first::<AccountQuotaRow>(
        &d1,
        "SELECT plan, daily_generation_limit FROM accounts WHERE user_id = ?",
        vec![json!(auth.user_id)],
    ).await?;
    let (plan, custom_limit) = account
        .map(|a| (a.plan, a.daily_generation_limit))
        .unwrap_or(("free".to_string(), None));
    let limit = daily_generation_limit(&plan, custom_limit);

    let today_used = db::first::<CountRow>(
        &d1,
        "SELECT COUNT(*) AS count FROM generation_jobs WHERE user_id = ? AND DATE(queued_at) = DATE('now')",
        vec![json!(auth.user_id)],
    ).await?.map(|r| r.count).unwrap_or(0);
    let quota = generations_remaining(limit, today_used);

    // Find current active batch (latest non-completed for this clone)
    let batch = db::first::<BatchRow>(
        &d1,
        r#"SELECT id, clone_id, batch_number, status, batch_size
           FROM blitz_batches
           WHERE clone_id = ? AND user_id = ? AND status IN ('ready', 'swiping', 'generating', 'pending')
           ORDER BY batch_number DESC LIMIT 1"#,
        vec![json!(clone_id), json!(auth.user_id)],
    ).await?;

    let (batch_response, next_status) = if let Some(b) = batch {
        let cards = if b.status == "ready" || b.status == "swiping" {
            load_batch_cards(&d1, &b.id).await?
        } else {
            vec![]
        };

        // Check for next batch
        let next = db::first::<BatchRow>(
            &d1,
            r#"SELECT id, clone_id, batch_number, status, batch_size
               FROM blitz_batches
               WHERE clone_id = ? AND user_id = ? AND batch_number = ?
               LIMIT 1"#,
            vec![json!(clone_id), json!(auth.user_id), json!(b.batch_number + 1)],
        ).await?;

        let next_status = next.map(|n| n.status);

        (Some(BatchResponse {
            id: b.id,
            clone_id: b.clone_id,
            batch_number: b.batch_number,
            status: b.status,
            cards,
        }), next_status)
    } else {
        (None, None)
    };

    Response::from_json(&BlitzCurrentResponse {
        batch: batch_response,
        next_batch_status: next_status,
        quota_remaining: quota.remaining,
        quota_total: quota.limit,
        quota_exhausted: quota.exhausted,
    })
}

pub async fn blitz_swipe(mut req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };

    let input = match req.json::<SwipeRequest>().await {
        Ok(input) => input,
        Err(_) => {
            return ApiError::bad_request(
                "invalid_swipe_request",
                "Expected batchId, outputId, and direction (like/dislike).",
            ).to_response()
        }
    };

    if input.direction != "like" && input.direction != "dislike" {
        return ApiError::bad_request(
            "invalid_direction",
            "Direction must be 'like' or 'dislike'.",
        ).to_response();
    }

    let d1 = ctx.env.d1("DB")?;

    // Verify batch belongs to user
    let batch = match db::first::<BatchRow>(
        &d1,
        "SELECT id, clone_id, batch_number, status, batch_size FROM blitz_batches WHERE id = ? AND user_id = ?",
        vec![json!(input.batch_id), json!(auth.user_id)],
    ).await? {
        Some(b) => b,
        None => return ApiError::not_found("batch_not_found", "Batch not found.").to_response(),
    };

    if batch.status != "ready" && batch.status != "swiping" {
        return ApiError::conflict("batch_not_swipable", "Batch is not in a swipable state.").to_response();
    }

    // Count existing swipes for this batch
    let swiped_before = db::first::<CountRow>(
        &d1,
        "SELECT COUNT(*) AS count FROM blitz_swipes WHERE batch_id = ?",
        vec![json!(batch.id)],
    ).await?.map(|r| r.count).unwrap_or(0);

    // Get aesthetic tags from the generation output's visual reference
    let aesthetic_tags_json = db::first::<CardRow>(
        &d1,
        r#"SELECT go.id AS output_id, go.media_asset_id AS media_id,
                  gj.input_visual_reference_id AS visual_reference_id,
                  vr.aesthetic_tags_json
           FROM generation_outputs go
           JOIN generation_jobs gj ON gj.id = go.job_id
           LEFT JOIN visual_references vr ON vr.id = gj.input_visual_reference_id
           WHERE go.id = ? AND go.user_id = ?"#,
        vec![json!(input.output_id), json!(auth.user_id)],
    ).await?.and_then(|c| c.aesthetic_tags_json).unwrap_or_else(|| "[]".to_string());

    // Insert swipe (idempotent via UNIQUE(batch_id, generation_output_id))
    let swipe_id = format!("swipe_{}", Uuid::new_v4().simple());
    let now = now_iso_string();

    let visual_ref_id = db::first::<CardRow>(
        &d1,
        r#"SELECT go.id AS output_id, NULL AS media_id,
                  gj.input_visual_reference_id AS visual_reference_id,
                  NULL AS aesthetic_tags_json
           FROM generation_outputs go
           JOIN generation_jobs gj ON gj.id = go.job_id
           WHERE go.id = ?"#,
        vec![json!(input.output_id)],
    ).await?.and_then(|c| c.visual_reference_id);

    db::exec(
        &d1,
        r#"INSERT OR IGNORE INTO blitz_swipes
           (id, user_id, clone_id, batch_id, generation_output_id, visual_reference_id, direction, aesthetic_tags_json, metadata_json, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, '{}', ?)"#,
        vec![
            json!(swipe_id),
            json!(auth.user_id),
            json!(batch.clone_id),
            json!(batch.id),
            json!(input.output_id),
            json!(visual_ref_id),
            json!(input.direction),
            json!(aesthetic_tags_json),
            json!(now),
        ],
    ).await?;

    // Update batch status to swiping if it was ready
    if batch.status == "ready" {
        db::exec(
            &d1,
            "UPDATE blitz_batches SET status = 'swiping', updated_at = ? WHERE id = ? AND status = 'ready'",
            vec![json!(now), json!(batch.id)],
        ).await?;
    }

    let swiped_after = swiped_before + 1;

    // If all cards swiped, mark batch completed
    if swiped_after >= batch.batch_size {
        db::exec(
            &d1,
            "UPDATE blitz_batches SET status = 'completed', completed_at = ?, updated_at = ? WHERE id = ?",
            vec![json!(now), json!(now), json!(batch.id)],
        ).await?;
    }

    // Check whether to trigger next batch
    let next_exists = db::first::<CountRow>(
        &d1,
        "SELECT COUNT(*) AS count FROM blitz_batches WHERE clone_id = ? AND batch_number = ?",
        vec![json!(batch.clone_id), json!(batch.batch_number + 1)],
    ).await?.map(|r| r.count).unwrap_or(0) > 0;

    let account = db::first::<AccountQuotaRow>(
        &d1,
        "SELECT plan, daily_generation_limit FROM accounts WHERE user_id = ?",
        vec![json!(auth.user_id)],
    ).await?;
    let (plan, custom_limit) = account
        .map(|a| (a.plan, a.daily_generation_limit))
        .unwrap_or(("free".to_string(), None));
    let limit = daily_generation_limit(&plan, custom_limit);
    let today_used = db::first::<CountRow>(
        &d1,
        "SELECT COUNT(*) AS count FROM generation_jobs WHERE user_id = ? AND DATE(queued_at) = DATE('now')",
        vec![json!(auth.user_id)],
    ).await?.map(|r| r.count).unwrap_or(0);
    let quota = generations_remaining(limit, today_used);

    let batch_size = parse_batch_size(
        ctx.var("BLITZ_BATCH_SIZE").ok().as_ref().map(|v| v.to_string()).as_deref()
    );
    let next_triggered = should_trigger_next_batch(swiped_before, next_exists, quota.remaining, batch_size);

    if next_triggered {
        // Create next batch record (generation will be handled by queue)
        let next_batch_id = format!("batch_{}", Uuid::new_v4().simple());
        let next_batch_number = batch.batch_number + 1;

        // Build taste snapshot from all completed batches for this clone
        let completed_swipes = db::all::<SwipeRow>(
            &d1,
            r#"SELECT bs.direction, bs.aesthetic_tags_json, bs.visual_reference_id
               FROM blitz_swipes bs
               JOIN blitz_batches bb ON bb.id = bs.batch_id
               WHERE bb.clone_id = ? AND bb.status = 'completed'"#,
            vec![json!(batch.clone_id)],
        ).await?;

        let swipe_records: Vec<SwipeRecord> = completed_swipes.iter().map(|s| {
            let tags: Vec<String> = serde_json::from_str(&s.aesthetic_tags_json).unwrap_or_default();
            SwipeRecord {
                direction: s.direction.clone(),
                aesthetic_tags: tags,
                visual_reference_id: s.visual_reference_id.clone(),
            }
        }).collect();

        let taste = build_taste_snapshot(&swipe_records);
        let taste_json = serde_json::to_string(&taste).unwrap_or_else(|_| "{}".to_string());

        db::exec(
            &d1,
            r#"INSERT OR IGNORE INTO blitz_batches
               (id, user_id, clone_id, batch_number, status, batch_size, taste_snapshot_json, visual_ref_ids_json, created_at, updated_at)
               VALUES (?, ?, ?, ?, 'pending', ?, ?, '[]', ?, ?)"#,
            vec![
                json!(next_batch_id),
                json!(auth.user_id),
                json!(batch.clone_id),
                json!(next_batch_number),
                json!(batch_size),
                json!(taste_json),
                json!(now),
                json!(now),
            ],
        ).await?;

        // Enqueue generation (log for now, actual queue dispatch added when generation pipeline is wired)
        web_sys::console::log_1(
            &format!("enqueue blitz batch generation batch_id={next_batch_id} clone={} batch_number={next_batch_number}", batch.clone_id).into(),
        );
    }

    Response::from_json(&SwipeResponse {
        swipe_id,
        batch_progress: BatchProgress {
            swiped: swiped_after,
            total: batch.batch_size,
        },
        next_batch_triggered: next_triggered,
    })
}

async fn load_batch_cards(d1: &worker::D1Database, batch_id: &str) -> WorkerResult<Vec<BlitzCard>> {
    let rows = db::all::<CardRow>(
        d1,
        r#"SELECT go.id AS output_id, go.media_asset_id AS media_id,
                  gj.input_visual_reference_id AS visual_reference_id,
                  vr.aesthetic_tags_json
           FROM generation_outputs go
           JOIN generation_jobs gj ON gj.id = go.job_id AND gj.blitz_batch_id = ?
           LEFT JOIN visual_references vr ON vr.id = gj.input_visual_reference_id
           ORDER BY go.output_index"#,
        vec![json!(batch_id)],
    ).await?;

    Ok(rows.into_iter().map(|r| {
        let tags: Vec<String> = r.aesthetic_tags_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        BlitzCard {
            output_id: r.output_id,
            media_id: r.media_id.clone(),
            media_url: r.media_id.map(|id| format!("/api/media/{}", id)),
            visual_reference_id: r.visual_reference_id,
            aesthetic_tags: tags,
        }
    }).collect())
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}
```

- [ ] **Step 2: Add `blitz` to `workers/product/src/routes/mod.rs`**

Add this line:

```rust
pub mod blitz;
```

- [ ] **Step 3: Register blitz routes in `workers/product/src/http/router.rs`**

Add these two routes inside the `Router::new()` chain, before the `.run(req, env)` call:

```rust
        .get_async("/api/blitz/current", crate::routes::blitz::blitz_current)
        .post_async("/api/blitz/swipe", crate::routes::blitz::blitz_swipe)
```

- [ ] **Step 4: Verify compilation**

Run: `cd workers/product && cargo check --target wasm32-unknown-unknown 2>&1 | tail -5`

Expected: compilation succeeds.

- [ ] **Step 5: Run all tests**

Run: `cd workers/product && cargo test 2>&1 | tail -5`

Expected: all tests PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/routes/blitz.rs workers/product/src/routes/mod.rs workers/product/src/http/router.rs
git commit -m "feat: add blitz current and swipe API routes"
```

---

## Task 10: Update Onboarding Bubbles To Require Clone ID

**Files:**
- Modify: `workers/product/src/routes/onboarding.rs`

- [ ] **Step 1: Update `SaveBubblesRequest` validation in `save_bubbles`**

In `workers/product/src/routes/onboarding.rs`, find the `save_bubbles` function. After parsing the `SaveBubblesRequest`, add a clone_id requirement check. Replace the `clone_id` handling with:

```rust
    let clone_id = match input.clone_id {
        Some(ref id) if !id.trim().is_empty() => id.clone(),
        _ => {
            return ApiError::bad_request(
                "missing_clone_id",
                "cloneId is required. Bubbles are saved per clone.",
            )
            .to_response()
        }
    };
```

This enforces the spec requirement that bubbles are per-clone.

- [ ] **Step 2: Update the bubble INSERT to use `clone_id`**

In the same function, find the SQL that inserts into `inspiration_bubbles`. Make sure the `clone_id` column is populated with the required value from the request instead of `NULL`. The existing code should already reference `clone_id` from `input.clone_id` -- verify the SQL INSERT uses the `clone_id` local variable from step 1.

- [ ] **Step 3: Verify compilation**

Run: `cd workers/product && cargo check --target wasm32-unknown-unknown 2>&1 | tail -5`

Expected: compilation succeeds.

- [ ] **Step 4: Run all tests**

Run: `cd workers/product && cargo test 2>&1 | tail -5`

Expected: all tests PASS.

- [ ] **Step 5: Commit**

```bash
git add workers/product/src/routes/onboarding.rs
git commit -m "feat: require cloneId for bubble save (bubbles are per-clone)"
```

---

## Task 11: Wrangler Config Updates

**Files:**
- Modify: `workers/product/wrangler.product.jsonc`

- [ ] **Step 1: Add new env vars to `vars` section**

Add these to the `vars` object in `workers/product/wrangler.product.jsonc`:

```jsonc
    "BLITZ_BATCH_SIZE": "5",
    "FREE_DAILY_GENERATION_LIMIT": "10",
    "PRO_DAILY_GENERATION_LIMIT": "50",
    "SCRAPE_DELAY_MS": "1000",
    "SCRAPE_MAX_POSTS_PER_QUERY": "10",
    "ENGAGEMENT_THRESHOLD": "10000",
    "HUMAN_PRESENCE_MIN_CONFIDENCE": "0.7",
    "OPENROUTER_MODEL": "moonshotai/kimi-k2.6",
    "NICHE_RESEARCH_MAX_QUERIES": "30",
    "NICHE_RESEARCH_MAX_KNOWLEDGE": "60"
```

- [ ] **Step 2: Verify the config is valid JSON**

Run:

```bash
node -e "const fs = require('fs'); const c = fs.readFileSync('workers/product/wrangler.product.jsonc','utf8').replace(/\/\/.*/g,''); JSON.parse(c); console.log('valid');"
```

Expected: `valid`

- [ ] **Step 3: Commit**

```bash
git add workers/product/wrangler.product.jsonc
git commit -m "feat: add blitz and niche research config vars"
```

---

## Task 12: Entitlements Update With Generation Limits

**Files:**
- Modify: `workers/product/src/domain/entitlements.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write the failing tests**

Add to `workers/product/tests/domain_tests.rs`:

```rust
#[test]
fn free_entitlements_include_generation_limit() {
    let e = Entitlements::free();
    assert_eq!(e.daily_generation_limit, 10);
}

#[test]
fn paid_entitlements_include_generation_limit() {
    let e = Entitlements::paid();
    assert_eq!(e.daily_generation_limit, 50);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd workers/product && cargo test --test domain_tests 2>&1 | tail -5`

Expected: FAIL -- `daily_generation_limit` field not found.

- [ ] **Step 3: Add `daily_generation_limit` to `Entitlements`**

Update `workers/product/src/domain/entitlements.rs`:

```rust
use crate::domain::quota::{FREE_DAILY_GENERATION_LIMIT, PRO_DAILY_GENERATION_LIMIT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entitlements {
    pub max_active_clones: u32,
    pub daily_generation_limit: u32,
}

pub const FREE_MAX_ACTIVE_CLONES: u32 = 1;
pub const PAID_MAX_ACTIVE_CLONES: u32 = 5;

impl Entitlements {
    pub const fn free() -> Self {
        Self {
            max_active_clones: FREE_MAX_ACTIVE_CLONES,
            daily_generation_limit: FREE_DAILY_GENERATION_LIMIT,
        }
    }

    pub const fn paid() -> Self {
        Self {
            max_active_clones: PAID_MAX_ACTIVE_CLONES,
            daily_generation_limit: PRO_DAILY_GENERATION_LIMIT,
        }
    }
}

pub fn can_create_clone(
    entitlements: &Entitlements,
    active_clone_count: u32,
) -> Result<(), &'static str> {
    if active_clone_count >= entitlements.max_active_clones {
        Err("clone_limit_reached")
    } else {
        Ok(())
    }
}
```

- [ ] **Step 4: Fix compilation errors from `Entitlements` struct change**

The `manual_upload` route in `workers/product/src/routes/clones.rs` constructs `Entitlements` with only `max_active_clones`. Update that construction to include `daily_generation_limit`:

```rust
    let entitlements = Entitlements {
        max_active_clones: verified.max_active_clones,
        daily_generation_limit: crate::domain::quota::daily_generation_limit(&verified.plan, None),
    };
```

Add this import at the top of `workers/product/src/routes/clones.rs` if not present:

```rust
use crate::domain::quota;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd workers/product && cargo test 2>&1 | tail -5`

Expected: all tests PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/domain/entitlements.rs workers/product/src/routes/clones.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add daily generation limit to entitlements"
```

---

## Parallelization Plan

Run tasks in this order:

1. Task 1 (schema).
2. In parallel: Task 2, Task 3, Task 4, Task 5, Task 6.
3. Task 7 after Task 6 (needs `NicheResearchMessage` re-export).
4. Task 8 after Tasks 2, 3.
5. Task 9 after Tasks 2, 3, 8.
6. Task 10 (independent, can run any time after Task 1).
7. Task 11 (independent config, can run any time).
8. Task 12 after Task 2 (needs `quota` module).

Disjoint write sets for safe parallel workers:

- Task 2: `domain/quota.rs`, `domain/mod.rs`
- Task 3: `services/taste.rs`
- Task 4: `services/openrouter.rs`
- Task 5: `services/scrape_creators.rs`
- Task 6: `ai/tasks.rs`

All share `workers/product/tests/domain_tests.rs` and `workers/product/src/services/mod.rs` -- merge through parent after parallel tasks finish.

## Final Verification Checklist

- [ ] `cd workers/product && cargo test` -- all tests PASS.
- [ ] `cd workers/product && cargo check --target wasm32-unknown-unknown` -- compiles.
- [ ] `npm run db:migrate:local` -- migration 1002 applies.
- [ ] Blitz routes registered: `GET /api/blitz/current`, `POST /api/blitz/swipe`.
- [ ] `POST /api/onboarding/bubbles` rejects missing `cloneId`.
- [ ] Wrangler config has all new env vars.
- [ ] `rg "BlitzTasteInfluence" workers/product/src` -- found in `ai/tasks.rs`.
- [ ] `rg "NicheResearchMessage" workers/product/src` -- has all 4 variants.
