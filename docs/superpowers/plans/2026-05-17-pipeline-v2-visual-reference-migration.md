# Pipeline V2 Visual Reference Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current broken moodboard reference pipeline with the working `pipeline-v2` discovery semantics, then gate every stored reference through Kimi review, Seedream text cleanup, and clone compatibility validation.

**Architecture:** Keep the Product Worker, D1, R2, and queue architecture. Reels Search is used only to discover Instagram owner handles; static posts and carousel images become candidates; Workers AI Kimi K2.6 performs visual review and clone compatibility; Seedream 5.0 Lite removes only visible image text before any image is stored. `visual_references` remains the generation-ready table and contains only cleaned, clone-compatible, R2-backed references.

**Tech Stack:** Cloudflare Workers, `workers-rs` 0.6, Rust/Wasm, D1, R2 `MEDIA`, Cloudflare Queues, Workers AI Kimi K2.6 (`@cf/moonshotai/kimi-k2.6`), ScrapeCreators Instagram APIs, Higgsfield MCP, Seedream 5.0 Lite, Rust unit tests.

---

## Scope Rules

- Do not run or import `social-page/pipeline-v2` as a sidecar. Port behavior into `workers/product`.
- Do not add OpenRouter.
- Do not store original Instagram image bytes in R2 or `media_assets`.
- Keep captions, handles, and source URLs only as candidate audit metadata.
- Use the exact cleanup prompt:

```text
Remove only the visible text from this image. Keep every non-text part of the image exactly the same.
```

- Clone compatibility v1 checks body proportions, hair length, and facial hair only. The compatibility prompt and tests must not mention gender.
- There are no production users. It is acceptable to edit the destructive D1 rebuild migration `config/d1/migrations/1007_visual_reference_pipeline.sql`.

## File Structure

- Modify `config/d1/migrations/1007_visual_reference_pipeline.sql`
  - Add cleanup and compatibility audit columns to `visual_reference_candidates`.
  - Add config rows for Reels Search, dimensions, cleanup retries, compatibility retries, and clone reference limits.

- Modify `workers/product/Cargo.toml`
  - Add `base64 = "0.22"` for Kimi data URLs built from private R2 clone reference assets.

- Modify `workers/product/wrangler.product.jsonc`
  - Add cleanup provider vars: `HIGGSFIELD_MCP_CLEANUP_TOOL` and `HIGGSFIELD_MCP_CLEANUP_MODEL`.

- Modify `workers/product/src/providers/mod.rs`
  - Export the new Seedream cleanup provider module.

- Create `workers/product/src/providers/seedream.rs`
  - Own the exact cleanup prompt, Higgsfield MCP cleanup request builder, response URL extraction, and cleanup error mapping.

- Modify `workers/product/src/providers/instagram_references.rs`
  - Add Reels Search URL builder.
  - Add owner-handle extraction from Reels Search payloads.
  - Add dimension gate helper.
  - Keep static post and carousel normalization as the only final reference source.

- Modify `workers/product/src/ai/workers_ai.rs`
  - Add multi-image Kimi request support.
  - Add clone compatibility prompt builder.
  - Add `CloneCompatibilityReview` decoding type.

- Modify `workers/product/src/domain/visual_reference.rs`
  - Add compatibility acceptance helper.
  - Update candidate ranking scores for `reels_owner` and `learned_related`.

- Modify `workers/product/src/services/visual_reference_cache.rs`
  - Rename parameters and metadata around cleaned images.
  - Ensure `remote_url` records the cleaned Seedream output URL.

- Modify `workers/product/src/queues/niche_research.rs`
  - Add queue variants: `DiscoverInstagramHandles`, `CleanupApprovedReference`, `ValidateCloneCompatibility`.
  - Replace configured-handle kickoff with moodboard search-term discovery.
  - Remove related-profile expansion from the active path.
  - Change accepted review flow to cleanup, compatibility, then cache.
  - Update finalization drains and status details.

- Modify `workers/product/tests/domain_tests.rs`
  - Add pure tests for schema, provider helpers, prompts, cleanup request shape, compatibility acceptance, and cache contract.

---

## Task 1: Schema And Config Contract

**Files:**
- Modify: `config/d1/migrations/1007_visual_reference_pipeline.sql`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write the failing schema test**

Add these assertions to `visual_reference_pipeline_schema_has_required_columns_and_config()` in `workers/product/tests/domain_tests.rs`:

```rust
    assert!(migration.contains("cleanup_json TEXT NOT NULL DEFAULT '{}'"));
    assert!(migration.contains("cleaned_image_url TEXT"));
    assert!(migration.contains("compatibility_json TEXT NOT NULL DEFAULT '{}'"));
    assert!(migration.contains("instagram_search_terms_per_moodboard"));
    assert!(migration.contains("instagram_reels_pages_per_term"));
    assert!(migration.contains("instagram_max_handles_per_moodboard"));
    assert!(migration.contains("instagram_min_image_width"));
    assert!(migration.contains("instagram_min_image_height"));
    assert!(migration.contains("visual_reference_cleanup_retry_limit"));
    assert!(migration.contains("visual_reference_compatibility_retry_limit"));
    assert!(migration.contains("clone_compatibility_reference_limit"));
```

- [ ] **Step 2: Run the schema test and verify it fails**

Run:

```bash
npm run product:test -- visual_reference_pipeline_schema_has_required_columns_and_config
```

Expected: FAIL because `cleanup_json`, `cleaned_image_url`, `compatibility_json`, and the new config keys are missing from the migration.

- [ ] **Step 3: Add candidate audit columns**

In `config/d1/migrations/1007_visual_reference_pipeline.sql`, update `CREATE TABLE IF NOT EXISTS visual_reference_candidates` so the block around `review_json` is:

```sql
  review_status TEXT NOT NULL DEFAULT 'unreviewed',
  review_json TEXT NOT NULL DEFAULT '{}',
  cleanup_json TEXT NOT NULL DEFAULT '{}',
  cleaned_image_url TEXT,
  compatibility_json TEXT NOT NULL DEFAULT '{}',
  rejection_reason TEXT,
```

- [ ] **Step 4: Add pipeline config rows**

In the `INSERT INTO blitz_config` seed list, replace the final row:

```sql
  ('moodboard_instagram_handles_json', '{}', '2026-05-14T00:00:00.000Z');
```

with:

```sql
  ('moodboard_instagram_handles_json', '{}', '2026-05-14T00:00:00.000Z'),
  ('instagram_search_terms_per_moodboard', '2', '2026-05-17T00:00:00.000Z'),
  ('instagram_reels_pages_per_term', '1', '2026-05-17T00:00:00.000Z'),
  ('instagram_max_handles_per_moodboard', '20', '2026-05-17T00:00:00.000Z'),
  ('instagram_min_image_width', '512', '2026-05-17T00:00:00.000Z'),
  ('instagram_min_image_height', '512', '2026-05-17T00:00:00.000Z'),
  ('visual_reference_cleanup_retry_limit', '3', '2026-05-17T00:00:00.000Z'),
  ('visual_reference_compatibility_retry_limit', '2', '2026-05-17T00:00:00.000Z'),
  ('clone_compatibility_reference_limit', '4', '2026-05-17T00:00:00.000Z');
```

- [ ] **Step 5: Run the schema test and verify it passes**

Run:

```bash
npm run product:test -- visual_reference_pipeline_schema_has_required_columns_and_config
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add config/d1/migrations/1007_visual_reference_pipeline.sql workers/product/tests/domain_tests.rs
git commit -m "feat: add visual reference cleanup schema"
```

---

## Task 2: Reels Owner Discovery Helpers

**Files:**
- Modify: `workers/product/src/providers/instagram_references.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write failing provider helper tests**

Extend the `mirai_product_worker::instagram_references` import in `workers/product/tests/domain_tests.rs` with:

```rust
    build_instagram_reels_search_url, extract_instagram_reels_owner_handles,
    instagram_candidate_meets_min_dimensions,
```

Add these tests near the existing Instagram normalization tests:

```rust
#[test]
fn reels_search_url_uses_query_and_optional_page() {
    assert_eq!(
        build_instagram_reels_search_url("https://api.scrapecreators.com/", "flash fashion", None)
            .unwrap(),
        "https://api.scrapecreators.com/v2/instagram/reels/search?query=flash%20fashion&trim=true"
    );
    assert_eq!(
        build_instagram_reels_search_url("https://api.scrapecreators.com", "flash fashion", Some(2))
            .unwrap(),
        "https://api.scrapecreators.com/v2/instagram/reels/search?query=flash%20fashion&page=2&trim=true"
    );
}

#[test]
fn reels_search_extracts_owner_handles_only() {
    let raw = json!({
        "items": [
            { "user": { "username": "CreatorA" }, "thumbnail_url": "https://cdn.example/reel.jpg" },
            { "owner": { "username": "@CreatorB" }, "display_url": "https://cdn.example/reel2.jpg" },
            { "username": "creator_c" },
            { "user": { "username": "CreatorA" } }
        ]
    });

    assert_eq!(
        extract_instagram_reels_owner_handles(&raw, 10),
        vec!["CreatorA".to_string(), "CreatorB".to_string(), "creator_c".to_string()]
    );
}

#[test]
fn instagram_candidate_dimension_gate_rejects_small_known_dimensions() {
    let mut candidate = instagram_candidate_fixture();
    candidate.image_width = Some(511);
    candidate.image_height = Some(900);
    assert!(!instagram_candidate_meets_min_dimensions(&candidate, 512, 512));

    candidate.image_width = Some(800);
    candidate.image_height = Some(512);
    assert!(instagram_candidate_meets_min_dimensions(&candidate, 512, 512));

    candidate.image_width = None;
    candidate.image_height = None;
    assert!(instagram_candidate_meets_min_dimensions(&candidate, 512, 512));
}
```

Add this helper near other test fixtures:

```rust
fn instagram_candidate_fixture() -> mirai_product_worker::instagram_references::InstagramImageCandidate {
    mirai_product_worker::instagram_references::InstagramImageCandidate {
        platform: "instagram".to_string(),
        source_handle: "creator".to_string(),
        source_profile_id: Some("profile_1".to_string()),
        source_post_id: "post_1".to_string(),
        source_post_code: "ABC123".to_string(),
        source_image_index: 0,
        source_url: Some("https://www.instagram.com/p/ABC123/".to_string()),
        source_published_at: Some("2026-01-01T00:00:00.000Z".to_string()),
        source_caption: Some("street style".to_string()),
        media_type: 1,
        image_url: "https://cdn.example.com/image.jpg".to_string(),
        image_width: Some(1080),
        image_height: Some(1350),
        like_count: Some(10),
        comment_count: Some(2),
        play_count: None,
        moodboard_id: "moodboard_1".to_string(),
        moodboard_slug: "flash-editorial".to_string(),
        discovered_via: "reels_owner".to_string(),
        raw_json: json!({}),
    }
}
```

- [ ] **Step 2: Run the helper tests and verify they fail**

Run:

```bash
npm run product:test -- reels_search_url_uses_query_and_optional_page reels_search_extracts_owner_handles_only instagram_candidate_dimension_gate_rejects_small_known_dimensions
```

Expected: FAIL because the helper functions do not exist.

- [ ] **Step 3: Add Reels Search helper functions**

Add these functions to `workers/product/src/providers/instagram_references.rs` near the existing URL builders:

```rust
pub fn build_instagram_reels_search_url(
    base_url: &str,
    query: &str,
    page: Option<u32>,
) -> Result<String, &'static str> {
    let query = query.trim();
    if query.is_empty() {
        return Err("missing_instagram_reels_search_query");
    }
    let mut url = format!(
        "{}/v2/instagram/reels/search?query={}",
        base_url.trim_end_matches('/'),
        url_encode(query)
    );
    if let Some(page) = page.filter(|page| *page > 1) {
        url.push_str("&page=");
        url.push_str(&page.to_string());
    }
    url.push_str("&trim=true");
    Ok(url)
}

pub fn extract_instagram_reels_owner_handles(raw: &Value, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    instagram_reels_items(raw)
        .into_iter()
        .filter_map(instagram_reel_owner_handle)
        .filter_map(|handle| clean_handle(&handle))
        .filter(|handle| seen.insert(handle.to_ascii_lowercase()))
        .take(limit)
        .collect()
}

fn instagram_reels_items(raw: &Value) -> Vec<&Value> {
    array_at(raw, &["items"])
        .or_else(|| array_at(raw, &["reels"]))
        .or_else(|| array_at(raw, &["data"]))
        .into_iter()
        .flatten()
        .collect()
}

fn instagram_reel_owner_handle(reel: &Value) -> Option<String> {
    text_at(reel, &["user", "username"])
        .or_else(|| text_at(reel, &["owner", "username"]))
        .or_else(|| text_at(reel, &["username"]))
}

pub fn instagram_candidate_meets_min_dimensions(
    candidate: &InstagramImageCandidate,
    min_width: u32,
    min_height: u32,
) -> bool {
    candidate
        .image_width
        .map(|width| width >= min_width)
        .unwrap_or(true)
        && candidate
            .image_height
            .map(|height| height >= min_height)
            .unwrap_or(true)
}
```

- [ ] **Step 4: Run the helper tests and verify they pass**

Run:

```bash
npm run product:test -- reels_search_url_uses_query_and_optional_page reels_search_extracts_owner_handles_only instagram_candidate_dimension_gate_rejects_small_known_dimensions
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add workers/product/src/providers/instagram_references.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add instagram reels owner discovery helpers"
```

---

## Task 3: Queue Message Contract

**Files:**
- Modify: `workers/product/src/queues/niche_research.rs`

- [ ] **Step 1: Write failing queue contract tests**

Inside the existing `#[cfg(test)] mod tests` in `workers/product/src/queues/niche_research.rs`, add:

```rust
    #[test]
    fn pipeline_v2_messages_serialize_as_queue_contract() {
        let discover = NicheResearchMessage::DiscoverInstagramHandles {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            run_id: Some("run_1".to_string()),
            moodboard_id: "moodboard_1".to_string(),
            moodboard_slug: "flash-editorial".to_string(),
            search_term: "flash fashion".to_string(),
            page: 1,
        };
        assert_eq!(
            serde_json::to_value(&discover).unwrap(),
            json!({
                "type": "discover_instagram_handles",
                "userId": "user_1",
                "cloneId": "clone_1",
                "runId": "run_1",
                "moodboardId": "moodboard_1",
                "moodboardSlug": "flash-editorial",
                "searchTerm": "flash fashion",
                "page": 1
            })
        );

        let cleanup = NicheResearchMessage::CleanupApprovedReference {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            run_id: Some("run_1".to_string()),
            candidate_id: "candidate_1".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&cleanup).unwrap(),
            json!({
                "type": "cleanup_approved_reference",
                "userId": "user_1",
                "cloneId": "clone_1",
                "runId": "run_1",
                "candidateId": "candidate_1"
            })
        );

        let compatibility = NicheResearchMessage::ValidateCloneCompatibility {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            run_id: Some("run_1".to_string()),
            candidate_id: "candidate_1".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&compatibility).unwrap(),
            json!({
                "type": "validate_clone_compatibility",
                "userId": "user_1",
                "cloneId": "clone_1",
                "runId": "run_1",
                "candidateId": "candidate_1"
            })
        );
    }
```

- [ ] **Step 2: Run the queue contract test and verify it fails**

Run:

```bash
npm run product:test -- pipeline_v2_messages_serialize_as_queue_contract
```

Expected: FAIL because the new message variants do not exist.

- [ ] **Step 3: Add queue variants**

In `NicheResearchMessage`, add:

```rust
    DiscoverInstagramHandles {
        user_id: String,
        clone_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        moodboard_id: String,
        moodboard_slug: String,
        search_term: String,
        page: u32,
    },
```

and after `ReviewVisualCandidates`:

```rust
    CleanupApprovedReference {
        user_id: String,
        clone_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        candidate_id: String,
    },
    ValidateCloneCompatibility {
        user_id: String,
        clone_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        candidate_id: String,
    },
```

- [ ] **Step 4: Add failure context branches**

In `message_failure_context`, add branches with these `message_type` values:

```rust
"discover_instagram_handles"
"cleanup_approved_reference"
"validate_clone_compatibility"
```

Each branch should clone `user_id`, `clone_id`, and `run_id` just like `ReviewVisualCandidates`.

- [ ] **Step 5: Add handler branches with stub functions**

In `handle_message`, route the new variants to functions with these signatures:

```rust
async fn discover_instagram_handles_message(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    moodboard_id: &str,
    moodboard_slug: &str,
    search_term: &str,
    page: u32,
) -> WorkerResult<()> {
    let Some(_run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    Err(Error::RustError("discover_instagram_handles_not_implemented".to_string()))
}

async fn cleanup_approved_reference_message(
    db: &D1Database,
    _env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    _candidate_id: &str,
) -> WorkerResult<()> {
    let Some(_run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    Err(Error::RustError("cleanup_approved_reference_not_implemented".to_string()))
}

async fn validate_clone_compatibility_message(
    db: &D1Database,
    _env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    _candidate_id: &str,
) -> WorkerResult<()> {
    let Some(_run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    Err(Error::RustError("validate_clone_compatibility_not_implemented".to_string()))
}
```

These stubs are deliberately replaced in later tasks before full test runs.

- [ ] **Step 6: Run the queue contract test and verify it passes**

Run:

```bash
npm run product:test -- pipeline_v2_messages_serialize_as_queue_contract
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add workers/product/src/queues/niche_research.rs
git commit -m "feat: add pipeline v2 queue messages"
```

---

## Task 4: Moodboard Search-Term Discovery Flow

**Files:**
- Modify: `workers/product/src/queues/niche_research.rs`
- Modify: `workers/product/src/providers/instagram_references.rs`

- [ ] **Step 1: Write failing source-level tests for kickoff behavior**

Update `onboarding_research_kickoff_uses_visual_reference_pipeline_contract()` in `workers/product/src/queues/niche_research.rs`:

```rust
        assert!(enqueue_research.contains("NicheResearchMessage::DiscoverInstagramHandles"));
        assert!(enqueue_research.contains("selected_search_terms("));
        assert!(!enqueue_research.contains("moodboard_handle_map("));
        assert!(!enqueue_research.contains("configured_handle"));
```

Add:

```rust
    #[test]
    fn search_term_selection_is_trimmed_deduped_and_bounded() {
        let terms = selected_search_terms(
            r#"[" flash fashion ", "Flash Fashion", "", "street creator"]"#,
            "flash-editorial",
            "Flash Editorial",
            2,
        );

        assert_eq!(terms, vec!["flash fashion".to_string(), "street creator".to_string()]);
    }
```

- [ ] **Step 2: Run the tests and verify they fail**

Run:

```bash
npm run product:test -- onboarding_research_kickoff_uses_visual_reference_pipeline_contract search_term_selection_is_trimmed_deduped_and_bounded
```

Expected: FAIL because kickoff still queues configured profiles and `selected_search_terms` does not exist.

- [ ] **Step 3: Add search-term selection helper**

Add this helper near `moodboard_handle_map`:

```rust
fn selected_search_terms(
    search_queries_json: &str,
    moodboard_slug: &str,
    moodboard_title: &str,
    limit: u32,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut terms = serde_json::from_str::<Vec<String>>(search_queries_json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|term| {
            let trimmed = term.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .filter(|term| seen.insert(term.to_ascii_lowercase()))
        .collect::<Vec<_>>();

    if terms.is_empty() {
        for fallback in [moodboard_title, moodboard_slug] {
            let trimmed = fallback.trim();
            if !trimmed.is_empty() && seen.insert(trimmed.to_ascii_lowercase()) {
                terms.push(trimmed.to_string());
            }
        }
    }

    terms
        .into_iter()
        .take(limit.max(1) as usize)
        .collect::<Vec<_>>()
}
```

- [ ] **Step 4: Replace kickoff profile queuing with discovery messages**

In `enqueue_moodboard_reference_research`, remove the configured-handle path and use:

```rust
    let search_terms_per_moodboard =
        config_u32(&config, "instagram_search_terms_per_moodboard", 2).max(1);
    let reels_pages_per_term = config_u32(&config, "instagram_reels_pages_per_term", 1).max(1);

    let mut queued = 0usize;
    for moodboard in moodboards {
        let terms = selected_search_terms(
            &moodboard.search_queries_json,
            &moodboard.slug,
            &moodboard.title,
            search_terms_per_moodboard,
        );

        for term in terms {
            for page in 1..=reels_pages_per_term {
                env.queue("NICHE_RESEARCH_QUEUE")?
                    .send(NicheResearchMessage::DiscoverInstagramHandles {
                        user_id: user_id.to_string(),
                        clone_id: clone_id.to_string(),
                        run_id: Some(run_id.to_string()),
                        moodboard_id: moodboard.id.clone(),
                        moodboard_slug: moodboard.slug.clone(),
                        search_term: term.clone(),
                        page,
                    })
                    .await?;
                queued += 1;
            }
        }
    }
```

Keep the existing selected moodboard validation and `queued == 0` status write, but change the detail string to:

```rust
"no moodboard search terms available for selected moodboards"
```

- [ ] **Step 5: Implement `discover_instagram_handles_message`**

Replace the stub with:

```rust
async fn discover_instagram_handles_message(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    moodboard_id: &str,
    moodboard_slug: &str,
    search_term: &str,
    page: u32,
) -> WorkerResult<()> {
    let Some(run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    let status_write = set_clone_research_status_with_run_result(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Scraping),
        &format!("discovering instagram handles moodboard={moodboard_slug} term={search_term} page={page}"),
        Some(&run_id),
        Some(&run_id),
    )
    .await?;
    if !status_write_allows_side_effects(status_write) {
        return Ok(());
    }

    let config = load_config_map(db).await?;
    let base_url = env_var(env, "SCRAPECREATORS_BASE_URL", "scrapecreators_base_url_missing")?;
    let api_key = env_var(env, "SCRAPECREATORS_API_KEY", "scrapecreators_api_key_missing")?;
    let request_url = build_instagram_reels_search_url(&base_url, search_term, Some(page))
        .map_err(|error| Error::RustError(error.to_string()))?;
    let now = now_iso_string();
    let params = instagram_reels_search_source_params(
        user_id,
        clone_id,
        &run_id,
        moodboard_id,
        moodboard_slug,
        search_term,
        page,
    );
    let source_id = upsert_discovery_source(db, &request_url, &params, &now).await?;
    let raw = match fetch_scrapecreators_json(&request_url, &api_key).await {
        Ok(raw) => raw,
        Err(error) => {
            return handle_scrapecreators_source_failure(
                db,
                env,
                user_id,
                clone_id,
                &run_id,
                &source_id,
                &params,
                &error,
                &now,
                "instagram_reels_search_source_failed",
            )
            .await;
        }
    };

    mark_discovery_source_fresh(db, &source_id, &params, &now).await?;
    let max_handles = config_u32(&config, "instagram_max_handles_per_moodboard", 20).max(1);
    let max_profiles_per_run = config_u32(&config, "instagram_max_profiles_per_run", 20) as usize;
    let mut handles = extract_instagram_reels_owner_handles(&raw, max_handles as usize)
        .into_iter()
        .map(|handle| HandleSeed {
            handle,
            discovered_via: "reels_owner".to_string(),
        })
        .collect::<Vec<_>>();

    if page == 1 {
        handles.extend(
            load_accepted_handles(db, clone_id, moodboard_id, max_handles)
                .await?
                .into_iter()
                .map(|handle| HandleSeed {
                    handle,
                    discovered_via: "learned_related".to_string(),
                }),
        );
    }

    let mut reserved = count_instagram_profile_sources_for_run(db, clone_id, &run_id).await?;
    for seed in dedupe_handle_seeds(handles).into_iter().take(max_handles as usize) {
        if reserved >= max_profiles_per_run {
            break;
        }
        let reserved_profile = reserve_instagram_profile_source(
            db,
            &base_url,
            user_id,
            clone_id,
            &run_id,
            moodboard_id,
            moodboard_slug,
            &seed.handle,
            &seed.discovered_via,
            0,
            max_profiles_per_run,
            &now,
        )
        .await?;
        if !reserved_profile {
            reserved = count_instagram_profile_sources_for_run(db, clone_id, &run_id).await?;
            continue;
        }
        env.queue("NICHE_RESEARCH_QUEUE")?
            .send(NicheResearchMessage::FetchInstagramProfile {
                user_id: user_id.to_string(),
                clone_id: clone_id.to_string(),
                run_id: Some(run_id.clone()),
                moodboard_id: moodboard_id.to_string(),
                moodboard_slug: moodboard_slug.to_string(),
                handle: seed.handle,
                discovered_via: seed.discovered_via,
                related_depth: 0,
            })
            .await?;
        reserved += 1;
    }

    enqueue_delayed_finalize_reference_pool(
        env,
        user_id,
        clone_id,
        &run_id,
        "instagram_handle_discovery_completed",
    )
    .await
}
```

Add imports at the top of `niche_research.rs`:

```rust
    build_instagram_reels_search_url, extract_instagram_reels_owner_handles,
```

- [ ] **Step 6: Add Reels Search source params**

Add:

```rust
fn instagram_reels_search_source_params(
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    search_term: &str,
    page: u32,
) -> Value {
    json!({
        "cloneId": clone_id,
        "userId": user_id,
        "runId": run_id,
        "platform": "instagram",
        "moodboardId": moodboard_id,
        "moodboardSlug": moodboard_slug,
        "searchTerm": search_term.trim(),
        "page": page,
        "requestType": "instagram_reels_search",
    })
}
```

- [ ] **Step 7: Remove active related-profile expansion**

In `fetch_instagram_profile_message`, delete the `if related_depth == 0 { ... }` block that enqueues `related_profile` handles. Keep the `related_depth` field in the message for backward-compatible deserialization, but do not use it to expand discovery in v1.

- [ ] **Step 8: Update finalize discovery drain**

In `finalize_pending_discovery_work_sql()`, add `'instagram_reels_search'` to the first `requestType IN (...)` list:

```sql
              IN ('instagram_reels_search', 'instagram_profile', 'instagram_user_posts', 'instagram_post_detail')
```

- [ ] **Step 9: Run the kickoff tests**

Run:

```bash
npm run product:test -- onboarding_research_kickoff_uses_visual_reference_pipeline_contract search_term_selection_is_trimmed_deduped_and_bounded
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add workers/product/src/queues/niche_research.rs
git commit -m "feat: discover instagram handles from moodboard searches"
```

---

## Task 5: Candidate Dimension Gate And Ranking

**Files:**
- Modify: `workers/product/src/queues/niche_research.rs`
- Modify: `workers/product/src/domain/visual_reference.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write failing ranking tests**

In `candidate_ranking_prefers_static_configured_recent_engaged_images`, change `discovered_via` values to the new pipeline names and expected order:

```rust
        ranking_candidate(
            "reels_static",
            "reels_owner",
            "warm-ambient",
            "handle_a",
            1,
            1_000,
            "2026-01-02T00:00:00.000Z",
        ),
        ranking_candidate(
            "learned_carousel",
            "learned_related",
            "flash-editorial",
            "handle_c",
            8,
            5_000,
            "2026-01-01T00:00:00.000Z",
        ),
```

Assert:

```rust
    assert_eq!(ranked[0].id, "reels_static");
    assert_eq!(ranked[1].id, "learned_carousel");
```

Add this source-level queue test to `niche_research.rs`:

```rust
    #[test]
    fn posts_message_applies_min_dimension_gate_before_insert() {
        let source = include_str!("niche_research.rs");
        let body = function_body(source, "async fn fetch_instagram_posts_message");

        assert!(body.contains("instagram_min_image_width"));
        assert!(body.contains("instagram_min_image_height"));
        assert!(body.contains("instagram_candidate_meets_min_dimensions"));
    }
```

- [ ] **Step 2: Run the tests and verify they fail**

Run:

```bash
npm run product:test -- candidate_ranking_prefers_static_configured_recent_engaged_images posts_message_applies_min_dimension_gate_before_insert
```

Expected: FAIL because ranking does not score `reels_owner`/`learned_related`, and the queue does not use the dimension gate.

- [ ] **Step 3: Update ranking discovered-via scores**

In `workers/product/src/domain/visual_reference.rs`, replace `discovered_via_score` with:

```rust
fn discovered_via_score(discovered_via: &str) -> u16 {
    match discovered_via.trim().to_ascii_lowercase().as_str() {
        "reels_owner" => 220,
        "learned_related" => 210,
        "accepted_handle" => 200,
        "configured_handle" => 150,
        "related_profile" => 50,
        _ => 0,
    }
}
```

- [ ] **Step 4: Apply dimension gate in posts and detail messages**

Add `instagram_candidate_meets_min_dimensions` to the `instagram_references` import in `niche_research.rs`.

In `fetch_instagram_posts_message`, after `images_per_post`, add:

```rust
    let min_width = config_u32(&config, "instagram_min_image_width", 512);
    let min_height = config_u32(&config, "instagram_min_image_height", 512);
```

Replace:

```rust
    for candidate in candidates.into_iter().take(candidate_cap) {
```

with:

```rust
    for candidate in candidates
        .into_iter()
        .filter(|candidate| instagram_candidate_meets_min_dimensions(candidate, min_width, min_height))
        .take(candidate_cap)
    {
```

Repeat the same `min_width`/`min_height` config load and filter loop in `fetch_instagram_post_detail_message`.

- [ ] **Step 5: Run the tests and verify they pass**

Run:

```bash
npm run product:test -- candidate_ranking_prefers_static_configured_recent_engaged_images posts_message_applies_min_dimension_gate_before_insert
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add workers/product/src/domain/visual_reference.rs workers/product/src/queues/niche_research.rs workers/product/tests/domain_tests.rs
git commit -m "feat: gate and rank pipeline v2 candidates"
```

---

## Task 6: Kimi Review Prompt Parity

**Files:**
- Modify: `workers/product/src/ai/workers_ai.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write failing prompt guardrail test updates**

In `visual_reference_review_prompt_contains_guardrail_and_caption_rules()`, add:

```rust
    assert!(prompt.contains("Do not reject solely because caption/source text includes"));
    assert!(prompt.contains("discount code"));
    assert!(prompt.contains("brand tag"));
    assert!(prompt.contains("photographer credit"));
    assert!(prompt.contains("Do not reject solely because the image uses"));
    assert!(prompt.contains("dark lighting"));
    assert!(prompt.contains("red gel lighting"));
    assert!(prompt.contains("stylized editorial processing"));
    assert!(prompt.contains("text-dominant"));
```

- [ ] **Step 2: Run the prompt test and verify it fails**

Run:

```bash
npm run product:test -- visual_reference_review_prompt_contains_guardrail_and_caption_rules
```

Expected: FAIL until the prompt includes the pipeline-v2 caption and stylized-image allowances.

- [ ] **Step 3: Update `visual_reference_review_prompt` guardrails**

In `workers/product/src/ai/workers_ai.rs`, add these bullets after the existing caption-untrusted line:

```text
- Only use caption/source text to reject synthetic source when it explicitly says the image is AI-generated, a render, a prompt showcase, a generated image showcase, or similar synthetic output.
- Do not infer synthetic source from poetic, slang, humorous, aesthetic, or persona captions.
- Do not reject solely because caption/source text includes a discount code, brand tag, photographer credit, creator promo, sponsored wording, product mention, or affiliate-style copy.
- Do not reject solely because the image uses dark lighting, red gel lighting, theatrical light, direct flash, high contrast, visible grain, compression, or stylized editorial processing when the person count and moodboard fit are still assessable.
```

Keep the hard rejection text for screenshots, app UI, tutorials, templates, and text-dominant graphics.

- [ ] **Step 4: Run the prompt test and verify it passes**

Run:

```bash
npm run product:test -- visual_reference_review_prompt_contains_guardrail_and_caption_rules
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add workers/product/src/ai/workers_ai.rs workers/product/tests/domain_tests.rs
git commit -m "feat: align visual review prompt with pipeline v2"
```

---

## Task 7: Seedream Cleanup Provider

**Files:**
- Create: `workers/product/src/providers/seedream.rs`
- Modify: `workers/product/src/providers/mod.rs`
- Modify: `workers/product/wrangler.product.jsonc`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write failing cleanup provider tests**

Add imports in `workers/product/tests/domain_tests.rs`:

```rust
use mirai_product_worker::providers::seedream::{
    cleanup_prompt, extract_seedream_cleaned_image_url, seedream_cleanup_arguments,
    SEEDREAM_CLEANUP_MODEL,
};
```

If `providers` is not public from `lib.rs`, export only the Seedream module through a public re-export:

```rust
pub use providers::seedream;
```

Add tests:

```rust
#[test]
fn seedream_cleanup_prompt_is_exact_text_only_instruction() {
    assert_eq!(
        cleanup_prompt(),
        "Remove only the visible text from this image. Keep every non-text part of the image exactly the same."
    );
    let lower = cleanup_prompt().to_ascii_lowercase();
    for forbidden in ["identity", "style", "clothing", "background", "generate", "face"] {
        assert!(!lower.contains(forbidden), "{forbidden} must not appear in cleanup prompt");
    }
}

#[test]
fn seedream_cleanup_arguments_use_lite_model_and_uploaded_reference() {
    let args = seedream_cleanup_arguments("uploaded_media_1");

    assert_eq!(args["params"]["model"], SEEDREAM_CLEANUP_MODEL);
    assert_eq!(args["params"]["prompt"], cleanup_prompt());
    assert_eq!(args["params"]["medias"][0]["role"], "image");
    assert_eq!(args["params"]["medias"][0]["value"], "uploaded_media_1");
}

#[test]
fn seedream_response_extracts_cleaned_image_url() {
    let wrapped = json!({
        "result": {
            "content": [{
                "text": "{\"result\":{\"images\":[{\"url\":\"https://cdn.example.com/cleaned.webp\"}],\"id\":\"job_1\"}}"
            }]
        }
    });

    assert_eq!(
        extract_seedream_cleaned_image_url(&wrapped).as_deref(),
        Some("https://cdn.example.com/cleaned.webp")
    );
}
```

- [ ] **Step 2: Run the cleanup tests and verify they fail**

Run:

```bash
npm run product:test -- seedream_cleanup_prompt_is_exact_text_only_instruction seedream_cleanup_arguments_use_lite_model_and_uploaded_reference seedream_response_extracts_cleaned_image_url
```

Expected: FAIL because the Seedream module does not exist.

- [ ] **Step 3: Add provider module exports**

In `workers/product/src/providers/mod.rs`, add:

```rust
pub mod seedream;
```

In `workers/product/src/lib.rs`, add:

```rust
pub use providers::seedream;
```

- [ ] **Step 4: Implement `providers/seedream.rs`**

Create `workers/product/src/providers/seedream.rs`:

```rust
use serde_json::{json, Value};

pub const SEEDREAM_CLEANUP_MODEL: &str = "seedream_5_lite";

pub fn cleanup_prompt() -> &'static str {
    "Remove only the visible text from this image. Keep every non-text part of the image exactly the same."
}

pub fn seedream_cleanup_arguments(uploaded_reference_value: &str) -> Value {
    json!({
        "params": {
            "model": SEEDREAM_CLEANUP_MODEL,
            "prompt": cleanup_prompt(),
            "medias": [{
                "value": uploaded_reference_value,
                "role": "image"
            }],
            "count": 1
        }
    })
}

pub fn seedream_cleanup_arguments_with_model(
    uploaded_reference_value: &str,
    model: &str,
) -> Value {
    let mut arguments = seedream_cleanup_arguments(uploaded_reference_value);
    if let Some(params) = arguments.get_mut("params").and_then(Value::as_object_mut) {
        params.insert("model".to_string(), json!(model.trim()));
    }
    arguments
}

pub fn extract_seedream_cleaned_image_url(raw_json: &Value) -> Option<String> {
    provider_payloads(raw_json)
        .iter()
        .find_map(cleaned_image_url_from_payload)
}

fn cleaned_image_url_from_payload(payload: &Value) -> Option<String> {
    for path in [
        "/result/url",
        "/result/image_url",
        "/result/imageUrl",
        "/result/output_url",
        "/result/outputUrl",
        "/result/images/0/url",
        "/result/images/0/image_url",
        "/result/images/0/imageUrl",
        "/url",
        "/image_url",
        "/imageUrl",
        "/output_url",
        "/outputUrl",
        "/images/0/url",
    ] {
        if let Some(url) = json_string_at(payload, path).filter(|url| url.starts_with("http")) {
            return Some(url);
        }
    }
    None
}

fn provider_payloads(raw_json: &Value) -> Vec<Value> {
    let mut payloads = vec![raw_json.clone()];
    collect_text_payloads(raw_json, &mut payloads);
    payloads
}

fn collect_text_payloads(value: &Value, payloads: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                    payloads.push(parsed);
                }
            }
            for child in map.values() {
                collect_text_payloads(child, payloads);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text_payloads(item, payloads);
            }
        }
        _ => {}
    }
}

fn json_string_at(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
```

- [ ] **Step 5: Add cleanup vars**

In `workers/product/wrangler.product.jsonc`, add to `"vars"`:

```json
    "HIGGSFIELD_MCP_CLEANUP_TOOL": "generate_image",
    "HIGGSFIELD_MCP_CLEANUP_MODEL": "seedream_5_lite",
```

- [ ] **Step 6: Run cleanup provider tests**

Run:

```bash
npm run product:test -- seedream_cleanup_prompt_is_exact_text_only_instruction seedream_cleanup_arguments_use_lite_model_and_uploaded_reference seedream_response_extracts_cleaned_image_url
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add workers/product/src/providers/seedream.rs workers/product/src/providers/mod.rs workers/product/src/lib.rs workers/product/wrangler.product.jsonc workers/product/tests/domain_tests.rs
git commit -m "feat: add seedream cleanup provider contract"
```

---

## Task 8: Cleanup Queue State

**Files:**
- Modify: `workers/product/src/queues/niche_research.rs`
- Modify: `workers/product/src/services/visual_reference_cache.rs`

- [ ] **Step 1: Write failing source-level cleanup flow tests**

Add tests in `workers/product/src/queues/niche_research.rs`:

```rust
    #[test]
    fn approved_review_enqueues_cleanup_instead_of_cache() {
        let source = include_str!("niche_research.rs");
        let body = function_body(source, "async fn review_visual_candidates_message");

        assert!(body.contains("NicheResearchMessage::CleanupApprovedReference"));
        assert!(!body.contains("NicheResearchMessage::CacheApprovedReference"));
    }

    #[test]
    fn cleanup_sql_uses_single_review_status_lifecycle() {
        for sql in [
            claim_visual_candidate_for_cleanup_sql(),
            mark_candidate_cleanup_succeeded_sql(),
            mark_candidate_cleanup_failed_sql(),
        ] {
            assert!(sql.contains("review_status"));
            assert!(!sql.contains("cleanup_status"));
        }
        assert!(mark_candidate_cleanup_succeeded_sql().contains("cleaned_image_url = ?"));
        assert!(mark_candidate_cleanup_succeeded_sql().contains("review_status = 'compatibility_pending'"));
        assert!(mark_candidate_cleanup_failed_sql().contains("THEN 'cleanup_retryable'"));
        assert!(mark_candidate_cleanup_failed_sql().contains("ELSE 'cleanup_failed'"));
    }
```

- [ ] **Step 2: Run cleanup flow tests and verify they fail**

Run:

```bash
npm run product:test -- approved_review_enqueues_cleanup_instead_of_cache cleanup_sql_uses_single_review_status_lifecycle
```

Expected: FAIL until cleanup SQL helpers and review flow are changed.

- [ ] **Step 3: Add cleanup SQL helpers**

Add these SQL helper functions near the existing review/cache SQL helpers:

```rust
fn claim_visual_candidate_for_cleanup_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET cleanup_json = json_set(
              CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
              '$.claimStatus',
              'cleanup_pending',
              '$.claimStartedAt',
              ?,
              '$.attempts',
              COALESCE(CAST(json_extract(
                CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) + 1
            ),
            reviewed_at = ?
        WHERE id = ?
          AND clone_id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND review_status IN ('cleanup_pending', 'cleanup_retryable')
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) = ?
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) < ?
          AND image_url IS NOT NULL
          AND TRIM(image_url) <> ''
        "#
}

fn mark_candidate_cleanup_succeeded_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = 'compatibility_pending',
            cleaned_image_url = ?,
            cleanup_json = json_set(
              CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
              '$.cleanedImageUrl',
              ?,
              '$.providerJobId',
              ?,
              '$.completedAt',
              ?
            ),
            rejection_reason = NULL,
            reviewed_at = ?
        WHERE id = ?
          AND clone_id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND review_status IN ('cleanup_pending', 'cleanup_retryable')
        "#
}

fn mark_candidate_cleanup_failed_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = CASE
              WHEN COALESCE(CAST(json_extract(
                CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) < ?
              THEN 'cleanup_retryable'
              ELSE 'cleanup_failed'
            END,
            cleanup_json = json_set(
              CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
              '$.errorCode',
              ?,
              '$.error',
              ?,
              '$.failedAt',
              ?
            ),
            rejection_reason = ?,
            reviewed_at = ?
        WHERE id = ?
          AND clone_id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND review_status IN ('cleanup_pending', 'cleanup_retryable')
        "#
}
```

- [ ] **Step 4: Add cleanup row type and attempt helper**

Add:

```rust
#[derive(Clone, Debug, Deserialize)]
struct CleanupCandidateRow {
    id: String,
    image_url: String,
    image_width: Option<u32>,
    image_height: Option<u32>,
    cleanup_json: String,
}

fn attempts_from_json(value: &str) -> u32 {
    serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|value| value.get("attempts").and_then(Value::as_u64))
        .and_then(|attempts| u32::try_from(attempts).ok())
        .unwrap_or_default()
}
```

- [ ] **Step 5: Change approved review status to cleanup pending**

In `approve_visual_candidate_with_cap_guards_sql()`, change:

```sql
        SET review_status = 'approved',
```

to:

```sql
        SET review_status = 'cleanup_pending',
```

Update cap-count SQL status sets from:

```sql
review_status IN ('approved', 'caching')
```

to:

```sql
review_status IN ('cleanup_pending', 'cleanup_retryable', 'compatibility_pending', 'compatibility_retryable', 'cache_pending', 'caching', 'cached')
```

Apply this to `approved_candidate_count_for_run_sql()`, `approved_candidate_count_for_run_and_handle_sql()`, and the subqueries in `approve_visual_candidate_with_cap_guards_sql()`.

- [ ] **Step 6: Enqueue cleanup after approval**

In `review_visual_candidates_message`, rename `cache_messages_enqueued` to `pipeline_messages_enqueued`. Replace the approval enqueue block with:

```rust
                        env.queue("NICHE_RESEARCH_QUEUE")?
                            .send(NicheResearchMessage::CleanupApprovedReference {
                                user_id: user_id.to_string(),
                                clone_id: clone_id.to_string(),
                                run_id: Some(run_id.clone()),
                                candidate_id: candidate.id.clone(),
                            })
                            .await?;
                        pipeline_messages_enqueued += 1;
```

Keep `ReviewCompletionAction::WaitForCache`; rename it later is optional, but its behavior should remain a delayed finalize nudge.

- [ ] **Step 7: Implement cleanup provider call**

At the top of `niche_research.rs`, add imports:

```rust
use crate::providers::higgsfield_auth::provider_account_access_token;
use crate::providers::higgsfield_mcp::{
    call_tool, upload_media_files, HiggsfieldMcpMediaFile,
};
use crate::providers::seedream::{
    extract_seedream_cleaned_image_url, seedream_cleanup_arguments_with_model,
    SEEDREAM_CLEANUP_MODEL,
};
```

Add constants near generation’s provider constants:

```rust
const HIGGSFIELD_REFRESH_SECRET_NAME: &str = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER";
const HIGGSFIELD_PROVIDER_ACCOUNT_ID: &str = "pa_higgsfield_founder";
const HIGGSFIELD_CLEANUP_TOOL_VAR: &str = "HIGGSFIELD_MCP_CLEANUP_TOOL";
const HIGGSFIELD_CLEANUP_MODEL_VAR: &str = "HIGGSFIELD_MCP_CLEANUP_MODEL";
```

Add helper functions:

```rust
async fn cleanup_reference_with_seedream(
    env: &Env,
    candidate_id: &str,
    image_url: &str,
) -> WorkerResult<(String, String, Value)> {
    let token = provider_account_access_token(
        env,
        HIGGSFIELD_PROVIDER_ACCOUNT_ID,
        HIGGSFIELD_REFRESH_SECRET_NAME,
    )
    .await?;
    let (bytes, content_type) = fetch_reference_cleanup_image(image_url).await?;
    let uploaded = upload_media_files(
        &token,
        &[HiggsfieldMcpMediaFile {
            filename: format!("{candidate_id}.{}", cleanup_extension(&content_type)),
            content_type,
            bytes,
        }],
    )
    .await
    .map_err(|error| Error::RustError(format!("seedream_cleanup_upload_failed:{error}")))?;
    let Some(reference) = uploaded.first() else {
        return Err(Error::RustError("seedream_cleanup_upload_missing".to_string()));
    };
    let tool_name = env
        .var(HIGGSFIELD_CLEANUP_TOOL_VAR)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "generate_image".to_string());
    let model = env
        .var(HIGGSFIELD_CLEANUP_MODEL_VAR)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| SEEDREAM_CLEANUP_MODEL.to_string());
    let response = call_tool(
        &token,
        json!(format!("seedream-cleanup:{candidate_id}")),
        &tool_name,
        seedream_cleanup_arguments_with_model(&reference.reference_value, &model),
    )
    .await
    .map_err(|error| Error::RustError(format!("seedream_cleanup_failed:{error}")))?;
    let cleaned_url = extract_seedream_cleaned_image_url(&response.raw_json)
        .ok_or_else(|| Error::RustError("seedream_cleanup_missing_output_url".to_string()))?;
    let provider_job_id =
        crate::providers::higgsfield_mcp::extract_provider_job_id(&response.raw_json)
            .unwrap_or_default();
    Ok((cleaned_url, provider_job_id, response.raw_json))
}
```

Add a Worker-compatible image fetch helper by copying the same size and content-type checks used by `visual_reference_cache` into private queue helpers, or by making `fetch_visual_reference_image` public in `visual_reference_cache.rs`. The public version should be:

```rust
pub async fn fetch_visual_reference_image(image_url: &str) -> WorkerResult<(Vec<u8>, String)> {
    ...
}
```

Then in `niche_research.rs`:

```rust
async fn fetch_reference_cleanup_image(image_url: &str) -> WorkerResult<(Vec<u8>, String)> {
    crate::services::visual_reference_cache::fetch_visual_reference_image(image_url).await
}

fn cleanup_extension(content_type: &str) -> &'static str {
    match content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "image/heif" => "heif",
        _ => "jpg",
    }
}
```

- [ ] **Step 8: Implement cleanup message**

Replace the cleanup stub with a function that:

1. Loads `visual_reference_cleanup_retry_limit`.
2. Loads the candidate by `clone_id`, `run_id`, `candidate_id`, and status `cleanup_pending` or `cleanup_retryable`.
3. Claims by exact observed attempts.
4. Calls `cleanup_reference_with_seedream`.
5. Marks success to `compatibility_pending` and enqueues `ValidateCloneCompatibility`.
6. Marks failure to `cleanup_retryable` or `cleanup_failed`, then enqueues `ReviewVisualCandidates` and delayed finalize.

Use this candidate load query:

```rust
async fn load_candidate_for_cleanup(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
) -> WorkerResult<Option<CleanupCandidateRow>> {
    db::first(
        db,
        r#"
        SELECT id, image_url, image_width, image_height, cleanup_json
        FROM visual_reference_candidates
        WHERE clone_id = ?
          AND id = ?
          AND review_status IN ('cleanup_pending', 'cleanup_retryable')
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND image_url IS NOT NULL
          AND TRIM(image_url) <> ''
        LIMIT 1
        "#,
        vec![json!(clone_id), json!(candidate_id), json!(run_id), json!(run_id)],
    )
    .await
}
```

- [ ] **Step 9: Run cleanup flow tests**

Run:

```bash
npm run product:test -- approved_review_enqueues_cleanup_instead_of_cache cleanup_sql_uses_single_review_status_lifecycle
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add workers/product/src/queues/niche_research.rs workers/product/src/services/visual_reference_cache.rs
git commit -m "feat: clean approved visual references before storage"
```

---

## Task 9: Kimi Multi-Image Compatibility Prompt

**Files:**
- Modify: `workers/product/Cargo.toml`
- Modify: `workers/product/src/ai/workers_ai.rs`
- Modify: `workers/product/src/domain/visual_reference.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write failing compatibility prompt tests**

Update the Workers AI import in `workers/product/tests/domain_tests.rs`:

```rust
    clone_compatibility_prompt, multi_vision_json_input, CloneCompatibilityReview,
```

Update the visual reference domain import:

```rust
    accept_clone_compatibility,
```

Add tests:

```rust
#[test]
fn clone_compatibility_prompt_checks_only_body_hair_and_facial_hair() {
    let prompt = clone_compatibility_prompt(3);
    let lower = prompt.to_ascii_lowercase();

    assert!(lower.contains("body proportions"));
    assert!(lower.contains("hair length"));
    assert!(lower.contains("facial hair"));
    assert!(!lower.contains("gender"));
    assert!(!lower.contains("same clothing"));
    assert!(!lower.contains("same background"));
}

#[test]
fn multi_vision_payload_contains_candidate_and_clone_images() {
    let input = multi_vision_json_input(
        "Compare these",
        &[
            "https://cdn.example.com/cleaned.webp".to_string(),
            "data:image/jpeg;base64,abc".to_string(),
        ],
    );
    let value = serde_json::to_value(input).unwrap();

    assert_eq!(value["messages"][0]["content"][0]["type"], "text");
    assert_eq!(value["messages"][0]["content"][1]["image_url"]["url"], "https://cdn.example.com/cleaned.webp");
    assert_eq!(value["messages"][0]["content"][2]["image_url"]["url"], "data:image/jpeg;base64,abc");
}

#[test]
fn clone_compatibility_acceptance_requires_all_v1_signals() {
    let accepted = CloneCompatibilityReview {
        compatible: true,
        body_proportions_compatible: true,
        hair_length_compatible: true,
        facial_hair_compatible: true,
        rejection_reason: None,
        reason: "compatible".to_string(),
    };
    assert_eq!(accept_clone_compatibility(&accepted), Ok(()));

    let mismatch = CloneCompatibilityReview {
        facial_hair_compatible: false,
        compatible: true,
        body_proportions_compatible: true,
        hair_length_compatible: true,
        rejection_reason: Some("facial hair mismatch".to_string()),
        reason: "facial hair mismatch".to_string(),
    };
    assert_eq!(accept_clone_compatibility(&mismatch), Err("facial_hair_mismatch"));
}
```

- [ ] **Step 2: Run compatibility tests and verify they fail**

Run:

```bash
npm run product:test -- clone_compatibility_prompt_checks_only_body_hair_and_facial_hair multi_vision_payload_contains_candidate_and_clone_images clone_compatibility_acceptance_requires_all_v1_signals
```

Expected: FAIL because the prompt, multi-image input, and acceptance helper do not exist.

- [ ] **Step 3: Add base64 dependency**

In `workers/product/Cargo.toml`, add:

```toml
base64 = "0.22"
```

- [ ] **Step 4: Add multi-image Workers AI input helper**

In `workers/product/src/ai/workers_ai.rs`, add:

```rust
pub async fn run_multi_vision_json<T: DeserializeOwned>(
    ai: &Ai,
    prompt: &str,
    image_urls: &[String],
) -> WorkerResult<T> {
    let input = multi_vision_json_input(prompt, image_urls);
    let response = ai
        .run::<_, WorkersAiTextResponse>(KIMI_K2_6_MODEL, input)
        .await?;
    decode_structured_response(response)
}

pub fn multi_vision_json_input<'a>(
    prompt: &'a str,
    image_urls: &'a [String],
) -> WorkersAiInput<'a> {
    let mut content = vec![WorkersAiContentPart {
        kind: "text",
        text: Some(prompt),
        image_url: None,
    }];
    for url in image_urls {
        content.push(WorkersAiContentPart {
            kind: "image_url",
            text: None,
            image_url: Some(WorkersAiImageUrl { url }),
        });
    }
    WorkersAiInput {
        messages: vec![WorkersAiMessage {
            role: "user",
            content: WorkersAiMessageContent::Parts(content),
        }],
        response_format: WorkersAiResponseFormat {
            kind: "json_object",
        },
        temperature: 0.1,
    }
}
```

- [ ] **Step 5: Add compatibility type and prompt**

In `workers/product/src/ai/workers_ai.rs`, add:

```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CloneCompatibilityReview {
    pub compatible: bool,
    pub body_proportions_compatible: bool,
    pub hair_length_compatible: bool,
    pub facial_hair_compatible: bool,
    pub rejection_reason: Option<String>,
    #[serde(default)]
    pub reason: String,
}

pub fn clone_compatibility_prompt(clone_reference_count: usize) -> String {
    format!(
        r#"Compare the first image to the following {clone_reference_count} clone reference image(s).

The first image is a cleaned visual reference candidate. The remaining image(s) are the clone's own training references.

Return exactly one strict JSON object:
{{
  "compatible": boolean,
  "bodyProportionsCompatible": boolean,
  "hairLengthCompatible": boolean,
  "facialHairCompatible": boolean,
  "rejectionReason": string | null,
  "reason": string
}}

Accept only if the cleaned candidate is physically compatible enough for Soul-based generation:
- similar body proportions
- similar hair length
- matching facial-hair presence or absence when facial hair is relevant

Do not require the same face, identity, clothing, outfit, background, pose, lighting, camera, or scene. The clone identity comes from the Soul. This review is only a pre-generation compatibility gate for body proportions, hair length, and facial hair."#
    )
}
```

- [ ] **Step 6: Add compatibility acceptance helper**

In `workers/product/src/domain/visual_reference.rs`, add:

```rust
pub fn accept_clone_compatibility(
    review: &crate::ai::workers_ai::CloneCompatibilityReview,
) -> Result<(), &'static str> {
    if !review.compatible {
        return Err("clone_mismatch");
    }
    if !review.body_proportions_compatible {
        return Err("body_proportions_mismatch");
    }
    if !review.hair_length_compatible {
        return Err("hair_length_mismatch");
    }
    if !review.facial_hair_compatible {
        return Err("facial_hair_mismatch");
    }
    Ok(())
}
```

- [ ] **Step 7: Run compatibility tests**

Run:

```bash
npm run product:test -- clone_compatibility_prompt_checks_only_body_hair_and_facial_hair multi_vision_payload_contains_candidate_and_clone_images clone_compatibility_acceptance_requires_all_v1_signals
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add workers/product/Cargo.toml workers/product/src/ai/workers_ai.rs workers/product/src/domain/visual_reference.rs workers/product/tests/domain_tests.rs
git commit -m "feat: add clone compatibility review contract"
```

---

## Task 10: Clone Compatibility Queue State

**Files:**
- Modify: `workers/product/src/queues/niche_research.rs`

- [ ] **Step 1: Write failing compatibility queue tests**

Add tests to `workers/product/src/queues/niche_research.rs`:

```rust
    #[test]
    fn compatibility_sql_uses_single_review_status_lifecycle() {
        assert!(claim_visual_candidate_for_compatibility_sql().contains("review_status IN ('compatibility_pending', 'compatibility_retryable')"));
        assert!(mark_candidate_compatibility_succeeded_sql().contains("review_status = 'cache_pending'"));
        assert!(mark_candidate_clone_mismatch_sql().contains("review_status = 'clone_mismatch'"));
        assert!(mark_candidate_compatibility_failed_sql().contains("THEN 'compatibility_retryable'"));
        assert!(mark_candidate_compatibility_failed_sql().contains("ELSE 'compatibility_failed'"));
    }

    #[test]
    fn cache_load_requires_cleaned_compatible_candidate() {
        let sql = load_approved_candidate_for_cache_sql();

        assert!(sql.contains("review_status IN ('cache_pending', 'caching')"));
        assert!(sql.contains("cleaned_image_url"));
        assert!(sql.contains("compatibility_json"));
        assert!(!sql.contains("review_status IN ('approved', 'caching')"));
    }
```

- [ ] **Step 2: Run compatibility queue tests and verify they fail**

Run:

```bash
npm run product:test -- compatibility_sql_uses_single_review_status_lifecycle cache_load_requires_cleaned_compatible_candidate
```

Expected: FAIL because compatibility SQL and cache load are not updated.

- [ ] **Step 3: Add clone reference row and data URL helpers**

At the top of `niche_research.rs`, import:

```rust
use crate::ai::workers_ai::{
    clone_compatibility_prompt, run_multi_vision_json, CloneCompatibilityReview,
};
use crate::domain::visual_reference::accept_clone_compatibility;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
```

Add row types:

```rust
#[derive(Clone, Debug, Deserialize)]
struct CompatibilityCandidateRow {
    id: String,
    cleaned_image_url: String,
    compatibility_json: String,
}

#[derive(Clone, Debug, Deserialize)]
struct CloneReferenceImageRow {
    storage_key: String,
    content_type: Option<String>,
}
```

Add:

```rust
async fn load_clone_reference_image_urls(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    limit: u32,
) -> WorkerResult<Vec<String>> {
    let rows = db::all::<CloneReferenceImageRow>(
        db,
        r#"
        SELECT ma.storage_key, ma.content_type
        FROM clone_reference_assets cra
        INNER JOIN media_assets ma
          ON ma.id = cra.media_asset_id
         AND ma.deleted_at IS NULL
         AND ma.storage_key IS NOT NULL
         AND TRIM(ma.storage_key) <> ''
        WHERE cra.user_id = ?
          AND cra.clone_id = ?
          AND cra.training_selected = 1
          AND cra.eligibility_status = 'accepted'
        ORDER BY cra.sort_order ASC, cra.created_at ASC
        LIMIT ?
        "#,
        vec![json!(user_id), json!(clone_id), json!(limit)],
    )
    .await?;

    let mut urls = Vec::with_capacity(rows.len());
    for row in rows {
        let object = env
            .bucket("MEDIA")?
            .get(row.storage_key.clone())
            .execute()
            .await?
            .ok_or_else(|| Error::RustError("clone_compatibility_reference_missing".to_string()))?;
        let body = object
            .body()
            .ok_or_else(|| Error::RustError("clone_compatibility_reference_body_missing".to_string()))?;
        let bytes = body.bytes().await?;
        let content_type = row
            .content_type
            .as_deref()
            .unwrap_or("image/jpeg")
            .split(';')
            .next()
            .unwrap_or("image/jpeg")
            .trim()
            .to_string();
        urls.push(format!(
            "data:{};base64,{}",
            content_type,
            BASE64_STANDARD.encode(bytes)
        ));
    }

    Ok(urls)
}
```

- [ ] **Step 4: Add compatibility SQL helpers**

Add SQL helpers mirroring cleanup attempts, but using `compatibility_json` and statuses:

```rust
fn claim_visual_candidate_for_compatibility_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET compatibility_json = json_set(
              CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
              '$.claimStatus',
              'compatibility_pending',
              '$.claimStartedAt',
              ?,
              '$.attempts',
              COALESCE(CAST(json_extract(
                CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) + 1
            ),
            reviewed_at = ?
        WHERE id = ?
          AND clone_id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND review_status IN ('compatibility_pending', 'compatibility_retryable')
          AND cleaned_image_url IS NOT NULL
          AND TRIM(cleaned_image_url) <> ''
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) = ?
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) < ?
        "#
}

fn mark_candidate_compatibility_succeeded_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = 'cache_pending',
            compatibility_json = ?,
            rejection_reason = NULL,
            reviewed_at = ?
        WHERE id = ?
          AND clone_id = ?
          AND review_status IN ('compatibility_pending', 'compatibility_retryable')
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
        "#
}

fn mark_candidate_clone_mismatch_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = 'clone_mismatch',
            compatibility_json = ?,
            rejection_reason = ?,
            reviewed_at = ?
        WHERE id = ?
          AND clone_id = ?
          AND review_status IN ('compatibility_pending', 'compatibility_retryable')
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
        "#
}

fn mark_candidate_compatibility_failed_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = CASE
              WHEN COALESCE(CAST(json_extract(
                CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) < ?
              THEN 'compatibility_retryable'
              ELSE 'compatibility_failed'
            END,
            compatibility_json = json_set(
              CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
              '$.errorCode',
              ?,
              '$.error',
              ?,
              '$.failedAt',
              ?
            ),
            rejection_reason = ?,
            reviewed_at = ?
        WHERE id = ?
          AND clone_id = ?
          AND review_status IN ('compatibility_pending', 'compatibility_retryable')
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
        "#
}
```

- [ ] **Step 5: Implement compatibility message**

Replace the compatibility stub with a function that:

1. Loads `visual_reference_compatibility_retry_limit` and `clone_compatibility_reference_limit`.
2. Loads candidate where status is `compatibility_pending` or `compatibility_retryable`.
3. Claims by observed `compatibility_json` attempts.
4. Loads clone reference data URLs from R2.
5. Calls `run_multi_vision_json::<CloneCompatibilityReview>` with the cleaned candidate URL first and clone reference data URLs after it.
6. Calls `accept_clone_compatibility`.
7. On success, marks `cache_pending` and enqueues `CacheApprovedReference`.
8. On clear mismatch, marks `clone_mismatch`, then enqueues `ReviewVisualCandidates` and delayed finalize.
9. On provider/parse failure, marks `compatibility_retryable` or `compatibility_failed`, then enqueues `ReviewVisualCandidates` and delayed finalize.

Use this load query:

```rust
async fn load_candidate_for_compatibility(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
) -> WorkerResult<Option<CompatibilityCandidateRow>> {
    db::first(
        db,
        r#"
        SELECT id, cleaned_image_url, compatibility_json
        FROM visual_reference_candidates
        WHERE clone_id = ?
          AND id = ?
          AND review_status IN ('compatibility_pending', 'compatibility_retryable')
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND cleaned_image_url IS NOT NULL
          AND TRIM(cleaned_image_url) <> ''
        LIMIT 1
        "#,
        vec![json!(clone_id), json!(candidate_id), json!(run_id), json!(run_id)],
    )
    .await
}
```

- [ ] **Step 6: Run compatibility queue tests**

Run:

```bash
npm run product:test -- compatibility_sql_uses_single_review_status_lifecycle cache_load_requires_cleaned_compatible_candidate
```

Expected: The first test passes after SQL helpers exist. The second remains failing until Task 11 updates cache load.

- [ ] **Step 7: Commit compatibility queue state**

```bash
git add workers/product/src/queues/niche_research.rs
git commit -m "feat: validate visual references against clone"
```

---

## Task 11: Cache Only Cleaned Compatible Images

**Files:**
- Modify: `workers/product/src/queues/niche_research.rs`
- Modify: `workers/product/src/services/visual_reference_cache.rs`
- Modify: `workers/product/tests/domain_tests.rs`

- [ ] **Step 1: Write failing cache contract tests**

In `workers/product/tests/domain_tests.rs`, add:

```rust
#[test]
fn visual_reference_cache_metadata_uses_cleaned_remote_url_label() {
    let source = include_str!("../src/services/visual_reference_cache.rs");

    assert!(source.contains("cleaned_image_url"));
    assert!(source.contains("\"cleanedImageUrl\""));
    assert!(!source.contains("original_image_url"));
}
```

In `workers/product/src/queues/niche_research.rs`, keep the failing `cache_load_requires_cleaned_compatible_candidate` test from Task 10.

- [ ] **Step 2: Run cache tests and verify they fail**

Run:

```bash
npm run product:test -- visual_reference_cache_metadata_uses_cleaned_remote_url_label cache_load_requires_cleaned_compatible_candidate
```

Expected: FAIL because cache still loads `image_url` from approved candidates.

- [ ] **Step 3: Update cache service naming and metadata**

In `workers/product/src/services/visual_reference_cache.rs`, change the public function signature parameter:

```rust
    cleaned_image_url: &str,
```

Use it for fetch:

```rust
    let (bytes, content_type) = fetch_visual_reference_image(cleaned_image_url).await?;
```

Use it in `media_assets.remote_url`:

```rust
            json!(cleaned_image_url),
```

Change metadata to:

```rust
            json!(json!({
                "visualReferenceId": visual_reference_id,
                "cleanedImageUrl": cleaned_image_url
            }).to_string()),
```

Make `fetch_visual_reference_image` public if Task 8 did not already:

```rust
pub async fn fetch_visual_reference_image(image_url: &str) -> WorkerResult<(Vec<u8>, String)> {
```

- [ ] **Step 4: Update approved candidate row**

In `ApprovedVisualCandidateRow`, replace:

```rust
    image_url: String,
```

with:

```rust
    cleaned_image_url: String,
    compatibility_json: String,
```

- [ ] **Step 5: Add cache load SQL helper function**

Replace inline SQL in `load_approved_candidate_for_cache` with:

```rust
fn load_approved_candidate_for_cache_sql() -> &'static str {
    r#"
        SELECT
          id,
          source_handle,
          source_post_code,
          source_url,
          source_published_at,
          cleaned_image_url,
          image_width,
          image_height,
          moodboard_id,
          moodboard_slug,
          review_json,
          compatibility_json
        FROM visual_reference_candidates
        WHERE clone_id = ?
          AND id = ?
          AND review_status IN ('cache_pending', 'caching')
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND cleaned_image_url IS NOT NULL
          AND TRIM(cleaned_image_url) <> ''
        LIMIT 1
        "#
}
```

Use it in `load_approved_candidate_for_cache`.

- [ ] **Step 6: Update cache claim SQL**

In `claim_visual_candidate_for_cache_sql()`, change the eligible queued status from `approved` to `cache_pending`:

```sql
            review_status = 'cache_pending'
```

Keep stale `caching` retry support.

- [ ] **Step 7: Update cache call to use cleaned image URL**

In `cache_approved_reference_message`, call:

```rust
        &candidate.cleaned_image_url,
```

instead of `&candidate.image_url`.

- [ ] **Step 8: Mark cache success as cached**

In `mark_candidate_cache_succeeded_sql()`, change:

```sql
        SET review_status = 'approved',
```

to:

```sql
        SET review_status = 'cached',
```

- [ ] **Step 9: Run cache tests**

Run:

```bash
npm run product:test -- visual_reference_cache_metadata_uses_cleaned_remote_url_label cache_load_requires_cleaned_compatible_candidate
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add workers/product/src/queues/niche_research.rs workers/product/src/services/visual_reference_cache.rs workers/product/tests/domain_tests.rs
git commit -m "feat: cache only cleaned compatible references"
```

---

## Task 12: Finalization Drain And Replacement Search

**Files:**
- Modify: `workers/product/src/queues/niche_research.rs`

- [ ] **Step 1: Write failing drain tests**

Update `finalize_visual_drain_sql_detects_reviewable_and_cacheable_work()` with:

```rust
        assert!(sql.contains("vc.review_status = 'cleanup_pending'"));
        assert!(sql.contains("vc.review_status = 'cleanup_retryable'"));
        assert!(sql.contains("vc.review_status = 'compatibility_pending'"));
        assert!(sql.contains("vc.review_status = 'compatibility_retryable'"));
        assert!(sql.contains("vc.review_status = 'cache_pending'"));
```

Add:

```rust
    #[test]
    fn final_status_detail_counts_cleanup_and_compatibility_failures() {
        let source = include_str!("niche_research.rs");
        let body = function_body(source, "async fn finalize_reference_pool_message");

        assert!(body.contains("cleanup_failed"));
        assert!(body.contains("compatibility_failed"));
        assert!(body.contains("clone_mismatch"));
        assert!(body.contains("cache_failed"));
    }
```

- [ ] **Step 2: Run drain tests and verify they fail**

Run:

```bash
npm run product:test -- finalize_visual_drain_sql_detects_reviewable_and_cacheable_work final_status_detail_counts_cleanup_and_compatibility_failures
```

Expected: FAIL until finalization includes cleanup and compatibility statuses.

- [ ] **Step 3: Update pending visual work SQL**

In `finalize_pending_visual_work_sql()`, add these branches to the OR block:

```sql
            OR vc.review_status = 'cleanup_pending'
            OR (
              vc.review_status = 'cleanup_retryable'
              AND COALESCE(CAST(json_extract(
                CASE WHEN json_valid(vc.cleanup_json) THEN vc.cleanup_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) < ?
            )
            OR vc.review_status = 'compatibility_pending'
            OR (
              vc.review_status = 'compatibility_retryable'
              AND COALESCE(CAST(json_extract(
                CASE WHEN json_valid(vc.compatibility_json) THEN vc.compatibility_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) < ?
            )
            OR vc.review_status = 'cache_pending'
```

Update `load_finalize_drain_state` to pass cleanup retry limit and compatibility retry limit into this SQL.

- [ ] **Step 4: Update in-progress visual work SQL**

Include statuses with recent claims:

```sql
          AND (
            vc.review_status IN ('reviewing', 'caching')
            OR (
              vc.review_status IN ('cleanup_pending', 'cleanup_retryable')
              AND json_extract(
                CASE WHEN json_valid(vc.cleanup_json) THEN vc.cleanup_json ELSE '{}' END,
                '$.claimStartedAt'
              ) IS NOT NULL
            )
            OR (
              vc.review_status IN ('compatibility_pending', 'compatibility_retryable')
              AND json_extract(
                CASE WHEN json_valid(vc.compatibility_json) THEN vc.compatibility_json ELSE '{}' END,
                '$.claimStartedAt'
              ) IS NOT NULL
            )
          )
```

Keep the existing stale-time condition using `reviewed_at`.

- [ ] **Step 5: Update uncached candidate SQL**

In `finalize_approved_uncached_candidates_sql()`, change the cacheable status from `approved` to `cache_pending`, while retaining stale `caching`:

```sql
            vc.review_status = 'cache_pending'
```

- [ ] **Step 6: Update drain action**

Extend `FinalizeDrainState`:

```rust
    cleanup_candidate_ids: Vec<String>,
    compatibility_candidate_ids: Vec<String>,
```

Load limited IDs for statuses `cleanup_pending`/retryable and `compatibility_pending`/retryable. Add actions:

```rust
    EnqueueCleanup,
    EnqueueCompatibility,
```

Order in `finalize_drain_action`:

```rust
    if !state.cleanup_candidate_ids.is_empty() {
        FinalizeDrainAction::EnqueueCleanup
    } else if !state.compatibility_candidate_ids.is_empty() {
        FinalizeDrainAction::EnqueueCompatibility
    } else if !state.approved_uncached_candidate_ids.is_empty() {
        FinalizeDrainAction::EnqueueCache
```

In `finalize_reference_pool_message`, enqueue the corresponding message for each candidate ID.

- [ ] **Step 7: Continue searching for replacements**

After cleanup failure, compatibility failure, or clone mismatch, enqueue:

```rust
env.queue("NICHE_RESEARCH_QUEUE")?
    .send(NicheResearchMessage::ReviewVisualCandidates {
        user_id: user_id.to_string(),
        clone_id: clone_id.to_string(),
        run_id: Some(run_id.clone()),
        limit: review_limit,
    })
    .await?;
```

Use this in the failure branches in cleanup and compatibility messages. This lets the run keep reviewing remaining candidates for replacements before finalization settles readiness.

- [ ] **Step 8: Add final status counts**

Add helper:

```rust
#[derive(Debug, Default, Deserialize)]
struct CandidateFailureCountsRow {
    cleanup_failed: u32,
    compatibility_failed: u32,
    clone_mismatch: u32,
    cache_failed: u32,
}
```

Add load query:

```rust
async fn load_candidate_failure_counts(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
) -> WorkerResult<CandidateFailureCountsRow> {
    Ok(db::first(
        db,
        r#"
        SELECT
          SUM(CASE WHEN review_status = 'cleanup_failed' THEN 1 ELSE 0 END) AS cleanup_failed,
          SUM(CASE WHEN review_status = 'compatibility_failed' THEN 1 ELSE 0 END) AS compatibility_failed,
          SUM(CASE WHEN review_status = 'clone_mismatch' THEN 1 ELSE 0 END) AS clone_mismatch,
          SUM(CASE WHEN review_status = 'cache_failed' THEN 1 ELSE 0 END) AS cache_failed
        FROM visual_reference_candidates
        WHERE clone_id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
        "#,
        vec![json!(clone_id), json!(run_id)],
    )
    .await?
    .unwrap_or_default())
}
```

In final status detail, append:

```rust
            ", cleanup_failed={}, compatibility_failed={}, clone_mismatch={}, cache_failed={}",
            failures.cleanup_failed,
            failures.compatibility_failed,
            failures.clone_mismatch,
            failures.cache_failed,
```

- [ ] **Step 9: Run drain tests**

Run:

```bash
npm run product:test -- finalize_visual_drain_sql_detects_reviewable_and_cacheable_work final_status_detail_counts_cleanup_and_compatibility_failures
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add workers/product/src/queues/niche_research.rs
git commit -m "feat: drain cleanup and compatibility work"
```

---

## Task 13: Full Contract Verification

**Files:**
- Potentially modify files from earlier tasks only to fix compile and contract issues.

- [ ] **Step 1: Run Rust unit tests**

Run:

```bash
npm run product:test
```

Expected: PASS. If it fails, use `superpowers:systematic-debugging` before changing implementation.

- [ ] **Step 2: Run Product Worker check**

Run:

```bash
npm run product:check
```

Expected: PASS for `wasm32-unknown-unknown`.

- [ ] **Step 3: Run TypeScript checks**

Run:

```bash
npm run typecheck
```

Expected: PASS.

- [ ] **Step 4: Build**

Run:

```bash
npm run build
```

Expected: PASS.

- [ ] **Step 5: Inspect git diff for scope**

Run:

```bash
git status --short
git diff --stat
```

Expected: only files listed in this plan changed, plus Cargo lockfile changes if generated by Cargo. Pre-existing dirty files may still be present and must not be reverted.

- [ ] **Step 6: Commit final fixes**

If Step 1 through Step 4 required compile or test fixes, commit them:

```bash
git add workers/product/Cargo.toml workers/product/Cargo.lock workers/product/src workers/product/tests config/d1/migrations/1007_visual_reference_pipeline.sql workers/product/wrangler.product.jsonc
git commit -m "test: verify pipeline v2 visual references"
```

Skip this commit if no files changed after the previous task commit.

---

## Self-Review

- Spec coverage:
  - Pipeline-v2 Reels Search owner-handle discovery: Tasks 2 and 4.
  - Static posts and carousel images only: Tasks 2, 4, and 5 keep final candidates in post/profile flows and do not use reel media.
  - Workers AI Kimi K2.6 review: Task 6 preserves Workers AI Kimi and improves the prompt.
  - Seedream 5.0 Lite text-only cleanup: Tasks 7 and 8 add the exact prompt and cleanup queue state.
  - Keep only cleaned images: Task 11 changes cache to use `cleaned_image_url`.
  - Retry cleanup then fail and continue replacements: Tasks 8 and 12.
  - Clone compatibility before generation-ready storage: Tasks 9 and 10.
  - No gender compatibility signal: Task 9 tests the prompt does not mention gender.
  - `visual_references` generation-ready only: Tasks 10, 11, and 12 gate cache and finalization.
  - Finalization drains cleanup and compatibility: Task 12.

- Placeholder scan:
  - No placeholder red-flag task shortcuts are intentionally used in implementation steps.
  - Stub functions in Task 3 are named sentinel failures and are explicitly replaced by Tasks 4, 8, and 10.

- Type consistency:
  - Candidate lifecycle uses `visual_reference_candidates.review_status` only.
  - New JSON columns are `cleanup_json` and `compatibility_json`.
  - Cleaned URL column is `cleaned_image_url`.
  - Queue variants use snake_case message names from the approved spec.
  - Compatibility review fields use camelCase externally and snake_case Rust fields through `serde(rename_all = "camelCase")`.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-17-pipeline-v2-visual-reference-migration.md`. Two execution options:

**1. Subagent-Driven (recommended)** - Dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
