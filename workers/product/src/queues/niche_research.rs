use crate::ai::workers_ai::{
    clustering_prompt, human_presence_prompt, knowledge_extraction_prompt, run_text_json,
    run_vision_json, seed_extraction_prompt,
};
use crate::db;
use crate::domain::blitz::{
    can_accept_human_presence, classify_freshness, filter_synthetic_terms, FreshnessDecision,
    HumanPresenceReview,
};
use crate::providers::scrapecreators::{
    build_scrape_request, fetch_scrapecreators_json, normalize_instagram_reels_search,
    normalize_tiktok_hashtag_search, normalize_tiktok_keyword_search, NormalizedDiscoveryItem,
    ScrapePlatform,
};
use crate::services::blitz::create_next_batch;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use worker::{Ai, D1Database, Delay, Env, Error, MessageBatch, MessageExt, Result as WorkerResult};

const NICHE_RESEARCH_STATUS_CAS_MISS: &str = "niche_research_status_cas_miss";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum NicheResearchMessage {
    ResearchMoodboardReferences {
        user_id: String,
        clone_id: String,
        moodboard_ids: Vec<String>,
        reason: String,
    },
    FetchInstagramProfile {
        user_id: String,
        clone_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        moodboard_id: String,
        moodboard_slug: String,
        handle: String,
        discovered_via: String,
        related_depth: u8,
    },
    FetchInstagramPosts {
        user_id: String,
        clone_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        moodboard_id: String,
        moodboard_slug: String,
        handle: String,
        discovered_via: String,
        next_max_id: Option<String>,
        page: u8,
    },
    ReviewVisualCandidates {
        user_id: String,
        clone_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        limit: u32,
    },
    CacheApprovedReference {
        user_id: String,
        clone_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        candidate_id: String,
    },
    FinalizeReferencePool {
        user_id: String,
        clone_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        reason: String,
    },
    RefreshPool {
        user_id: String,
        clone_id: String,
        reason: String,
    },
}

pub async fn handle_batch(batch: MessageBatch<Value>, env: Env) -> WorkerResult<()> {
    for raw_message in batch.raw_iter() {
        let message =
            match serde_wasm_bindgen::from_value::<NicheResearchMessage>(raw_message.body()) {
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

        let failure_context = message_failure_context(&message);
        match handle_message(message, &env).await {
            Ok(()) => raw_message.ack(),
            Err(error) => {
                let detail = error.to_string();
                if is_niche_research_status_race_error(&detail) {
                    web_sys::console::error_1(
                        &format!(
                            "retry niche research queue message after raced status write: message={}",
                            failure_context.message_type
                        )
                        .into(),
                    );
                    raw_message.retry();
                    continue;
                }

                web_sys::console::error_1(
                    &format!(
                        "niche research queue message failed without panic: code={}, detail={}",
                        queue_error_code(&detail),
                        compact_error_detail(&detail)
                    )
                    .into(),
                );
                match record_message_failure(&env, &failure_context, &detail).await {
                    Ok(outcome) => match failure_record_action(Some(outcome)) {
                        QueueMessageAction::Ack => {
                            if outcome != FailureRecordOutcome::Recorded {
                                web_sys::console::log_1(
                                    &format!(
                                        "ack stale niche research failure message without status write: message={}",
                                        failure_context.message_type
                                    )
                                    .into(),
                                );
                            }
                            raw_message.ack();
                        }
                        QueueMessageAction::Retry => {
                            web_sys::console::error_1(
                                &format!(
                                    "retry niche research queue message after raced failure record: message={}",
                                    failure_context.message_type
                                )
                                .into(),
                            );
                            raw_message.retry();
                        }
                    },
                    Err(status_error) => {
                        web_sys::console::error_1(
                            &format!(
                                "failed to record niche research queue failure: code={}, detail={}",
                                queue_error_code(&status_error.to_string()),
                                compact_error_detail(&status_error.to_string())
                            )
                            .into(),
                        );
                        raw_message.retry();
                    }
                }
            }
        }
    }

    Ok(())
}

struct MessageFailureContext {
    user_id: String,
    clone_id: String,
    run_id: Option<String>,
    message_type: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailureRecordOutcome {
    Recorded,
    SkippedStale,
    SkippedRaced,
    MissingClone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueueMessageAction {
    Ack,
    Retry,
}

fn failure_record_action(outcome: Option<FailureRecordOutcome>) -> QueueMessageAction {
    match outcome {
        Some(
            FailureRecordOutcome::Recorded
            | FailureRecordOutcome::SkippedStale
            | FailureRecordOutcome::MissingClone,
        ) => QueueMessageAction::Ack,
        Some(FailureRecordOutcome::SkippedRaced) | None => QueueMessageAction::Retry,
    }
}

fn is_niche_research_status_race_error(error: &str) -> bool {
    error.contains(NICHE_RESEARCH_STATUS_CAS_MISS)
}

fn message_failure_context(message: &NicheResearchMessage) -> MessageFailureContext {
    match message {
        NicheResearchMessage::ResearchMoodboardReferences {
            user_id, clone_id, ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: None,
            message_type: "research_moodboard_references",
        },
        NicheResearchMessage::FetchInstagramProfile {
            user_id,
            clone_id,
            run_id,
            ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: run_id.clone(),
            message_type: "fetch_instagram_profile",
        },
        NicheResearchMessage::FetchInstagramPosts {
            user_id,
            clone_id,
            run_id,
            ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: run_id.clone(),
            message_type: "fetch_instagram_posts",
        },
        NicheResearchMessage::ReviewVisualCandidates {
            user_id,
            clone_id,
            run_id,
            ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: run_id.clone(),
            message_type: "review_visual_candidates",
        },
        NicheResearchMessage::CacheApprovedReference {
            user_id,
            clone_id,
            run_id,
            ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: run_id.clone(),
            message_type: "cache_approved_reference",
        },
        NicheResearchMessage::FinalizeReferencePool {
            user_id,
            clone_id,
            run_id,
            ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: run_id.clone(),
            message_type: "finalize_reference_pool",
        },
        NicheResearchMessage::RefreshPool {
            user_id, clone_id, ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: None,
            message_type: "refresh_pool",
        },
    }
}

async fn handle_message(message: NicheResearchMessage, env: &Env) -> WorkerResult<()> {
    let db = env.d1("DB")?;
    match message {
        NicheResearchMessage::ResearchMoodboardReferences {
            user_id,
            clone_id,
            moodboard_ids,
            reason,
        } => {
            research_moodboard_references(&db, env, &user_id, &clone_id, &moodboard_ids, &reason)
                .await
        }
        NicheResearchMessage::FetchInstagramProfile {
            user_id,
            clone_id,
            run_id,
            moodboard_id,
            moodboard_slug,
            handle,
            discovered_via,
            related_depth,
        } => {
            fetch_instagram_profile_message(
                &db,
                env,
                &user_id,
                &clone_id,
                run_id.as_deref(),
                &moodboard_id,
                &moodboard_slug,
                &handle,
                &discovered_via,
                related_depth,
            )
            .await
        }
        NicheResearchMessage::FetchInstagramPosts {
            user_id,
            clone_id,
            run_id,
            moodboard_id,
            moodboard_slug,
            handle,
            discovered_via,
            next_max_id,
            page,
        } => {
            fetch_instagram_posts_message(
                &db,
                env,
                &user_id,
                &clone_id,
                run_id.as_deref(),
                &moodboard_id,
                &moodboard_slug,
                &handle,
                &discovered_via,
                next_max_id.as_deref(),
                page,
            )
            .await
        }
        NicheResearchMessage::ReviewVisualCandidates {
            user_id,
            clone_id,
            run_id,
            limit,
        } => {
            review_visual_candidates_message(
                &db,
                env,
                &user_id,
                &clone_id,
                run_id.as_deref(),
                limit,
            )
            .await
        }
        NicheResearchMessage::CacheApprovedReference {
            user_id,
            clone_id,
            run_id,
            candidate_id,
        } => {
            cache_approved_reference_message(
                &db,
                env,
                &user_id,
                &clone_id,
                run_id.as_deref(),
                &candidate_id,
            )
            .await
        }
        NicheResearchMessage::FinalizeReferencePool {
            user_id,
            clone_id,
            run_id,
            reason,
        } => {
            finalize_reference_pool_message(
                &db,
                env,
                &user_id,
                &clone_id,
                run_id.as_deref(),
                &reason,
            )
            .await
        }
        NicheResearchMessage::RefreshPool {
            user_id,
            clone_id,
            reason,
        } => {
            let moodboard_ids = load_selected_moodboard_ids(&db, &user_id, &clone_id).await?;
            research_moodboard_references(&db, env, &user_id, &clone_id, &moodboard_ids, &reason)
                .await
        }
    }
}

async fn research_moodboard_references(
    db: &D1Database,
    _env: &Env,
    user_id: &str,
    clone_id: &str,
    _moodboard_ids: &[String],
    reason: &str,
) -> WorkerResult<()> {
    let run_id = new_research_run_id();
    set_clone_research_status_with_run(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Queued),
        reason,
        None,
        Some(&run_id),
    )
    .await
}

async fn fetch_instagram_profile_message(
    db: &D1Database,
    _env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    _moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    _discovered_via: &str,
    _related_depth: u8,
) -> WorkerResult<()> {
    set_clone_research_status_with_run(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Scraping),
        &format!("fetching instagram profile handle={handle} moodboard={moodboard_slug}"),
        run_id,
        run_id,
    )
    .await
}

async fn fetch_instagram_posts_message(
    db: &D1Database,
    _env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    _moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    _discovered_via: &str,
    _next_max_id: Option<&str>,
    page: u8,
) -> WorkerResult<()> {
    set_clone_research_status_with_run(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Scraping),
        &format!("fetching instagram posts handle={handle} moodboard={moodboard_slug} page={page}"),
        run_id,
        run_id,
    )
    .await
}

async fn review_visual_candidates_message(
    db: &D1Database,
    _env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    limit: u32,
) -> WorkerResult<()> {
    set_clone_research_status_with_run(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Reviewing),
        &format!("reviewing visual candidates limit={limit}"),
        run_id,
        run_id,
    )
    .await
}

async fn cache_approved_reference_message(
    db: &D1Database,
    _env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    candidate_id: &str,
) -> WorkerResult<()> {
    set_clone_research_status_with_run(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Reviewing),
        &format!("caching approved visual reference candidate={candidate_id}"),
        run_id,
        run_id,
    )
    .await
}

async fn finalize_reference_pool_message(
    db: &D1Database,
    _env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    reason: &str,
) -> WorkerResult<()> {
    set_clone_research_status_with_run(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::InsufficientRefs),
        &format!("finalize requested before discovery expansion: {reason}"),
        run_id,
        run_id,
    )
    .await
}

async fn record_message_failure(
    env: &Env,
    context: &MessageFailureContext,
    detail: &str,
) -> WorkerResult<FailureRecordOutcome> {
    let db = env.d1("DB")?;
    let code = queue_error_code(detail);
    let compact_detail = compact_error_detail(detail);
    web_sys::console::error_1(
        &format!(
            "niche research queue failure recorded: message={}, code={}, detail={}",
            context.message_type, code, compact_detail
        )
        .into(),
    );
    let result = write_clone_research_status(
        &db,
        &context.user_id,
        &context.clone_id,
        research_status_for_phase(ResearchPhase::Failed),
        &format!(
            "{} failed: code={}, detail={}",
            context.message_type, code, compact_detail
        ),
        ResearchStatusWriteMode::Failure,
        context.run_id.as_deref(),
        context.run_id.as_deref(),
    )
    .await?;

    Ok(match result {
        ResearchStatusWriteResult::Written => FailureRecordOutcome::Recorded,
        ResearchStatusWriteResult::SkippedStale => FailureRecordOutcome::SkippedStale,
        ResearchStatusWriteResult::SkippedRaced => FailureRecordOutcome::SkippedRaced,
        ResearchStatusWriteResult::MissingClone => FailureRecordOutcome::MissingClone,
    })
}

async fn handle_seed_from_moodboards(
    db: &D1Database,
    env: &Env,
    user_id: String,
    clone_id: String,
    moodboard_ids: Vec<String>,
    moderation_level: u8,
    platforms: Vec<String>,
) -> WorkerResult<()> {
    let Some(clone) = load_clone_for_research(db, &user_id, &clone_id).await? else {
        web_sys::console::log_1(
            &format!("ack niche research for missing clone user={user_id} clone={clone_id}").into(),
        );
        return Ok(());
    };
    let ai = env.ai("AI")?;
    let moodboards = load_selected_moodboards(db, &user_id, &clone_id, &moodboard_ids).await?;
    let selected_moodboard_ids = moodboards
        .iter()
        .map(|moodboard| moodboard.id.as_str())
        .collect::<Vec<_>>();
    if !valid_loaded_moodboard_count(moodboards.len()) {
        set_clone_research_status(
            db,
            &user_id,
            &clone_id,
            "insufficient_moodboards",
            &format!(
                "selected_moodboards={}, required=5, moodboard_ids={}",
                moodboards.len(),
                selected_moodboard_ids.join(",")
            ),
        )
        .await?;
        return Ok(());
    }

    let config = load_config_map(db).await?;
    let allowed_platforms = normalize_platforms(&platforms);
    if allowed_platforms.is_empty() {
        set_clone_research_status(
            db,
            &user_id,
            &clone_id,
            "insufficient_refs",
            "no supported platforms requested",
        )
        .await?;
        return Ok(());
    }

    let active_niche = active_niche_from_moodboards(&moodboards);
    let excluded_terms = moodboard_search_queries(&moodboards);
    let seed_prompt = seed_extraction_prompt(&active_niche, &excluded_terms);
    let seed_response = run_text_json::<SeedExtractionResponse>(&ai, &seed_prompt).await?;
    let mut seed_queries = accepted_seed_queries(seed_response.seeds, &allowed_platforms);
    if seed_queries.is_empty() {
        seed_queries = fallback_moodboard_seed_queries(&excluded_terms, &allowed_platforms);
    }
    let max_seed_queries_per_platform =
        config_u32(&config, "max_seed_queries_per_platform", 8) as usize;
    seed_queries = cap_seed_queries_per_platform(seed_queries, max_seed_queries_per_platform);
    insert_seed_queries(db, &user_id, &clone_id, &seed_queries).await?;

    run_scrape_pass(db, env, &user_id, &clone_id, &seed_queries, &config).await?;
    let deeper_queries = run_knowledge_and_clustering(
        db,
        &ai,
        &user_id,
        &clone_id,
        &active_niche,
        &allowed_platforms,
        &config,
    )
    .await?;
    if !deeper_queries.is_empty() {
        let deeper_queries =
            cap_seed_queries_per_platform(deeper_queries, max_seed_queries_per_platform);
        insert_seed_queries(db, &user_id, &clone_id, &deeper_queries).await?;
        run_scrape_pass(db, env, &user_id, &clone_id, &deeper_queries, &config).await?;
    }

    run_visual_reference_selection(
        db,
        &ai,
        &user_id,
        &clone_id,
        moderation_level,
        &allowed_platforms,
        &config,
    )
    .await?;
    finalize_research_pool(db, env, &clone, &clone_id, &config).await
}

async fn handle_refresh_pool(
    db: &D1Database,
    env: &Env,
    user_id: String,
    clone_id: String,
    reason: String,
) -> WorkerResult<()> {
    let moodboard_ids = load_selected_moodboard_ids(db, &user_id, &clone_id).await?;
    research_moodboard_references(db, env, &user_id, &clone_id, &moodboard_ids, &reason).await
}

async fn load_clone_for_research(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
) -> WorkerResult<Option<CloneResearchRow>> {
    db::first(
        db,
        r#"
        SELECT user_id, soul_status, provider_soul_id, provider_config_json
        FROM clone_profiles
        WHERE user_id = ?
          AND id = ?
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        vec![json!(user_id), json!(clone_id)],
    )
    .await
}

async fn load_selected_moodboards(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    moodboard_ids: &[String],
) -> WorkerResult<Vec<MoodboardRow>> {
    if moodboard_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat("?")
        .take(moodboard_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT id, title, vibe_summary, search_queries_json
        FROM moodboards
        WHERE user_id = ?
          AND clone_id = ?
          AND selected = 1
          AND id IN ({placeholders})
        ORDER BY sort_order ASC, created_at ASC
        "#
    );
    let mut params = vec![json!(user_id), json!(clone_id)];
    params.extend(moodboard_ids.iter().map(|id| json!(id)));
    db::all(db, &sql, params).await
}

async fn load_selected_moodboard_ids(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
) -> WorkerResult<Vec<String>> {
    let rows = db::all::<IdRow>(
        db,
        r#"
        SELECT id
        FROM moodboards
        WHERE user_id = ?
          AND clone_id = ?
          AND selected = 1
        ORDER BY sort_order ASC, created_at ASC
        "#,
        vec![json!(user_id), json!(clone_id)],
    )
    .await?;
    Ok(rows.into_iter().map(|row| row.id).collect())
}

async fn insert_seed_queries(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    seeds: &[SeedQuery],
) -> WorkerResult<()> {
    let now = now_iso_string();
    for seed in seeds {
        if seed.query.trim().is_empty() || filter_synthetic_terms(&seed.query).is_err() {
            continue;
        }
        let id = deterministic_id("niche_query", &[clone_id, &seed.platform, &seed.query]);
        db::exec(
            db,
            r#"
            INSERT OR IGNORE INTO niche_research_queries (
              id, user_id, clone_id, moodboard_id, query, source, status, raw_json, created_at
            )
            VALUES (?, ?, ?, NULL, ?, ?, 'new', ?, ?)
            "#,
            vec![
                json!(id),
                json!(user_id),
                json!(clone_id),
                json!(seed.query),
                json!(seed.source),
                json!(seed.raw_json.to_string()),
                json!(now),
            ],
        )
        .await?;
    }
    Ok(())
}

async fn run_scrape_pass(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    seeds: &[SeedQuery],
    config: &HashMap<String, String>,
) -> WorkerResult<()> {
    if seeds.is_empty() {
        return Ok(());
    }

    let base_url = env_var(
        env,
        "SCRAPECREATORS_BASE_URL",
        "scrapecreators_base_url_missing",
    )?;
    let api_key = env_var(
        env,
        "SCRAPECREATORS_API_KEY",
        "scrapecreators_api_key_missing",
    )?;
    let region = env
        .var("DISCOVERY_DEFAULT_REGION")
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "US".to_string());
    let now = now_iso_string();
    let freshness_years = config_u32(config, "freshness_window_years", 5) as i64;
    let allow_unknown_source_date = config_bool(config, "allow_unknown_source_date", true);
    let scrape_delay_ms = config_u32(config, "scrape_delay_ms", 1000);

    for seed in seeds {
        let Some(platform) = scrape_platform_for_seed(seed) else {
            continue;
        };
        if filter_synthetic_terms(&seed.query).is_err() {
            continue;
        }

        let request_url = build_scrape_request(&base_url, platform, &seed.query, &region)
            .map_err(|error| Error::RustError(error.to_string()))?;
        let params = json!({
            "cloneId": clone_id,
            "userId": user_id,
            "platform": seed.platform,
            "query": seed.query,
            "requestType": scrape_platform_name(platform),
            "region": region,
        });
        let source_id = upsert_discovery_source(db, &request_url, &params, &now).await?;
        let raw = match fetch_scrapecreators_json(&request_url, &api_key).await {
            Ok(raw) => raw,
            Err(error) => {
                mark_discovery_source_failed(db, &source_id, &params, &error.to_string(), &now)
                    .await?;
                if scrape_delay_ms > 0 {
                    Delay::from(Duration::from_millis(scrape_delay_ms as u64)).await;
                }
                continue;
            }
        };
        let items = normalize_discovery_items(platform, &raw);
        insert_discovery_items(
            db,
            &source_id,
            items,
            &raw,
            &now,
            freshness_years,
            allow_unknown_source_date,
        )
        .await?;
        mark_discovery_source_fresh(db, &source_id, &params, &now).await?;

        if scrape_delay_ms > 0 {
            Delay::from(Duration::from_millis(scrape_delay_ms as u64)).await;
        }
    }

    Ok(())
}

async fn upsert_discovery_source(
    db: &D1Database,
    source: &str,
    params: &Value,
    now: &str,
) -> WorkerResult<String> {
    let params_json = params.to_string();
    let source_id = deterministic_id(
        "discovery_source",
        &["scrapecreators", source, &params_json],
    );
    db::exec(
        db,
        r#"
        INSERT INTO discovery_sources (
          id, provider, source, params_json, refreshed_at, status
        )
        VALUES (?, 'scrapecreators', ?, ?, ?, 'refreshing')
        ON CONFLICT(id) DO UPDATE SET
          provider = 'scrapecreators',
          source = excluded.source,
          params_json = excluded.params_json,
          refreshed_at = excluded.refreshed_at,
          status = 'refreshing'
        "#,
        vec![
            json!(source_id),
            json!(source),
            json!(params_json),
            json!(now),
        ],
    )
    .await?;

    Ok(source_id)
}

async fn mark_discovery_source_fresh(
    db: &D1Database,
    source_id: &str,
    params: &Value,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE discovery_sources
        SET status = 'fresh',
            refreshed_at = ?,
            params_json = ?
        WHERE id = ?
        "#,
        vec![json!(now), json!(params.to_string()), json!(source_id)],
    )
    .await
}

async fn mark_discovery_source_failed(
    db: &D1Database,
    source_id: &str,
    params: &Value,
    error: &str,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE discovery_sources
        SET status = 'failed',
            refreshed_at = ?,
            params_json = ?
        WHERE id = ?
        "#,
        vec![
            json!(now),
            json!(failed_source_params_json(params, error, now)),
            json!(source_id),
        ],
    )
    .await
}

async fn insert_discovery_items(
    db: &D1Database,
    source_id: &str,
    items: Vec<NormalizedDiscoveryItem>,
    raw: &Value,
    now: &str,
    freshness_years: i64,
    allow_unknown_source_date: bool,
) -> WorkerResult<()> {
    for item in items {
        if item.title.trim().is_empty() || filter_synthetic_terms(&item.title).is_err() {
            continue;
        }
        if classify_freshness(
            item.source_published_at.as_deref(),
            allow_unknown_source_date,
            now,
            freshness_years,
        ) == FreshnessDecision::TooOld
        {
            continue;
        }

        let item_id = deterministic_id(
            "discovery_item",
            &[source_id, &item.platform, &item.external_id],
        );
        db::exec(
            db,
            r#"
            INSERT INTO discovery_items (
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
            VALUES (?, ?, ?, ?, 'short_video', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(source_id, platform, external_id) DO UPDATE SET
              media_type = excluded.media_type,
              title = excluded.title,
              author_handle = excluded.author_handle,
              thumbnail_url = excluded.thumbnail_url,
              image_url = excluded.image_url,
              source_url = excluded.source_url,
              source_published_at = excluded.source_published_at,
              metrics_json = excluded.metrics_json,
              raw_json = excluded.raw_json,
              discovered_at = excluded.discovered_at
            "#,
            vec![
                json!(item_id),
                json!(source_id),
                json!(item.external_id),
                json!(item.platform),
                json!(item.title),
                json!(item.author_handle),
                json!(item.image_url),
                json!(item.image_url),
                json!(item.source_url),
                json!(item.source_published_at),
                json!(json!({ "likes": item.like_count }).to_string()),
                json!(raw.to_string()),
                json!(now),
                json!(now),
            ],
        )
        .await?;
    }
    Ok(())
}

async fn run_knowledge_and_clustering(
    db: &D1Database,
    ai: &Ai,
    user_id: &str,
    clone_id: &str,
    active_niche: &str,
    allowed_platforms: &[String],
    config: &HashMap<String, String>,
) -> WorkerResult<Vec<SeedQuery>> {
    let items = load_clone_discovery_items(db, clone_id, allowed_platforms, 120).await?;
    let now = now_iso_string();
    let freshness_years = config_u32(config, "freshness_window_years", 5) as i64;
    let allow_unknown_source_date = config_bool(config, "allow_unknown_source_date", true);
    let context_items = items
        .iter()
        .filter(|item| filter_synthetic_terms(&item.title).is_ok())
        .filter(|item| {
            classify_freshness(
                item.source_published_at.as_deref(),
                allow_unknown_source_date,
                &now,
                freshness_years,
            ) != FreshnessDecision::TooOld
        })
        .map(|item| {
            json!({
                "platform": item.platform,
                "title": item.title,
                "sourceUrl": item.source_url,
                "sourcePublishedAt": item.source_published_at,
            })
        })
        .collect::<Vec<_>>();
    if context_items.is_empty() {
        return Ok(Vec::new());
    }

    let knowledge_prompt = format!(
        "{}\nDiscovery context JSON:\n{}",
        knowledge_extraction_prompt(active_niche),
        serde_json::to_string_pretty(&context_items).unwrap_or_else(|_| "[]".to_string())
    );
    let knowledge = run_text_json::<KnowledgeExtractionResponse>(ai, &knowledge_prompt).await?;
    let deeper_from_knowledge = knowledge_seed_queries(&knowledge, allowed_platforms);
    insert_knowledge_rows(db, user_id, clone_id, &knowledge).await?;

    let seeds_json = research_seeds_for_clustering(db, clone_id).await?;
    let clusters =
        run_text_json::<ClusterResponse>(ai, &clustering_prompt(active_niche, &seeds_json)).await?;
    update_clusters(db, clone_id, &clusters).await?;

    let threshold = config_f64(config, "cluster_relevance_threshold", 0.72);
    let expand_limit = config_u32(config, "expand_clusters_per_run", 4) as usize;
    let mut expanded = deeper_from_knowledge;
    for cluster in clusters
        .clusters
        .iter()
        .filter(|cluster| cluster.relevance_score >= threshold)
        .take(expand_limit)
    {
        expanded.extend(cluster_seed_queries(cluster, allowed_platforms));
    }
    Ok(dedupe_seed_queries(expanded))
}

async fn load_clone_discovery_items(
    db: &D1Database,
    clone_id: &str,
    allowed_platforms: &[String],
    limit: u32,
) -> WorkerResult<Vec<DiscoveryItemRow>> {
    if allowed_platforms.is_empty() {
        return Ok(Vec::new());
    }

    db::all(
        db,
        clone_discovery_items_sql(),
        vec![
            json!(clone_id),
            json!(clone_id),
            json!(discovery_platform_filter_json(allowed_platforms)),
            json!(limit),
        ],
    )
    .await
}

fn clone_discovery_items_sql() -> &'static str {
    r#"
        SELECT
          di.id,
          di.platform,
          di.title,
          di.image_url,
          di.thumbnail_url,
          di.source_url,
          di.source_published_at,
          di.metrics_json,
          CAST(json_extract(di.metrics_json, '$.likes') AS INTEGER) AS like_count,
          (
            SELECT nrq.cluster
            FROM niche_research_queries nrq
            WHERE nrq.clone_id = ?
              AND lower(nrq.query) = lower(json_extract(ds.params_json, '$.query'))
              AND nrq.cluster IS NOT NULL
            LIMIT 1
          ) AS niche_cluster
        FROM discovery_items di
        INNER JOIN discovery_sources ds
          ON ds.id = di.source_id
        WHERE json_extract(ds.params_json, '$.cloneId') = ?
          AND di.platform IN (SELECT value FROM json_each(?))
        ORDER BY COALESCE(di.source_published_at, di.discovered_at) DESC
        LIMIT ?
        "#
}

fn discovery_platform_filter_json(allowed_platforms: &[String]) -> String {
    serde_json::to_string(&normalize_platforms(allowed_platforms))
        .unwrap_or_else(|_| "[]".to_string())
}

async fn insert_knowledge_rows(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    knowledge: &KnowledgeExtractionResponse,
) -> WorkerResult<()> {
    let now = now_iso_string();
    for source in [
        ("signal", &knowledge.signals),
        ("avoid", &knowledge.avoid),
        ("source_note", &knowledge.source_notes),
    ] {
        for value in source.1 {
            let Some(bit) = knowledge_bit_from_value(value, source.0) else {
                continue;
            };
            if filter_synthetic_terms(&bit).is_err() {
                continue;
            }
            let id = deterministic_id("niche_knowledge", &[clone_id, &bit]);
            db::exec(
                db,
                r#"
                INSERT OR IGNORE INTO niche_knowledge (
                  id, user_id, clone_id, bit, source_platform, source_url, score, raw_json, created_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                vec![
                    json!(id),
                    json!(user_id),
                    json!(clone_id),
                    json!(bit),
                    json!(value_text(value, "platform")),
                    json!(value_text(value, "sourceUrl").or_else(|| value_text(value, "source_url"))),
                    json!(value_f64(value, "score").unwrap_or(1.0)),
                    json!(value.to_string()),
                    json!(now),
                ],
            )
            .await?;
        }
    }
    Ok(())
}

async fn research_seeds_for_clustering(db: &D1Database, clone_id: &str) -> WorkerResult<String> {
    let queries = db::all::<ResearchSeedRow>(
        db,
        r#"
        SELECT query AS value
        FROM niche_research_queries
        WHERE clone_id = ?
        UNION ALL
        SELECT bit AS value
        FROM niche_knowledge
        WHERE clone_id = ?
        LIMIT 200
        "#,
        vec![json!(clone_id), json!(clone_id)],
    )
    .await?;
    Ok(
        serde_json::to_string(&queries.into_iter().map(|row| row.value).collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".to_string()),
    )
}

async fn update_clusters(
    db: &D1Database,
    clone_id: &str,
    clusters: &ClusterResponse,
) -> WorkerResult<()> {
    for cluster in &clusters.clusters {
        let label = cluster.label.trim();
        if label.is_empty() || filter_synthetic_terms(label).is_err() {
            continue;
        }
        let reason = cluster_reason(cluster);
        for term in string_array_from_value(&cluster.terms) {
            if term.trim().is_empty() || filter_synthetic_terms(&term).is_err() {
                continue;
            }
            db::exec(
                db,
                r#"
                UPDATE niche_research_queries
                SET cluster = ?,
                    cluster_relevance_score = ?,
                    cluster_relevance_reason = ?
                WHERE clone_id = ?
                  AND lower(query) = lower(?)
                "#,
                vec![
                    json!(label),
                    json!(cluster.relevance_score),
                    json!(reason),
                    json!(clone_id),
                    json!(term),
                ],
            )
            .await?;
            db::exec(
                db,
                r#"
                UPDATE niche_knowledge
                SET cluster = ?,
                    cluster_relevance_score = ?,
                    cluster_relevance_reason = ?
                WHERE clone_id = ?
                  AND lower(bit) LIKE lower(?)
                "#,
                vec![
                    json!(label),
                    json!(cluster.relevance_score),
                    json!(reason),
                    json!(clone_id),
                    json!(format!("%{term}%")),
                ],
            )
            .await?;
        }
    }
    Ok(())
}

async fn run_visual_reference_selection(
    db: &D1Database,
    ai: &Ai,
    user_id: &str,
    clone_id: &str,
    moderation_level: u8,
    allowed_platforms: &[String],
    config: &HashMap<String, String>,
) -> WorkerResult<()> {
    let items = load_clone_discovery_items(db, clone_id, allowed_platforms, 200).await?;
    let now = now_iso_string();
    let freshness_years = config_u32(config, "freshness_window_years", 5) as i64;
    let allow_unknown_source_date = config_bool(config, "allow_unknown_source_date", false);
    let prompt = human_presence_prompt();

    for item in items {
        let Some(image_url) = item
            .image_url
            .as_deref()
            .or(item.thumbnail_url.as_deref())
            .map(str::trim)
            .filter(|url| !url.is_empty())
        else {
            continue;
        };
        if !meets_like_threshold(config, &item.platform, item.like_count) {
            continue;
        }

        let freshness = classify_freshness(
            item.source_published_at.as_deref(),
            allow_unknown_source_date,
            &now,
            freshness_years,
        );
        let candidate_id =
            insert_visual_candidate(db, user_id, clone_id, &item, image_url, &freshness, &now)
                .await?;
        if freshness == FreshnessDecision::TooOld || freshness == FreshnessDecision::UnknownRejected
        {
            mark_candidate_rejected(
                db,
                &candidate_id,
                &freshness_status(&freshness),
                "stale_or_unknown_source_date",
                &now,
            )
            .await?;
            continue;
        }
        let visual_reference_id = visual_reference_id_for(clone_id, &candidate_id);
        if visual_reference_exists(db, clone_id, &candidate_id).await? {
            insert_inspiration_pool_row(
                db,
                user_id,
                clone_id,
                &visual_reference_id,
                &item.id,
                1.0,
                &now,
            )
            .await?;
            continue;
        }

        let review = match run_vision_json::<HumanPresenceReview>(ai, &prompt, image_url).await {
            Ok(review) => review,
            Err(error) => {
                mark_candidate_rejected(
                    db,
                    &candidate_id,
                    &freshness_status(&freshness),
                    &format!("human_presence_review_failed:{error}"),
                    &now,
                )
                .await?;
                continue;
            }
        };
        if let Err(reason) = can_accept_human_presence(&review) {
            mark_candidate_reviewed(db, &candidate_id, "rejected", Some(reason), &review, &now)
                .await?;
            continue;
        }

        mark_candidate_reviewed(db, &candidate_id, "accepted", None, &review, &now).await?;
        let visual_reference_id = insert_visual_reference(
            db,
            user_id,
            clone_id,
            &candidate_id,
            &item,
            &review,
            moderation_level,
            &now,
        )
        .await?;
        insert_inspiration_pool_row(
            db,
            user_id,
            clone_id,
            &visual_reference_id,
            &item.id,
            review.organic_photo_score + review.freshness_visual_score,
            &now,
        )
        .await?;
    }

    Ok(())
}

async fn insert_visual_candidate(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    item: &DiscoveryItemRow,
    image_url: &str,
    freshness: &FreshnessDecision,
    now: &str,
) -> WorkerResult<String> {
    let candidate_id = deterministic_id("visual_candidate", &[clone_id, &item.id]);
    db::exec(
        db,
        r#"
        INSERT OR IGNORE INTO visual_reference_candidates (
          id,
          user_id,
          clone_id,
          discovery_item_id,
          source_platform,
          source_url,
          source_published_at,
          freshness_status,
          image_url,
          human_presence_status,
          niche_cluster,
          metadata_json,
          created_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'unreviewed', ?, ?, ?)
        "#,
        vec![
            json!(candidate_id),
            json!(user_id),
            json!(clone_id),
            json!(item.id),
            json!(item.platform),
            json!(item.source_url),
            json!(item.source_published_at),
            json!(freshness_status(freshness)),
            json!(image_url),
            json!(item.niche_cluster),
            json!(json!({
                "title": item.title,
                "metrics": item.metrics_json,
            })
            .to_string()),
            json!(now),
        ],
    )
    .await?;
    Ok(candidate_id)
}

async fn mark_candidate_rejected(
    db: &D1Database,
    candidate_id: &str,
    freshness_status_value: &str,
    rejection_reason: &str,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE visual_reference_candidates
        SET freshness_status = ?,
            human_presence_status = 'rejected',
            rejection_reason = ?,
            reviewed_at = ?
        WHERE id = ?
        "#,
        vec![
            json!(freshness_status_value),
            json!(rejection_reason),
            json!(now),
            json!(candidate_id),
        ],
    )
    .await
}

async fn mark_candidate_reviewed(
    db: &D1Database,
    candidate_id: &str,
    status: &str,
    rejection_reason: Option<&str>,
    review: &HumanPresenceReview,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE visual_reference_candidates
        SET human_presence_status = ?,
            human_presence_score = ?,
            organic_photo_score = ?,
            freshness_visual_score = ?,
            capture_style = ?,
            rejection_reason = ?,
            reviewed_at = ?
        WHERE id = ?
        "#,
        vec![
            json!(status),
            json!(review.confidence),
            json!(review.organic_photo_score),
            json!(review.freshness_visual_score),
            json!(review.capture_style),
            json!(rejection_reason),
            json!(now),
            json!(candidate_id),
        ],
    )
    .await
}

async fn visual_reference_exists(
    db: &D1Database,
    clone_id: &str,
    candidate_id: &str,
) -> WorkerResult<bool> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM visual_references
        WHERE clone_id = ?
          AND candidate_id = ?
        "#,
        vec![json!(clone_id), json!(candidate_id)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0) > 0)
}

async fn insert_visual_reference(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    candidate_id: &str,
    item: &DiscoveryItemRow,
    review: &HumanPresenceReview,
    moderation_level: u8,
    now: &str,
) -> WorkerResult<String> {
    let id = visual_reference_id_for(clone_id, candidate_id);
    db::exec(
        db,
        r#"
        INSERT OR IGNORE INTO visual_references (
          id,
          user_id,
          clone_id,
          candidate_id,
          source_platform,
          source_url,
          source_published_at,
          aesthetic_tags_json,
          niche_cluster,
          human_presence_type,
          human_presence_score,
          organic_photo_score,
          freshness_visual_score,
          moderation_level,
          status,
          created_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?)
        "#,
        vec![
            json!(id),
            json!(user_id),
            json!(clone_id),
            json!(candidate_id),
            json!(item.platform),
            json!(item.source_url),
            json!(item.source_published_at),
            json!(
                serde_json::to_string(&review.aesthetic_tags).unwrap_or_else(|_| "[]".to_string())
            ),
            json!(item.niche_cluster),
            json!(review.human_type),
            json!(review.confidence),
            json!(review.organic_photo_score),
            json!(review.freshness_visual_score),
            json!(moderation_level),
            json!(now),
        ],
    )
    .await?;
    Ok(id)
}

async fn insert_inspiration_pool_row(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    visual_reference_id: &str,
    discovery_item_id: &str,
    score: f64,
    now: &str,
) -> WorkerResult<()> {
    let id = deterministic_id("inspiration_pool", &[clone_id, visual_reference_id]);
    db::exec(
        db,
        r#"
        INSERT OR IGNORE INTO user_inspiration_pool (
          id, user_id, clone_id, visual_reference_id, discovery_item_id, score, created_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        vec![
            json!(id),
            json!(user_id),
            json!(clone_id),
            json!(visual_reference_id),
            json!(discovery_item_id),
            json!(score),
            json!(now),
        ],
    )
    .await
}

async fn finalize_research_pool(
    db: &D1Database,
    env: &Env,
    clone: &CloneResearchRow,
    clone_id: &str,
    config: &HashMap<String, String>,
) -> WorkerResult<()> {
    let min_visual_refs = config_u32(config, "min_visual_refs", 5);
    let count = active_visual_reference_count(db, clone_id).await?;
    let detail_suffix = if serde_json::from_str::<Value>(&clone.provider_config_json).is_ok() {
        ""
    } else {
        ", provider_config_was_invalid=true"
    };
    if count < min_visual_refs {
        set_clone_research_status(
            db,
            &clone.user_id,
            clone_id,
            "insufficient_refs",
            &format!("active_refs={count}, minimum={min_visual_refs}{detail_suffix}"),
        )
        .await?;
        return Ok(());
    }

    if clone.soul_status == "ready" {
        if let Some(provider_soul_id) = clone.provider_soul_id.as_deref() {
            create_next_batch(db, env, &clone.user_id, clone_id, provider_soul_id).await?;
            return Ok(());
        }
    }

    set_clone_research_status(
        db,
        &clone.user_id,
        clone_id,
        "pool_ready_awaiting_soul",
        &format!("active_refs={count}, minimum={min_visual_refs}{detail_suffix}"),
    )
    .await
}

async fn active_visual_reference_count(db: &D1Database, clone_id: &str) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM visual_references
        WHERE clone_id = ?
          AND status = 'active'
        "#,
        vec![json!(clone_id)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResearchPhase {
    Queued,
    Scraping,
    Reviewing,
    PoolReady,
    PartialPoolReady,
    InsufficientRefs,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResearchStatusWriteMode {
    Normal,
    Failure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResearchStatusWriteResult {
    Written,
    SkippedStale,
    SkippedRaced,
    MissingClone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResearchStatusSnapshot<'a> {
    status: Option<&'a str>,
    run_id: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResearchStatusWriteDecision {
    Write,
    SkipStale,
    MissingClone,
}

fn research_status_for_phase(phase: ResearchPhase) -> &'static str {
    match phase {
        ResearchPhase::Queued => "queued",
        ResearchPhase::Scraping => "scraping",
        ResearchPhase::Reviewing => "reviewing",
        ResearchPhase::PoolReady => "pool_ready",
        ResearchPhase::PartialPoolReady => "partial_pool_ready",
        ResearchPhase::InsufficientRefs => "insufficient_refs",
        ResearchPhase::Failed => "research_failed",
    }
}

fn research_phase_from_status(status: &str) -> Option<ResearchPhase> {
    match status.trim() {
        "queued" => Some(ResearchPhase::Queued),
        "scraping" => Some(ResearchPhase::Scraping),
        "reviewing" => Some(ResearchPhase::Reviewing),
        "pool_ready" => Some(ResearchPhase::PoolReady),
        "partial_pool_ready" => Some(ResearchPhase::PartialPoolReady),
        "insufficient_refs" => Some(ResearchPhase::InsufficientRefs),
        "research_failed" => Some(ResearchPhase::Failed),
        _ => None,
    }
}

fn research_status_transition_allowed(current: Option<&str>, next: &str) -> bool {
    let Some(next_phase) = research_phase_from_status(next) else {
        return true;
    };
    if next_phase == ResearchPhase::Queued {
        return true;
    }

    let Some(current_phase) = current.and_then(research_phase_from_status) else {
        return true;
    };

    match current_phase {
        ResearchPhase::Queued => true,
        ResearchPhase::Scraping => true,
        ResearchPhase::Reviewing => matches!(
            next_phase,
            ResearchPhase::Reviewing
                | ResearchPhase::PoolReady
                | ResearchPhase::PartialPoolReady
                | ResearchPhase::InsufficientRefs
                | ResearchPhase::Failed
        ),
        ResearchPhase::PoolReady
        | ResearchPhase::PartialPoolReady
        | ResearchPhase::InsufficientRefs
        | ResearchPhase::Failed => false,
    }
}

#[cfg(test)]
fn research_failure_status_transition_allowed(current: Option<&str>, next: &str) -> bool {
    if research_phase_from_status(next) != Some(ResearchPhase::Failed) {
        return research_status_transition_allowed(current, next);
    }

    let Some(current_phase) = current.and_then(research_phase_from_status) else {
        return true;
    };

    matches!(
        current_phase,
        ResearchPhase::Scraping | ResearchPhase::Reviewing
    )
}

fn research_failure_status_transition_allowed_for_run(
    current: ResearchStatusSnapshot<'_>,
    next: &str,
    expected_run_id: Option<&str>,
) -> bool {
    if research_phase_from_status(next) != Some(ResearchPhase::Failed) {
        return research_status_transition_allowed(current.status, next);
    }

    let Some(current_phase) = current.status.and_then(research_phase_from_status) else {
        return true;
    };

    match current_phase {
        ResearchPhase::Queued => expected_run_id.is_some() && current.run_id == expected_run_id,
        ResearchPhase::Scraping | ResearchPhase::Reviewing => true,
        ResearchPhase::PoolReady
        | ResearchPhase::PartialPoolReady
        | ResearchPhase::InsufficientRefs
        | ResearchPhase::Failed => false,
    }
}

fn research_status_write_decision(
    current: Option<ResearchStatusSnapshot<'_>>,
    next: &str,
    mode: ResearchStatusWriteMode,
    expected_run_id: Option<&str>,
) -> ResearchStatusWriteDecision {
    let Some(current) = current else {
        return ResearchStatusWriteDecision::MissingClone;
    };
    let starts_new_run = research_phase_from_status(next) == Some(ResearchPhase::Queued);
    if !starts_new_run {
        match mode {
            ResearchStatusWriteMode::Normal => {
                if let Some(current_run_id) = current.run_id {
                    if expected_run_id != Some(current_run_id) {
                        return ResearchStatusWriteDecision::SkipStale;
                    }
                } else if expected_run_id.is_some() {
                    return ResearchStatusWriteDecision::SkipStale;
                }
            }
            ResearchStatusWriteMode::Failure => {
                if let Some(expected_run_id) = expected_run_id {
                    if current.run_id != Some(expected_run_id) {
                        return ResearchStatusWriteDecision::SkipStale;
                    }
                } else if current.run_id.is_some() {
                    return ResearchStatusWriteDecision::SkipStale;
                }
            }
        }
    }

    let transition_allowed = match mode {
        ResearchStatusWriteMode::Normal => research_status_transition_allowed(current.status, next),
        ResearchStatusWriteMode::Failure => {
            research_failure_status_transition_allowed_for_run(current, next, expected_run_id)
        }
    };
    if transition_allowed {
        ResearchStatusWriteDecision::Write
    } else {
        ResearchStatusWriteDecision::SkipStale
    }
}

fn normal_status_write_action(result: ResearchStatusWriteResult) -> QueueMessageAction {
    match result {
        ResearchStatusWriteResult::Written
        | ResearchStatusWriteResult::SkippedStale
        | ResearchStatusWriteResult::MissingClone => QueueMessageAction::Ack,
        ResearchStatusWriteResult::SkippedRaced => QueueMessageAction::Retry,
    }
}

fn research_status_result_from_changed_rows(changed_rows: usize) -> ResearchStatusWriteResult {
    if changed_rows > 0 {
        ResearchStatusWriteResult::Written
    } else {
        ResearchStatusWriteResult::SkippedRaced
    }
}

fn queue_error_code(error: &str) -> &'static str {
    let normalized = error.to_ascii_lowercase();
    if is_scrapecreators_retryable_error(&normalized) {
        "scrapecreators_retryable"
    } else if crate::ai::workers_ai::is_workers_ai_upstream_timeout(&normalized) {
        "ai_upstream_timeout"
    } else {
        "research_message_failed"
    }
}

fn is_scrapecreators_retryable_error(normalized: &str) -> bool {
    normalized.contains("scrapecreators")
        && (normalized.contains("status 429")
            || normalized.contains("status 500")
            || normalized.contains("status 502")
            || normalized.contains("status 503")
            || normalized.contains("status 504"))
}

fn compact_error_detail(error: &str) -> String {
    const MAX_DETAIL_LENGTH: usize = 240;
    let compact = error.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(MAX_DETAIL_LENGTH).collect()
}

fn new_research_run_id() -> String {
    format!("niche_run_{}", uuid::Uuid::new_v4().simple())
}

fn changed_rows(result: &worker::D1Result) -> WorkerResult<usize> {
    Ok(result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or_default())
}

async fn load_config_map(db: &D1Database) -> WorkerResult<HashMap<String, String>> {
    let rows = db::all::<ConfigRow>(
        db,
        r#"
        SELECT key, value
        FROM blitz_config
        "#,
        vec![],
    )
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.key, row.value))
        .collect::<HashMap<_, _>>())
}

fn config_u32(config: &HashMap<String, String>, key: &str, default: u32) -> u32 {
    config
        .get(key)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}

fn config_bool(config: &HashMap<String, String>, key: &str, default: bool) -> bool {
    config
        .get(key)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
        .unwrap_or(default)
}

fn config_f64(config: &HashMap<String, String>, key: &str, default: f64) -> f64 {
    config
        .get(key)
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(default)
}

async fn load_clone_research_state(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
) -> WorkerResult<Option<ResearchStatusRow>> {
    let row = db::first::<ResearchStatusRow>(
        db,
        r#"
        SELECT
          CASE
            WHEN json_valid(provider_config_json)
              THEN CAST(json_extract(provider_config_json, '$.nicheResearchStatus') AS TEXT)
            ELSE NULL
          END AS status,
          CASE
            WHEN json_valid(provider_config_json)
              THEN CAST(json_extract(provider_config_json, '$.nicheResearchRunId') AS TEXT)
            ELSE NULL
          END AS run_id
        FROM clone_profiles
        WHERE user_id = ?
          AND id = ?
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        vec![json!(user_id), json!(clone_id)],
    )
    .await?;
    Ok(row)
}

async fn set_clone_research_status(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    status: &str,
    detail: &str,
) -> WorkerResult<()> {
    set_clone_research_status_with_run(db, user_id, clone_id, status, detail, None, None).await
}

async fn set_clone_research_status_with_run(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    status: &str,
    detail: &str,
    expected_run_id: Option<&str>,
    run_id_to_store: Option<&str>,
) -> WorkerResult<()> {
    let result = write_clone_research_status(
        db,
        user_id,
        clone_id,
        status,
        detail,
        ResearchStatusWriteMode::Normal,
        expected_run_id,
        run_id_to_store,
    )
    .await?;
    match normal_status_write_action(result) {
        QueueMessageAction::Ack => Ok(()),
        QueueMessageAction::Retry => {
            Err(Error::RustError(NICHE_RESEARCH_STATUS_CAS_MISS.to_string()))
        }
    }
}

async fn write_clone_research_status(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    status: &str,
    detail: &str,
    mode: ResearchStatusWriteMode,
    expected_run_id: Option<&str>,
    run_id_to_store: Option<&str>,
) -> WorkerResult<ResearchStatusWriteResult> {
    let current_state = load_clone_research_state(db, user_id, clone_id).await?;
    let current = current_state.as_ref().map(|state| ResearchStatusSnapshot {
        status: state.status.as_deref(),
        run_id: state.run_id.as_deref(),
    });
    match research_status_write_decision(current, status, mode, expected_run_id) {
        ResearchStatusWriteDecision::Write => {}
        ResearchStatusWriteDecision::MissingClone => {
            web_sys::console::log_1(
                &format!("skip niche research status write for missing clone next={status}").into(),
            );
            return Ok(ResearchStatusWriteResult::MissingClone);
        }
        ResearchStatusWriteDecision::SkipStale => {
            web_sys::console::log_1(
                &format!(
                    "skip stale niche research status transition current={} next={} current_run={} expected_run={}",
                    current.and_then(|state| state.status).unwrap_or(""),
                    status,
                    current.and_then(|state| state.run_id).unwrap_or(""),
                    expected_run_id.unwrap_or("")
                )
                .into(),
            );
            return Ok(ResearchStatusWriteResult::SkippedStale);
        }
    }

    let current = current.expect("write decision requires current state");
    let starts_new_run = research_phase_from_status(status) == Some(ResearchPhase::Queued);
    let next_run_id = if starts_new_run {
        run_id_to_store
    } else {
        run_id_to_store.or(expected_run_id).or(current.run_id)
    };

    let now = now_iso_string();
    let result = db::run(
        db,
        r#"
        UPDATE clone_profiles
        SET provider_config_json = json_set(
              CASE
                WHEN json_valid(provider_config_json) THEN provider_config_json
                ELSE '{}'
              END,
              '$.nicheResearchStatus',
              ?,
              '$.nicheResearchDetail',
              ?,
              '$.nicheResearchUpdatedAt',
              ?,
              '$.nicheResearchRunId',
              ?
            ),
            updated_at = ?
        WHERE user_id = ?
          AND id = ?
          AND deleted_at IS NULL
          AND (
            (
              ? IS NULL
              AND (
                CASE
                  WHEN json_valid(provider_config_json)
                    THEN CAST(json_extract(provider_config_json, '$.nicheResearchStatus') AS TEXT)
                  ELSE NULL
                END
              ) IS NULL
            )
            OR (
              ? IS NOT NULL
              AND (
                CASE
                  WHEN json_valid(provider_config_json)
                    THEN CAST(json_extract(provider_config_json, '$.nicheResearchStatus') AS TEXT)
                  ELSE NULL
                END
              ) = ?
            )
          )
          AND (
            (
              ? IS NULL
              AND (
                CASE
                  WHEN json_valid(provider_config_json)
                    THEN CAST(json_extract(provider_config_json, '$.nicheResearchRunId') AS TEXT)
                  ELSE NULL
                END
              ) IS NULL
            )
            OR (
              ? IS NOT NULL
              AND (
                CASE
                  WHEN json_valid(provider_config_json)
                    THEN CAST(json_extract(provider_config_json, '$.nicheResearchRunId') AS TEXT)
                  ELSE NULL
                END
              ) = ?
            )
          )
        "#,
        vec![
            json!(status),
            json!(detail),
            json!(now),
            json!(next_run_id),
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(current.status),
            json!(current.status),
            json!(current.status),
            json!(current.run_id),
            json!(current.run_id),
            json!(current.run_id),
        ],
    )
    .await?;

    let result = research_status_result_from_changed_rows(changed_rows(&result)?);
    if result == ResearchStatusWriteResult::SkippedRaced {
        web_sys::console::log_1(
            &format!(
                "skip raced niche research status transition current={} next={} current_run={} expected_run={}",
                current.status.unwrap_or(""),
                status,
                current.run_id.unwrap_or(""),
                expected_run_id.unwrap_or("")
            )
            .into(),
        );
    }
    Ok(result)
}

fn active_niche_from_moodboards(moodboards: &[MoodboardRow]) -> String {
    moodboards
        .iter()
        .map(|moodboard| {
            format!(
                "{}: {}",
                moodboard.title.trim(),
                moodboard.vibe_summary.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn moodboard_search_queries(moodboards: &[MoodboardRow]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut queries = Vec::new();
    for moodboard in moodboards {
        let Ok(values) = serde_json::from_str::<Vec<String>>(&moodboard.search_queries_json) else {
            continue;
        };
        for value in values {
            let normalized = value.trim().to_ascii_lowercase();
            if !normalized.is_empty() && seen.insert(normalized) {
                queries.push(value.trim().to_string());
            }
        }
    }
    queries
}

fn valid_loaded_moodboard_count(count: usize) -> bool {
    count == 5
}

fn normalize_platforms(platforms: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    platforms
        .iter()
        .map(|platform| platform.trim().to_ascii_lowercase())
        .filter(|platform| platform == "tiktok" || platform == "instagram")
        .filter(|platform| seen.insert(platform.clone()))
        .collect()
}

fn accepted_seed_queries(
    seeds: Vec<SeedCandidate>,
    allowed_platforms: &[String],
) -> Vec<SeedQuery> {
    let allowed = allowed_platforms.iter().collect::<HashSet<_>>();
    seeds
        .into_iter()
        .filter_map(|seed| {
            let query = seed.query.trim();
            let platform = seed.platform.trim().to_ascii_lowercase();
            if query.is_empty()
                || !allowed.contains(&platform)
                || filter_synthetic_terms(query).is_err()
            {
                return None;
            }
            Some(SeedQuery {
                query: query.to_string(),
                platform,
                source: "seed_extraction".to_string(),
                raw_json: serde_json::to_value(seed).unwrap_or_else(|_| json!({})),
            })
        })
        .collect()
}

fn fallback_moodboard_seed_queries(
    queries: &[String],
    allowed_platforms: &[String],
) -> Vec<SeedQuery> {
    queries
        .iter()
        .filter(|query| filter_synthetic_terms(query).is_ok())
        .flat_map(|query| {
            allowed_platforms.iter().map(move |platform| SeedQuery {
                query: query.trim().to_string(),
                platform: platform.clone(),
                source: "moodboard_seed".to_string(),
                raw_json: json!({
                    "term": query,
                    "platform": platform,
                    "source": "moodboard_seed"
                }),
            })
        })
        .collect()
}

fn cap_seed_queries_per_platform(seeds: Vec<SeedQuery>, max_per_platform: usize) -> Vec<SeedQuery> {
    if max_per_platform == 0 {
        return Vec::new();
    }

    let mut counts = HashMap::<String, usize>::new();
    seeds
        .into_iter()
        .filter(|seed| {
            let count = counts.entry(seed.platform.clone()).or_insert(0);
            if *count >= max_per_platform {
                return false;
            }
            *count += 1;
            true
        })
        .collect()
}

fn scrape_platform_for_seed(seed: &SeedQuery) -> Option<ScrapePlatform> {
    match seed.platform.as_str() {
        "tiktok" if seed.query.trim_start().starts_with('#') => Some(ScrapePlatform::TikTokHashtag),
        "tiktok" => Some(ScrapePlatform::TikTokKeyword),
        "instagram" => Some(ScrapePlatform::InstagramReels),
        _ => None,
    }
}

fn scrape_platform_name(platform: ScrapePlatform) -> &'static str {
    match platform {
        ScrapePlatform::TikTokKeyword => "tiktok_keyword",
        ScrapePlatform::TikTokHashtag => "tiktok_hashtag",
        ScrapePlatform::InstagramReels => "instagram_reels",
    }
}

fn normalize_discovery_items(
    platform: ScrapePlatform,
    raw: &Value,
) -> Vec<NormalizedDiscoveryItem> {
    match platform {
        ScrapePlatform::TikTokKeyword => normalize_tiktok_keyword_search(raw),
        ScrapePlatform::TikTokHashtag => normalize_tiktok_hashtag_search(raw),
        ScrapePlatform::InstagramReels => normalize_instagram_reels_search(raw),
    }
}

fn knowledge_seed_queries(
    knowledge: &KnowledgeExtractionResponse,
    allowed_platforms: &[String],
) -> Vec<SeedQuery> {
    knowledge
        .deeper_queries
        .iter()
        .chain(knowledge.queries.iter())
        .cloned()
        .flat_map(|seed| accepted_seed_queries(vec![seed], allowed_platforms))
        .collect()
}

fn cluster_seed_queries(
    cluster: &ClusterCandidate,
    allowed_platforms: &[String],
) -> Vec<SeedQuery> {
    let mut seeds = cluster
        .deeper_queries
        .iter()
        .cloned()
        .flat_map(|seed| accepted_seed_queries(vec![seed], allowed_platforms))
        .collect::<Vec<_>>();
    if seeds.is_empty() {
        for term in string_array_from_value(&cluster.terms) {
            if filter_synthetic_terms(&term).is_ok() {
                for platform in allowed_platforms {
                    seeds.push(SeedQuery {
                        query: term.clone(),
                        platform: platform.clone(),
                        source: "cluster_expansion".to_string(),
                        raw_json: json!({ "cluster": cluster.label, "platform": platform }),
                    });
                }
            }
        }
    }
    seeds
}

fn dedupe_seed_queries(seeds: Vec<SeedQuery>) -> Vec<SeedQuery> {
    let mut seen = HashSet::new();
    seeds
        .into_iter()
        .filter(|seed| {
            seen.insert(format!(
                "{}:{}",
                seed.platform,
                seed.query.to_ascii_lowercase()
            ))
        })
        .collect()
}

fn knowledge_bit_from_value(value: &Value, source: &str) -> Option<String> {
    if let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Some(if source == "avoid" {
            format!("Avoid: {text}")
        } else {
            text.to_string()
        });
    }
    for key in ["bit", "signal", "text", "note", "observation"] {
        if let Some(text) = value_text(value, key) {
            return Some(if source == "avoid" {
                format!("Avoid: {}", text.trim())
            } else {
                text.trim().to_string()
            });
        }
    }
    None
}

fn value_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

fn value_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn string_array_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::trim))
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
        Value::String(text) if !text.trim().is_empty() => vec![text.trim().to_string()],
        _ => Vec::new(),
    }
}

fn cluster_reason(cluster: &ClusterCandidate) -> String {
    let mut parts = Vec::new();
    if !cluster.intent.trim().is_empty() {
        parts.push(cluster.intent.trim().to_string());
    }
    if let Some(criteria) = cluster.visual_criteria.as_str() {
        if !criteria.trim().is_empty() {
            parts.push(criteria.trim().to_string());
        }
    }
    parts.join(" | ")
}

fn meets_like_threshold(
    config: &HashMap<String, String>,
    platform: &str,
    like_count: Option<u64>,
) -> bool {
    let Some(like_count) = like_count else {
        return true;
    };
    let threshold = config
        .get("platform_engagement_thresholds_json")
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| {
            value
                .get(platform)
                .and_then(|platform| platform.get("likes"))
                .and_then(Value::as_u64)
        })
        .unwrap_or(0);
    like_count >= threshold
}

fn freshness_status(freshness: &FreshnessDecision) -> String {
    serde_json::to_value(freshness)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown_rejected".to_string())
}

fn failed_source_params_json(params: &Value, error: &str, now: &str) -> String {
    let mut params = params.clone();
    let compact_error = compact_error_text(error);
    if let Some(object) = params.as_object_mut() {
        object.insert("lastError".to_string(), json!(compact_error));
        object.insert("lastErrorAt".to_string(), json!(now));
    } else {
        params = json!({
            "lastError": compact_error,
            "lastErrorAt": now,
        });
    }
    params.to_string()
}

fn compact_error_text(error: &str) -> String {
    const MAX_ERROR_CHARS: usize = 280;
    error.chars().take(MAX_ERROR_CHARS).collect()
}

fn env_var(env: &Env, key: &str, error_code: &str) -> WorkerResult<String> {
    match env.var(key) {
        Ok(value) if !value.to_string().trim().is_empty() => Ok(value.to_string()),
        _ => Err(Error::RustError(error_code.to_string())),
    }
}

fn visual_reference_id_for(clone_id: &str, candidate_id: &str) -> String {
    deterministic_id("visual_ref", &[clone_id, candidate_id])
}

fn deterministic_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    let digest = hasher.finalize();
    format!("{prefix}_{}", hex::encode(&digest[..16]))
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

#[derive(Debug, Deserialize)]
struct MoodboardRow {
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

#[derive(Debug, Deserialize)]
struct ResearchStatusRow {
    status: Option<String>,
    run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdRow {
    id: String,
}

#[derive(Debug, Deserialize)]
struct DiscoveryItemRow {
    id: String,
    platform: String,
    title: String,
    image_url: Option<String>,
    thumbnail_url: Option<String>,
    source_url: Option<String>,
    source_published_at: Option<String>,
    metrics_json: String,
    like_count: Option<u64>,
    niche_cluster: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResearchSeedRow {
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeedCandidate {
    #[serde(default, alias = "term")]
    query: String,
    #[serde(default)]
    platform: String,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Deserialize)]
struct SeedExtractionResponse {
    #[serde(default)]
    seeds: Vec<SeedCandidate>,
}

#[derive(Debug, Clone)]
struct SeedQuery {
    query: String,
    platform: String,
    source: String,
    raw_json: Value,
}

#[derive(Debug, Deserialize)]
struct KnowledgeExtractionResponse {
    #[serde(default)]
    signals: Vec<Value>,
    #[serde(default)]
    avoid: Vec<Value>,
    #[serde(default)]
    source_notes: Vec<Value>,
    #[serde(default)]
    deeper_queries: Vec<SeedCandidate>,
    #[serde(default)]
    queries: Vec<SeedCandidate>,
}

fn default_relevance_score() -> f64 {
    1.0
}

#[derive(Debug, Deserialize)]
struct ClusterCandidate {
    #[serde(default)]
    label: String,
    #[serde(default)]
    terms: Value,
    #[serde(default)]
    intent: String,
    #[serde(default)]
    visual_criteria: Value,
    #[serde(default = "default_relevance_score")]
    relevance_score: f64,
    #[serde(default)]
    deeper_queries: Vec<SeedCandidate>,
}

#[derive(Debug, Deserialize)]
struct ClusterResponse {
    #[serde(default)]
    clusters: Vec<ClusterCandidate>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn visual_reference_research_messages_serialize_as_queue_contract() {
        let message = NicheResearchMessage::ResearchMoodboardReferences {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            moodboard_ids: vec!["moodboard_1".to_string(), "moodboard_2".to_string()],
            reason: "onboarding_selection".to_string(),
        };

        assert_eq!(
            serde_json::to_value(&message).unwrap(),
            json!({
                "type": "research_moodboard_references",
                "userId": "user_1",
                "cloneId": "clone_1",
                "moodboardIds": ["moodboard_1", "moodboard_2"],
                "reason": "onboarding_selection"
            })
        );

        let parsed: NicheResearchMessage = serde_json::from_value(json!({
            "type": "research_moodboard_references",
            "userId": "user_1",
            "cloneId": "clone_1",
            "moodboardIds": ["moodboard_1", "moodboard_2"],
            "reason": "onboarding_selection"
        }))
        .unwrap();
        assert!(matches!(
            parsed,
            NicheResearchMessage::ResearchMoodboardReferences {
                user_id,
                clone_id,
                moodboard_ids,
                reason
            } if user_id == "user_1"
                && clone_id == "clone_1"
                && moodboard_ids == vec!["moodboard_1".to_string(), "moodboard_2".to_string()]
                && reason == "onboarding_selection"
        ));
    }

    #[test]
    fn optional_run_token_is_omitted_from_legacy_chunk_message_contract() {
        let message = NicheResearchMessage::FetchInstagramProfile {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            run_id: None,
            moodboard_id: "moodboard_1".to_string(),
            moodboard_slug: "flash-editorial".to_string(),
            handle: "creator".to_string(),
            discovered_via: "configured_handle".to_string(),
            related_depth: 0,
        };

        assert_eq!(
            serde_json::to_value(&message).unwrap(),
            json!({
                "type": "fetch_instagram_profile",
                "userId": "user_1",
                "cloneId": "clone_1",
                "moodboardId": "moodboard_1",
                "moodboardSlug": "flash-editorial",
                "handle": "creator",
                "discoveredVia": "configured_handle",
                "relatedDepth": 0
            })
        );

        let parsed: NicheResearchMessage = serde_json::from_value(json!({
            "type": "fetch_instagram_profile",
            "userId": "user_1",
            "cloneId": "clone_1",
            "moodboardId": "moodboard_1",
            "moodboardSlug": "flash-editorial",
            "handle": "creator",
            "discoveredVia": "configured_handle",
            "relatedDepth": 0
        }))
        .unwrap();
        assert!(matches!(
            parsed,
            NicheResearchMessage::FetchInstagramProfile { run_id, .. } if run_id.is_none()
        ));
    }

    #[test]
    fn research_statuses_match_product_contract() {
        assert_eq!(research_status_for_phase(ResearchPhase::Queued), "queued");
        assert_eq!(
            research_status_for_phase(ResearchPhase::Scraping),
            "scraping"
        );
        assert_eq!(
            research_status_for_phase(ResearchPhase::Reviewing),
            "reviewing"
        );
        assert_eq!(
            research_status_for_phase(ResearchPhase::PoolReady),
            "pool_ready"
        );
        assert_eq!(
            research_status_for_phase(ResearchPhase::PartialPoolReady),
            "partial_pool_ready"
        );
        assert_eq!(
            research_status_for_phase(ResearchPhase::InsufficientRefs),
            "insufficient_refs"
        );
        assert_eq!(
            research_status_for_phase(ResearchPhase::Failed),
            "research_failed"
        );
    }

    #[test]
    fn stale_chunk_statuses_do_not_overwrite_ready_statuses() {
        for current in ["pool_ready", "partial_pool_ready"] {
            assert!(!research_status_transition_allowed(
                Some(current),
                "scraping"
            ));
            assert!(!research_status_transition_allowed(
                Some(current),
                "reviewing"
            ));
        }
    }

    #[test]
    fn stale_chunk_statuses_do_not_overwrite_terminal_statuses() {
        for current in ["insufficient_refs", "research_failed"] {
            assert!(!research_status_transition_allowed(
                Some(current),
                "scraping"
            ));
            assert!(!research_status_transition_allowed(
                Some(current),
                "reviewing"
            ));
        }
    }

    #[test]
    fn scraping_status_does_not_overwrite_reviewing_status() {
        assert!(!research_status_transition_allowed(
            Some("reviewing"),
            "scraping"
        ));
    }

    #[test]
    fn run_token_mismatch_skips_chunk_status_and_failure_recording() {
        let current = Some(ResearchStatusSnapshot {
            status: Some("queued"),
            run_id: Some("run_new"),
        });

        assert_eq!(
            research_status_write_decision(
                current,
                "scraping",
                ResearchStatusWriteMode::Normal,
                Some("run_old")
            ),
            ResearchStatusWriteDecision::SkipStale
        );
        assert_eq!(
            research_status_write_decision(
                current,
                "research_failed",
                ResearchStatusWriteMode::Failure,
                Some("run_old")
            ),
            ResearchStatusWriteDecision::SkipStale
        );
    }

    #[test]
    fn tokenless_chunk_status_skips_tokened_active_run() {
        let current = Some(ResearchStatusSnapshot {
            status: Some("queued"),
            run_id: Some("run_active"),
        });

        assert_eq!(
            research_status_write_decision(
                current,
                "scraping",
                ResearchStatusWriteMode::Normal,
                None
            ),
            ResearchStatusWriteDecision::SkipStale
        );
        assert_eq!(
            research_status_write_decision(
                current,
                "reviewing",
                ResearchStatusWriteMode::Normal,
                None
            ),
            ResearchStatusWriteDecision::SkipStale
        );
    }

    #[test]
    fn tokenless_queued_status_can_start_new_tokened_run() {
        let current = Some(ResearchStatusSnapshot {
            status: Some("pool_ready"),
            run_id: Some("run_previous"),
        });

        assert_eq!(
            research_status_write_decision(
                current,
                "queued",
                ResearchStatusWriteMode::Normal,
                None
            ),
            ResearchStatusWriteDecision::Write
        );
    }

    #[test]
    fn current_run_token_match_permits_queued_to_scraping() {
        let current = Some(ResearchStatusSnapshot {
            status: Some("queued"),
            run_id: Some("run_1"),
        });

        assert_eq!(
            research_status_write_decision(
                current,
                "scraping",
                ResearchStatusWriteMode::Normal,
                Some("run_1")
            ),
            ResearchStatusWriteDecision::Write
        );
    }

    #[test]
    fn same_run_token_queued_failure_is_recordable() {
        let current = Some(ResearchStatusSnapshot {
            status: Some("queued"),
            run_id: Some("run_1"),
        });

        assert_eq!(
            research_status_write_decision(
                current,
                "research_failed",
                ResearchStatusWriteMode::Failure,
                Some("run_1")
            ),
            ResearchStatusWriteDecision::Write
        );
    }

    #[test]
    fn tokenless_or_mismatched_queued_failure_stays_stale() {
        let current = Some(ResearchStatusSnapshot {
            status: Some("queued"),
            run_id: Some("run_active"),
        });

        assert_eq!(
            research_status_write_decision(
                current,
                "research_failed",
                ResearchStatusWriteMode::Failure,
                None
            ),
            ResearchStatusWriteDecision::SkipStale
        );
        assert_eq!(
            research_status_write_decision(
                current,
                "research_failed",
                ResearchStatusWriteMode::Failure,
                Some("run_old")
            ),
            ResearchStatusWriteDecision::SkipStale
        );
    }

    #[test]
    fn ready_statuses_reject_same_run_failure_recording() {
        for current_status in ["pool_ready", "partial_pool_ready"] {
            let current = Some(ResearchStatusSnapshot {
                status: Some(current_status),
                run_id: Some("run_1"),
            });

            assert_eq!(
                research_status_write_decision(
                    current,
                    "research_failed",
                    ResearchStatusWriteMode::Failure,
                    Some("run_1")
                ),
                ResearchStatusWriteDecision::SkipStale
            );
        }
    }

    #[test]
    fn missing_expected_run_token_cannot_record_failure_over_active_run() {
        let current = Some(ResearchStatusSnapshot {
            status: Some("scraping"),
            run_id: Some("run_active"),
        });

        assert_eq!(
            research_status_write_decision(
                current,
                "research_failed",
                ResearchStatusWriteMode::Failure,
                None
            ),
            ResearchStatusWriteDecision::SkipStale
        );
    }

    #[test]
    fn zero_row_status_write_is_classified_as_raced_not_failed() {
        assert_eq!(
            research_status_result_from_changed_rows(0),
            ResearchStatusWriteResult::SkippedRaced
        );
        assert_eq!(
            research_status_result_from_changed_rows(1),
            ResearchStatusWriteResult::Written
        );
    }

    #[test]
    fn normal_status_write_action_retries_raced_outcome() {
        assert_eq!(
            normal_status_write_action(ResearchStatusWriteResult::Written),
            QueueMessageAction::Ack
        );
        assert_eq!(
            normal_status_write_action(ResearchStatusWriteResult::SkippedStale),
            QueueMessageAction::Ack
        );
        assert_eq!(
            normal_status_write_action(ResearchStatusWriteResult::MissingClone),
            QueueMessageAction::Ack
        );
        assert_eq!(
            normal_status_write_action(ResearchStatusWriteResult::SkippedRaced),
            QueueMessageAction::Retry
        );
    }

    #[test]
    fn failure_record_action_retries_raced_and_error_outcomes() {
        assert_eq!(
            failure_record_action(Some(FailureRecordOutcome::Recorded)),
            QueueMessageAction::Ack
        );
        assert_eq!(
            failure_record_action(Some(FailureRecordOutcome::SkippedStale)),
            QueueMessageAction::Ack
        );
        assert_eq!(
            failure_record_action(Some(FailureRecordOutcome::MissingClone)),
            QueueMessageAction::Ack
        );
        assert_eq!(
            failure_record_action(Some(FailureRecordOutcome::SkippedRaced)),
            QueueMessageAction::Retry
        );
        assert_eq!(failure_record_action(None), QueueMessageAction::Retry);
    }

    #[test]
    fn missing_clone_status_write_is_classified_as_ackable_skip() {
        assert_eq!(
            research_status_write_decision(
                None,
                "scraping",
                ResearchStatusWriteMode::Normal,
                Some("run_1")
            ),
            ResearchStatusWriteDecision::MissingClone
        );
    }

    #[test]
    fn stale_failure_status_does_not_overwrite_ready_statuses() {
        for current in ["pool_ready", "partial_pool_ready"] {
            assert!(!research_failure_status_transition_allowed(
                Some(current),
                "research_failed"
            ));
        }
    }

    #[test]
    fn stale_failure_status_does_not_overwrite_new_queued_run() {
        assert!(!research_failure_status_transition_allowed(
            Some("queued"),
            "research_failed"
        ));
    }

    #[test]
    fn active_chunk_statuses_can_record_failure_status() {
        for current in ["scraping", "reviewing"] {
            assert!(research_failure_status_transition_allowed(
                Some(current),
                "research_failed"
            ));
        }
    }

    #[test]
    fn queued_status_can_start_new_run_from_terminal_statuses() {
        for current in [
            "pool_ready",
            "partial_pool_ready",
            "insufficient_refs",
            "research_failed",
        ] {
            assert!(research_status_transition_allowed(Some(current), "queued"));
        }
    }

    #[test]
    fn retryable_error_codes_are_compact_and_stable() {
        for status in [429, 500, 502, 503, 504] {
            assert_eq!(
                queue_error_code(&format!("scrapecreators endpoint returned status {status}")),
                "scrapecreators_retryable"
            );
        }
        assert_eq!(
            queue_error_code("AiError: upstream request failed with status 504"),
            "ai_upstream_timeout"
        );
        assert_eq!(
            queue_error_code("workers ai gateway timeout"),
            "ai_upstream_timeout"
        );
        assert_eq!(
            queue_error_code("failed to decode workers ai result"),
            "research_message_failed"
        );
        assert_eq!(
            queue_error_code("failed item id 504abc"),
            "research_message_failed"
        );
    }

    #[test]
    fn cluster_expansion_respects_requested_platform_allowlist() {
        let cluster = ClusterCandidate {
            label: "mirror fit".to_string(),
            terms: json!(["mirror outfit"]),
            intent: "creator outfit checks".to_string(),
            visual_criteria: json!("single person mirror photo"),
            relevance_score: 0.91,
            deeper_queries: Vec::new(),
        };
        let allowed_platforms = vec!["instagram".to_string()];

        let expanded = cluster_seed_queries(&cluster, &allowed_platforms);

        assert!(!expanded.is_empty());
        assert!(expanded.iter().all(|seed| seed.platform == "instagram"));
    }

    #[test]
    fn clone_discovery_item_load_sql_filters_requested_platforms_with_json_each() {
        let sql = clone_discovery_items_sql();
        let filter_json = discovery_platform_filter_json(&["instagram".to_string()]);

        assert!(sql.contains("di.platform IN (SELECT value FROM json_each(?))"));
        assert_eq!(filter_json, "[\"instagram\"]");
        assert!(!filter_json.contains("tiktok"));
    }

    #[test]
    fn seed_queries_are_capped_per_platform() {
        let seeds = (0..5)
            .map(|index| SeedQuery {
                query: format!("tiktok query {index}"),
                platform: "tiktok".to_string(),
                source: "seed_extraction".to_string(),
                raw_json: json!({}),
            })
            .chain((0..3).map(|index| SeedQuery {
                query: format!("instagram query {index}"),
                platform: "instagram".to_string(),
                source: "seed_extraction".to_string(),
                raw_json: json!({}),
            }))
            .collect::<Vec<_>>();

        let capped = cap_seed_queries_per_platform(seeds, 2);

        assert_eq!(
            capped
                .iter()
                .filter(|seed| seed.platform == "tiktok")
                .count(),
            2
        );
        assert_eq!(
            capped
                .iter()
                .filter(|seed| seed.platform == "instagram")
                .count(),
            2
        );
    }

    #[test]
    fn loaded_moodboard_count_must_be_exactly_five() {
        assert!(!valid_loaded_moodboard_count(4));
        assert!(valid_loaded_moodboard_count(5));
        assert!(!valid_loaded_moodboard_count(6));
    }

    #[test]
    fn refresh_pool_messages_serialize_as_queue_contract() {
        let message = NicheResearchMessage::RefreshPool {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            reason: "pool_depleted".to_string(),
        };

        assert_eq!(
            serde_json::to_value(message).unwrap(),
            json!({
                "type": "refresh_pool",
                "userId": "user_1",
                "cloneId": "clone_1",
                "reason": "pool_depleted"
            })
        );

        let parsed: NicheResearchMessage = serde_json::from_value(json!({
            "type": "refresh_pool",
            "userId": "user_1",
            "cloneId": "clone_1",
            "reason": "pool_depleted"
        }))
        .unwrap();
        assert!(matches!(
            parsed,
            NicheResearchMessage::RefreshPool {
                user_id,
                clone_id,
                reason
            } if user_id == "user_1" && clone_id == "clone_1" && reason == "pool_depleted"
        ));
    }
}
