use crate::ai::workers_ai::{
    clone_compatibility_prompt, run_multi_vision_json, run_vision_json,
    visual_reference_review_prompt, CloneCompatibilityReview,
};
use crate::db;
use crate::domain::visual_reference::{
    accept_clone_compatibility, accept_visual_review, rank_candidates_for_review,
    selected_moodboard_count_is_valid, visual_review_tags, CandidateDiversityCaps, MoodboardBrief,
    VisualCandidateForRanking, VisualReferenceReview,
};
use crate::providers::higgsfield_auth::provider_account_access_token;
use crate::providers::higgsfield_mcp::{call_tool, upload_media_files, HiggsfieldMcpMediaFile};
use crate::providers::instagram_references::{
    build_instagram_post_url, build_instagram_profile_url, build_instagram_reels_search_url,
    build_instagram_user_posts_url, extract_instagram_reels_owner_handles,
    instagram_candidate_meets_min_dimensions, normalize_instagram_post_detail,
    normalize_instagram_user_posts, InstagramFallbackPolicy, InstagramImageCandidate,
};
use crate::providers::scrapecreators::{fetch_scrapecreators_json, ScrapeCreatorsError};
use crate::providers::seedream::{
    extract_seedream_cleaned_image_url, seedream_cleanup_arguments_with_model,
    SEEDREAM_CLEANUP_MODEL,
};
use crate::services::blitz::create_next_batch;
use crate::services::visual_reference_cache::cache_approved_visual_reference;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use worker::{
    D1Database, Env, Error, MessageBatch, MessageBuilder, MessageExt, Result as WorkerResult,
};

const NICHE_RESEARCH_STATUS_CAS_MISS: &str = "niche_research_status_cas_miss";
const VISUAL_REFERENCE_DRAIN_RETRY_DELAY_SECONDS: u32 = 30;
const HIGGSFIELD_REFRESH_SECRET_NAME: &str = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER";
const HIGGSFIELD_PROVIDER_ACCOUNT_ID: &str = "pa_higgsfield_founder";
const HIGGSFIELD_CLEANUP_TOOL_VAR: &str = "HIGGSFIELD_MCP_CLEANUP_TOOL";
const HIGGSFIELD_CLEANUP_MODEL_VAR: &str = "HIGGSFIELD_MCP_CLEANUP_MODEL";

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
    FetchInstagramPostDetail {
        user_id: String,
        clone_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        moodboard_id: String,
        moodboard_slug: String,
        handle: String,
        discovered_via: String,
        source_url: String,
    },
    ReviewVisualCandidates {
        user_id: String,
        clone_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<String>,
        limit: u32,
    },
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

        let kickoff_run_id = kickoff_message_run_id(&message);
        let failure_context = message_failure_context(&message, kickoff_run_id.as_deref());
        match handle_message(message, &env, kickoff_run_id.as_deref()).await {
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

fn kickoff_message_run_id(message: &NicheResearchMessage) -> Option<String> {
    matches!(
        message,
        NicheResearchMessage::ResearchMoodboardReferences { .. }
            | NicheResearchMessage::RefreshPool { .. }
    )
    .then(new_research_run_id)
}

fn message_failure_context(
    message: &NicheResearchMessage,
    kickoff_run_id: Option<&str>,
) -> MessageFailureContext {
    match message {
        NicheResearchMessage::ResearchMoodboardReferences {
            user_id, clone_id, ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: kickoff_run_id.map(str::to_string),
            message_type: "research_moodboard_references",
        },
        NicheResearchMessage::DiscoverInstagramHandles {
            user_id,
            clone_id,
            run_id,
            ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: run_id.clone(),
            message_type: "discover_instagram_handles",
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
        NicheResearchMessage::FetchInstagramPostDetail {
            user_id,
            clone_id,
            run_id,
            ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: run_id.clone(),
            message_type: "fetch_instagram_post_detail",
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
        NicheResearchMessage::CleanupApprovedReference {
            user_id,
            clone_id,
            run_id,
            ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: run_id.clone(),
            message_type: "cleanup_approved_reference",
        },
        NicheResearchMessage::ValidateCloneCompatibility {
            user_id,
            clone_id,
            run_id,
            ..
        } => MessageFailureContext {
            user_id: user_id.clone(),
            clone_id: clone_id.clone(),
            run_id: run_id.clone(),
            message_type: "validate_clone_compatibility",
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
            run_id: kickoff_run_id.map(str::to_string),
            message_type: "refresh_pool",
        },
    }
}

async fn handle_message(
    message: NicheResearchMessage,
    env: &Env,
    kickoff_run_id: Option<&str>,
) -> WorkerResult<()> {
    let db = env.d1("DB")?;
    match message {
        NicheResearchMessage::ResearchMoodboardReferences {
            user_id,
            clone_id,
            moodboard_ids,
            reason,
        } => {
            research_moodboard_references(
                &db,
                env,
                &user_id,
                &clone_id,
                &moodboard_ids,
                &reason,
                kickoff_run_id,
            )
            .await
        }
        NicheResearchMessage::DiscoverInstagramHandles {
            user_id,
            clone_id,
            run_id,
            moodboard_id,
            moodboard_slug,
            search_term,
            page,
        } => {
            discover_instagram_handles_message(
                &db,
                env,
                &user_id,
                &clone_id,
                run_id.as_deref(),
                &moodboard_id,
                &moodboard_slug,
                &search_term,
                page,
            )
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
        NicheResearchMessage::FetchInstagramPostDetail {
            user_id,
            clone_id,
            run_id,
            moodboard_id,
            moodboard_slug,
            handle,
            discovered_via,
            source_url,
        } => {
            fetch_instagram_post_detail_message(
                &db,
                env,
                &user_id,
                &clone_id,
                run_id.as_deref(),
                &moodboard_id,
                &moodboard_slug,
                &handle,
                &discovered_via,
                &source_url,
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
        NicheResearchMessage::CleanupApprovedReference {
            user_id,
            clone_id,
            run_id,
            candidate_id,
        } => {
            cleanup_approved_reference_message(
                &db,
                env,
                &user_id,
                &clone_id,
                run_id.as_deref(),
                &candidate_id,
            )
            .await
        }
        NicheResearchMessage::ValidateCloneCompatibility {
            user_id,
            clone_id,
            run_id,
            candidate_id,
        } => {
            validate_clone_compatibility_message(
                &db,
                env,
                &user_id,
                &clone_id,
                run_id.as_deref(),
                &candidate_id,
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
            research_moodboard_references(
                &db,
                env,
                &user_id,
                &clone_id,
                &moodboard_ids,
                &reason,
                kickoff_run_id,
            )
            .await
        }
    }
}

async fn research_moodboard_references(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    moodboard_ids: &[String],
    reason: &str,
    run_id: Option<&str>,
) -> WorkerResult<()> {
    let run_id = start_visual_reference_research_run(db, user_id, clone_id, reason, run_id).await?;
    enqueue_moodboard_reference_research(db, env, user_id, clone_id, moodboard_ids, &run_id).await
}

async fn start_visual_reference_research_run(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    reason: &str,
    run_id: Option<&str>,
) -> WorkerResult<String> {
    let run_id = run_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(new_research_run_id);
    set_clone_research_status_with_run(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Queued),
        reason,
        None,
        Some(&run_id),
    )
    .await?;
    Ok(run_id)
}

async fn enqueue_moodboard_reference_research(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    moodboard_ids: &[String],
    run_id: &str,
) -> WorkerResult<()> {
    let config = load_config_map(db).await?;
    let moodboards = load_selected_moodboards(db, user_id, clone_id, moodboard_ids).await?;
    if !selected_moodboard_count_is_valid(moodboards.len()) {
        return set_clone_research_status_with_run(
            db,
            user_id,
            clone_id,
            research_status_for_phase(ResearchPhase::InsufficientRefs),
            &format!("selected_moodboards={}, required=1..10", moodboards.len()),
            Some(run_id),
            Some(run_id),
        )
        .await;
    }

    let search_terms_per_moodboard =
        config_u32(&config, "instagram_search_terms_per_moodboard", 2).max(1);
    let reels_pages_per_term = config_u32(&config, "instagram_reels_pages_per_term", 1).max(1);
    let base_url = env_var(
        env,
        "SCRAPECREATORS_BASE_URL",
        "scrapecreators_base_url_missing",
    )?;
    let now = now_iso_string();

    let mut available_searches = 0usize;
    for moodboard in moodboards {
        let terms = selected_search_terms(
            &moodboard.search_queries_json,
            &moodboard.slug,
            &moodboard.title,
            search_terms_per_moodboard,
        );

        for term in terms {
            for page in 1..=reels_pages_per_term {
                available_searches += 1;
                let reserved_search = reserve_instagram_reels_search_source(
                    db,
                    &base_url,
                    user_id,
                    clone_id,
                    run_id,
                    &moodboard.id,
                    &moodboard.slug,
                    &term,
                    page,
                    &now,
                )
                .await?;
                if !reserved_search {
                    continue;
                }
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
            }
        }
    }

    if available_searches == 0 {
        set_clone_research_status_with_run(
            db,
            user_id,
            clone_id,
            research_status_for_phase(ResearchPhase::InsufficientRefs),
            "no moodboard search terms available for selected moodboards",
            Some(run_id),
            Some(run_id),
        )
        .await?;
    }

    Ok(())
}

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
        &format!(
            "discovering instagram handles moodboard={moodboard_slug} term={search_term} page={page}"
        ),
        Some(&run_id),
        Some(&run_id),
    )
    .await?;
    if !status_write_allows_side_effects(status_write) {
        return Ok(());
    }

    let config = load_config_map(db).await?;
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
            if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                .await?
                .is_none()
            {
                return Ok(());
            }
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
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }

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
    for seed in dedupe_handle_seeds(handles) {
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

    mark_discovery_source_fresh(db, &source_id, &params, &now).await?;
    enqueue_delayed_finalize_reference_pool(
        env,
        user_id,
        clone_id,
        &run_id,
        "instagram_handle_discovery_completed",
    )
    .await
}

async fn fetch_instagram_profile_message(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    discovered_via: &str,
    _related_depth: u8,
) -> WorkerResult<()> {
    let Some(run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    let status_write = set_clone_research_status_with_run_result(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Scraping),
        &format!("fetching instagram profile handle={handle} moodboard={moodboard_slug}"),
        Some(&run_id),
        Some(&run_id),
    )
    .await?;
    if !status_write_allows_side_effects(status_write) {
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
    let request_url = build_instagram_profile_url(&base_url, handle)
        .map_err(|error| Error::RustError(error.to_string()))?;
    let now = now_iso_string();
    let params = instagram_profile_source_params(
        user_id,
        clone_id,
        &run_id,
        moodboard_id,
        moodboard_slug,
        handle,
        discovered_via,
        _related_depth,
    );
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }
    let source_id = upsert_discovery_source(db, &request_url, &params, &now).await?;
    match fetch_scrapecreators_json(&request_url, &api_key).await {
        Ok(_) => {}
        Err(error) => {
            if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                .await?
                .is_none()
            {
                return Ok(());
            }
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
                "instagram_profile_source_failed",
            )
            .await;
        }
    };
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }

    mark_discovery_source_fresh(db, &source_id, &params, &now).await?;
    let normalized_handle =
        normalize_instagram_handle(handle).unwrap_or_else(|| handle.to_string());
    reserve_instagram_posts_source(
        db,
        &base_url,
        user_id,
        clone_id,
        &run_id,
        moodboard_id,
        moodboard_slug,
        &normalized_handle,
        discovered_via,
        None,
        0,
        &now,
    )
    .await?;
    env.queue("NICHE_RESEARCH_QUEUE")?
        .send(NicheResearchMessage::FetchInstagramPosts {
            user_id: user_id.to_string(),
            clone_id: clone_id.to_string(),
            run_id: Some(run_id.clone()),
            moodboard_id: moodboard_id.to_string(),
            moodboard_slug: moodboard_slug.to_string(),
            handle: normalized_handle,
            discovered_via: discovered_via.to_string(),
            next_max_id: None,
            page: 0,
        })
        .await?;

    Ok(())
}

async fn fetch_instagram_posts_message(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    discovered_via: &str,
    next_max_id: Option<&str>,
    page: u8,
) -> WorkerResult<()> {
    let Some(run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    let status_write = set_clone_research_status_with_run_result(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Scraping),
        &format!("fetching instagram posts handle={handle} moodboard={moodboard_slug} page={page}"),
        Some(&run_id),
        Some(&run_id),
    )
    .await?;
    if !status_write_allows_side_effects(status_write) {
        return Ok(());
    }

    let config = load_config_map(db).await?;
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
    let request_url = build_instagram_user_posts_url(&base_url, handle, next_max_id)
        .map_err(|error| Error::RustError(error.to_string()))?;
    let now = now_iso_string();
    let params = instagram_posts_source_params(
        user_id,
        clone_id,
        &run_id,
        moodboard_id,
        moodboard_slug,
        handle,
        discovered_via,
        next_max_id,
        page,
    );
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }
    let source_id = upsert_discovery_source(db, &request_url, &params, &now).await?;
    let raw = match fetch_scrapecreators_json(&request_url, &api_key).await {
        Ok(raw) => raw,
        Err(error) => {
            if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                .await?
                .is_none()
            {
                return Ok(());
            }
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
                "instagram_posts_source_failed",
            )
            .await;
        }
    };
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }

    let images_per_post = config_u32(&config, "instagram_images_per_post", 3) as usize;
    let min_width = config_u32(&config, "instagram_min_image_width", 512);
    let min_height = config_u32(&config, "instagram_min_image_height", 512);
    let posts_per_profile = config_u32(&config, "instagram_posts_per_profile", 12) as usize;
    let candidate_cap = posts_per_profile
        .saturating_mul(images_per_post)
        .max(images_per_post);
    let candidates = normalize_instagram_user_posts(
        &raw,
        handle,
        moodboard_id,
        moodboard_slug,
        discovered_via,
        InstagramFallbackPolicy::SkipVideos,
        images_per_post,
    );
    let post_detail_targets =
        instagram_post_detail_targets(&raw, &candidates, images_per_post, posts_per_profile);
    for candidate in candidates
        .into_iter()
        .filter(|candidate| {
            instagram_candidate_meets_min_dimensions(candidate, min_width, min_height)
        })
        .take(candidate_cap)
    {
        insert_instagram_candidate(db, user_id, clone_id, &run_id, &source_id, &candidate, &now)
            .await?;
    }

    for target in post_detail_targets {
        if !reserve_instagram_post_detail_source(
            db,
            &base_url,
            user_id,
            clone_id,
            &run_id,
            moodboard_id,
            moodboard_slug,
            handle,
            discovered_via,
            &target.source_url,
            &now,
        )
        .await?
        {
            continue;
        }
        env.queue("NICHE_RESEARCH_QUEUE")?
            .send(NicheResearchMessage::FetchInstagramPostDetail {
                user_id: user_id.to_string(),
                clone_id: clone_id.to_string(),
                run_id: Some(run_id.clone()),
                moodboard_id: moodboard_id.to_string(),
                moodboard_slug: moodboard_slug.to_string(),
                handle: handle.to_string(),
                discovered_via: discovered_via.to_string(),
                source_url: target.source_url,
            })
            .await?;
    }

    mark_discovery_source_fresh(db, &source_id, &params, &now).await?;

    let review_limit = config_u32(&config, "instagram_candidate_review_limit", 60);
    env.queue("NICHE_RESEARCH_QUEUE")?
        .send(NicheResearchMessage::ReviewVisualCandidates {
            user_id: user_id.to_string(),
            clone_id: clone_id.to_string(),
            run_id: Some(run_id.clone()),
            limit: review_limit,
        })
        .await?;

    let pages_per_profile = config_u32(&config, "instagram_pages_per_profile", 1);
    let next_max_id = instagram_posts_next_max_id(&raw);
    if instagram_posts_more_available(&raw)
        && u32::from(page) + 1 < pages_per_profile
        && next_max_id.is_some()
    {
        let next_page = page.saturating_add(1);
        reserve_instagram_posts_source(
            db,
            &base_url,
            user_id,
            clone_id,
            &run_id,
            moodboard_id,
            moodboard_slug,
            handle,
            discovered_via,
            next_max_id.as_deref(),
            next_page,
            &now,
        )
        .await?;
        env.queue("NICHE_RESEARCH_QUEUE")?
            .send(NicheResearchMessage::FetchInstagramPosts {
                user_id: user_id.to_string(),
                clone_id: clone_id.to_string(),
                run_id: Some(run_id),
                moodboard_id: moodboard_id.to_string(),
                moodboard_slug: moodboard_slug.to_string(),
                handle: handle.to_string(),
                discovered_via: discovered_via.to_string(),
                next_max_id,
                page: next_page,
            })
            .await?;
    }

    Ok(())
}

async fn fetch_instagram_post_detail_message(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    discovered_via: &str,
    source_url: &str,
) -> WorkerResult<()> {
    let Some(run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    let status_write = set_clone_research_status_with_run_result(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Scraping),
        &format!("fetching instagram post detail url={source_url} moodboard={moodboard_slug}"),
        Some(&run_id),
        Some(&run_id),
    )
    .await?;
    if !status_write_allows_side_effects(status_write) {
        return Ok(());
    }

    let config = load_config_map(db).await?;
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
    let request_url = build_instagram_post_url(&base_url, source_url, "US")
        .map_err(|error| Error::RustError(error.to_string()))?;
    let now = now_iso_string();
    let params = instagram_post_detail_source_params(
        user_id,
        clone_id,
        &run_id,
        moodboard_id,
        moodboard_slug,
        handle,
        discovered_via,
        source_url,
    );
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }
    let source_id = upsert_discovery_source(db, &request_url, &params, &now).await?;
    let raw = match fetch_scrapecreators_json(&request_url, &api_key).await {
        Ok(raw) => raw,
        Err(error) => {
            if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                .await?
                .is_none()
            {
                return Ok(());
            }
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
                "instagram_post_detail_source_failed",
            )
            .await;
        }
    };
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }

    let images_per_post = config_u32(&config, "instagram_images_per_post", 3) as usize;
    let min_width = config_u32(&config, "instagram_min_image_width", 512);
    let min_height = config_u32(&config, "instagram_min_image_height", 512);
    let candidates = normalize_instagram_post_detail(
        &raw,
        handle,
        source_url,
        moodboard_id,
        moodboard_slug,
        discovered_via,
        images_per_post,
    );
    for candidate in candidates.into_iter().filter(|candidate| {
        instagram_candidate_meets_min_dimensions(candidate, min_width, min_height)
    }) {
        insert_instagram_candidate(db, user_id, clone_id, &run_id, &source_id, &candidate, &now)
            .await?;
    }

    mark_discovery_source_fresh(db, &source_id, &params, &now).await?;

    let review_limit = config_u32(&config, "instagram_candidate_review_limit", 60);
    env.queue("NICHE_RESEARCH_QUEUE")?
        .send(NicheResearchMessage::ReviewVisualCandidates {
            user_id: user_id.to_string(),
            clone_id: clone_id.to_string(),
            run_id: Some(run_id),
            limit: review_limit,
        })
        .await?;

    Ok(())
}

async fn review_visual_candidates_message(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    limit: u32,
) -> WorkerResult<()> {
    let Some(run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    let status_write = set_clone_research_status_with_run_result(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Reviewing),
        &format!("reviewing visual candidates limit={limit}"),
        Some(&run_id),
        Some(&run_id),
    )
    .await?;
    if !status_write_allows_side_effects(status_write) {
        return Ok(());
    }

    let config = load_config_map(db).await?;
    let moodboard_ids = load_selected_moodboard_ids(db, user_id, clone_id).await?;
    let moodboards = load_selected_moodboards(db, user_id, clone_id, &moodboard_ids).await?;
    if !selected_moodboard_count_is_valid(moodboards.len()) {
        return set_clone_research_status_with_run(
            db,
            user_id,
            clone_id,
            research_status_for_phase(ResearchPhase::InsufficientRefs),
            &format!("selected_moodboards={}, required=1..10", moodboards.len()),
            Some(&run_id),
            Some(&run_id),
        )
        .await;
    }
    let selected = moodboards
        .iter()
        .map(|row| MoodboardBrief {
            id: row.id.clone(),
            slug: row.slug.clone(),
            title: row.title.clone(),
            vibe_summary: row.vibe_summary.clone(),
            search_queries: serde_json::from_str(&row.search_queries_json).unwrap_or_default(),
        })
        .collect::<Vec<_>>();

    let configured_limit = config_u32(&config, "instagram_candidate_review_limit", 60);
    let review_limit = if limit == 0 {
        configured_limit
    } else {
        limit.min(configured_limit)
    }
    .max(1) as usize;
    let review_retry_limit =
        config_u32(&config, "instagram_candidate_review_retry_limit", 2).max(1);
    let accepted_refs_per_profile_cap =
        config_u32(&config, "accepted_refs_per_profile_cap", 3).max(1);
    let max_accepted_refs_per_run = config_u32(&config, "max_accepted_refs_per_run", 40).max(1);
    let caps = CandidateDiversityCaps {
        review_limit,
        per_handle_review_cap: accepted_refs_per_profile_cap as usize,
        per_moodboard_review_cap: config_u32(&config, "accepted_refs_per_moodboard_target", 5)
            .max(1) as usize,
    };
    let candidates = load_unreviewed_visual_candidates(
        db,
        clone_id,
        &run_id,
        review_retry_limit,
        review_limit.saturating_mul(4) as u32,
    )
    .await?;
    let retryable_candidates_loaded = candidates
        .iter()
        .filter(|candidate| candidate.is_review_retryable())
        .map(|candidate| candidate.id.clone())
        .collect::<HashSet<_>>();
    let candidate_by_id = candidates
        .into_iter()
        .map(|candidate| (candidate.id.clone(), candidate))
        .collect::<HashMap<_, _>>();
    let ranked = rank_candidates_for_review(
        candidate_by_id
            .values()
            .map(VisualCandidateReviewRow::for_ranking)
            .collect(),
        &caps,
    );
    let ranked_ids = ranked
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect::<HashSet<_>>();
    let retryable_candidates_remaining = retryable_candidates_loaded
        .iter()
        .any(|candidate_id| !ranked_ids.contains(candidate_id));
    let ai = env.ai("AI")?;
    let mut pipeline_messages_enqueued = 0usize;
    let mut retryable_follow_up_needed = retryable_candidates_remaining;
    let mut stale_review_write_seen = false;

    for ranked_candidate in ranked {
        let Some(candidate) = candidate_by_id.get(&ranked_candidate.id) else {
            continue;
        };
        if current_message_run_id(db, user_id, clone_id, Some(&run_id))
            .await?
            .is_none()
        {
            return Ok(());
        }
        let observed_attempts = candidate.review_attempts();
        let claimed = claim_visual_candidate_for_review(
            db,
            clone_id,
            &run_id,
            &candidate.id,
            observed_attempts,
            review_retry_limit,
        )
        .await?;
        if !claimed {
            stale_review_write_seen = true;
            continue;
        }
        let source_handle = candidate.source_handle.clone().unwrap_or_default();
        let prompt = visual_reference_review_prompt(
            &selected,
            "instagram",
            &source_handle,
            candidate.source_caption.as_deref(),
            candidate.like_count,
            candidate.comment_count,
            candidate.source_published_at.as_deref(),
        );
        let review = match run_vision_json::<VisualReferenceReview>(
            &ai,
            &prompt,
            &candidate.image_url,
        )
        .await
        {
            Ok(review) => review,
            Err(error) => {
                if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                    .await?
                    .is_none()
                {
                    return Ok(());
                }
                let code = queue_error_code(&error.to_string());
                let failure_outcome =
                    review_failure_outcome(code, observed_attempts, review_retry_limit);
                let recorded_failure = mark_candidate_review_failed(
                    db,
                    &candidate.id,
                    &run_id,
                    code,
                    &error.to_string(),
                    review_retry_limit,
                    observed_attempts,
                )
                .await?;
                if !recorded_failure {
                    stale_review_write_seen = true;
                } else if failure_outcome == ReviewFailureOutcome::Retryable {
                    retryable_follow_up_needed = true;
                }
                continue;
            }
        };
        if current_message_run_id(db, user_id, clone_id, Some(&run_id))
            .await?
            .is_none()
        {
            return Ok(());
        }
        let review_json = serde_json::to_string(&review).unwrap_or_else(|_| "{}".to_string());
        match accept_visual_review(&review, &selected) {
            Ok(accepted) => {
                match mark_candidate_approved_with_cap_guards(
                    db,
                    clone_id,
                    &run_id,
                    &candidate.id,
                    candidate.source_handle.as_deref(),
                    &review_json,
                    &accepted,
                    max_accepted_refs_per_run,
                    accepted_refs_per_profile_cap,
                )
                .await?
                {
                    GuardedCandidateApproval::Approved => {
                        env.queue("NICHE_RESEARCH_QUEUE")?
                            .send(NicheResearchMessage::CleanupApprovedReference {
                                user_id: user_id.to_string(),
                                clone_id: clone_id.to_string(),
                                run_id: Some(run_id.clone()),
                                candidate_id: candidate.id.clone(),
                            })
                            .await?;
                        pipeline_messages_enqueued += 1;
                    }
                    GuardedCandidateApproval::RunCapReached => {
                        let rejected = mark_candidate_rejected_with_review(
                            db,
                            &candidate.id,
                            &run_id,
                            &review_json,
                            "max_accepted_refs_per_run_reached",
                        )
                        .await?;
                        if !rejected {
                            stale_review_write_seen = true;
                        }
                    }
                    GuardedCandidateApproval::HandleCapReached => {
                        let rejected = mark_candidate_rejected_with_review(
                            db,
                            &candidate.id,
                            &run_id,
                            &review_json,
                            "accepted_refs_per_profile_cap_reached",
                        )
                        .await?;
                        if !rejected {
                            stale_review_write_seen = true;
                        }
                    }
                    GuardedCandidateApproval::Skipped => {
                        stale_review_write_seen = true;
                    }
                }
            }
            Err(reason) => {
                let rejected = mark_candidate_rejected_with_review(
                    db,
                    &candidate.id,
                    &run_id,
                    &review_json,
                    reason,
                )
                .await?;
                if !rejected {
                    stale_review_write_seen = true;
                }
            }
        }
    }

    if !retryable_follow_up_needed {
        retryable_follow_up_needed =
            has_remaining_retryable_visual_candidates(db, clone_id, &run_id, review_retry_limit)
                .await?;
    }

    if stale_review_write_seen && pipeline_messages_enqueued == 0 && !retryable_follow_up_needed {
        return Ok(());
    }

    match review_completion_action(pipeline_messages_enqueued, retryable_follow_up_needed) {
        action @ ReviewCompletionAction::WaitForCache => {
            debug_assert!(review_completion_schedules_finalize_nudge(action));
            enqueue_delayed_finalize_reference_pool(
                env,
                user_id,
                clone_id,
                &run_id,
                "visual_candidate_review_cache_pending",
            )
            .await?;
        }
        ReviewCompletionAction::EnqueueRetry => {
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::ReviewVisualCandidates {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id),
                    limit: review_limit as u32,
                })
                .await?;
        }
        ReviewCompletionAction::Finalize => {
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::FinalizeReferencePool {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id),
                    reason: "visual_candidate_review_completed".to_string(),
                })
                .await?;
        }
    }

    Ok(())
}

async fn cleanup_approved_reference_message(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    candidate_id: &str,
) -> WorkerResult<()> {
    let Some(run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    let status_write = set_clone_research_status_with_run_result(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Reviewing),
        &format!("cleaning approved visual reference candidate={candidate_id}"),
        Some(&run_id),
        Some(&run_id),
    )
    .await?;
    if !status_write_allows_side_effects(status_write) {
        return Ok(());
    }

    let config = load_config_map(db).await?;
    let cleanup_retry_limit = config_u32(&config, "visual_reference_cleanup_retry_limit", 3).max(1);
    let review_limit = config_u32(&config, "instagram_candidate_review_limit", 60).max(1);
    let Some(candidate) = load_candidate_for_cleanup(db, clone_id, &run_id, candidate_id).await?
    else {
        return Ok(());
    };
    let observed_attempts = attempts_from_json(&candidate.cleanup_json);
    let claimed = claim_visual_candidate_for_cleanup(
        db,
        clone_id,
        &run_id,
        &candidate.id,
        observed_attempts,
        cleanup_retry_limit,
    )
    .await?;
    if !claimed {
        enqueue_delayed_finalize_reference_pool(
            env,
            user_id,
            clone_id,
            &run_id,
            "approved_visual_reference_cleanup_claim_pending",
        )
        .await?;
        return Ok(());
    }
    let expected_cleanup_attempt = observed_attempts.saturating_add(1);
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }

    let cleanup_request = prepare_seedream_cleanup(env, &candidate.id, &candidate.image_url).await;
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }
    let cleanup_result = match cleanup_request {
        Ok(cleanup_request) => call_seedream_cleanup(&candidate.id, &cleanup_request).await,
        Err(error) => Err(error),
    };
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }

    match cleanup_result {
        Ok((cleaned_image_url, provider_job_id, _raw_json)) => {
            let marked = mark_candidate_cleanup_succeeded(
                db,
                clone_id,
                &run_id,
                &candidate.id,
                &cleaned_image_url,
                &provider_job_id,
                expected_cleanup_attempt,
            )
            .await?;
            if !marked {
                return Ok(());
            }
            if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                .await?
                .is_none()
            {
                return Ok(());
            }
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::ValidateCloneCompatibility {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id),
                    candidate_id: candidate.id,
                })
                .await?;
        }
        Err(error) => {
            let code = queue_error_code(&error.to_string());
            let marked = mark_candidate_cleanup_failed(
                db,
                clone_id,
                &run_id,
                &candidate.id,
                cleanup_retry_limit,
                expected_cleanup_attempt,
                code,
                &error.to_string(),
            )
            .await?;
            if !marked {
                return Ok(());
            }
            if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                .await?
                .is_none()
            {
                return Ok(());
            }
            if observed_attempts.saturating_add(1) < cleanup_retry_limit {
                env.queue("NICHE_RESEARCH_QUEUE")?
                    .send(NicheResearchMessage::CleanupApprovedReference {
                        user_id: user_id.to_string(),
                        clone_id: clone_id.to_string(),
                        run_id: Some(run_id.clone()),
                        candidate_id: candidate.id.clone(),
                    })
                    .await?;
            }
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::ReviewVisualCandidates {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id.clone()),
                    limit: review_limit,
                })
                .await?;
            enqueue_delayed_finalize_reference_pool(
                env,
                user_id,
                clone_id,
                &run_id,
                "approved_visual_reference_cleanup_failed",
            )
            .await?;
        }
    }

    Ok(())
}

async fn prepare_seedream_cleanup(
    env: &Env,
    candidate_id: &str,
    image_url: &str,
) -> WorkerResult<SeedreamCleanupRequest> {
    let token = provider_account_access_token(
        env,
        HIGGSFIELD_PROVIDER_ACCOUNT_ID,
        HIGGSFIELD_REFRESH_SECRET_NAME,
    )
    .await
    .map_err(|error| Error::RustError(format!("seedream_cleanup_auth_failed:{error}")))?;
    let (bytes, content_type) = fetch_reference_cleanup_image(image_url).await?;
    let uploaded = upload_media_files(
        &token.access_token,
        &[HiggsfieldMcpMediaFile {
            filename: format!("{candidate_id}.{}", cleanup_extension(&content_type)),
            content_type,
            bytes,
        }],
    )
    .await
    .map_err(|error| Error::RustError(format!("seedream_cleanup_upload_failed:{error}")))?;
    let Some(reference) = uploaded.first() else {
        return Err(Error::RustError(
            "seedream_cleanup_upload_missing".to_string(),
        ));
    };
    let tool_name = env
        .var(HIGGSFIELD_CLEANUP_TOOL_VAR)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "generate_image".to_string());
    let model = env
        .var(HIGGSFIELD_CLEANUP_MODEL_VAR)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| SEEDREAM_CLEANUP_MODEL.to_string());
    Ok(SeedreamCleanupRequest {
        access_token: token.access_token,
        reference_value: reference.reference_value.clone(),
        tool_name,
        model,
    })
}

async fn call_seedream_cleanup(
    candidate_id: &str,
    request: &SeedreamCleanupRequest,
) -> WorkerResult<(String, String, Value)> {
    let response = call_tool(
        &request.access_token,
        json!(format!("seedream-cleanup:{candidate_id}")),
        &request.tool_name,
        seedream_cleanup_arguments_with_model(&request.reference_value, &request.model),
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

async fn fetch_reference_cleanup_image(image_url: &str) -> WorkerResult<(Vec<u8>, String)> {
    crate::services::visual_reference_cache::fetch_visual_reference_image(image_url).await
}

fn cleanup_extension(content_type: &str) -> &'static str {
    match content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "image/heif" => "heif",
        _ => "jpg",
    }
}

async fn validate_clone_compatibility_message(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    candidate_id: &str,
) -> WorkerResult<()> {
    let Some(run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    let status_write = set_clone_research_status_with_run_result(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Reviewing),
        &format!("validating clone compatibility candidate={candidate_id}"),
        Some(&run_id),
        Some(&run_id),
    )
    .await?;
    if !status_write_allows_side_effects(status_write) {
        return Ok(());
    }

    let config = load_config_map(db).await?;
    let compatibility_retry_limit =
        config_u32(&config, "visual_reference_compatibility_retry_limit", 3).max(1);
    let clone_reference_limit =
        config_u32(&config, "clone_compatibility_reference_limit", 6).max(1);
    let review_limit = config_u32(&config, "instagram_candidate_review_limit", 60).max(1);
    let Some(candidate) =
        load_candidate_for_compatibility(db, clone_id, &run_id, candidate_id).await?
    else {
        return Ok(());
    };
    let observed_attempts = attempts_from_json(&candidate.compatibility_json);
    let claimed = claim_visual_candidate_for_compatibility(
        db,
        clone_id,
        &run_id,
        &candidate.id,
        observed_attempts,
        compatibility_retry_limit,
    )
    .await?;
    if !claimed {
        enqueue_delayed_finalize_reference_pool(
            env,
            user_id,
            clone_id,
            &run_id,
            "clone_compatibility_claim_pending",
        )
        .await?;
        return Ok(());
    }
    let expected_compatibility_attempt = observed_attempts.saturating_add(1);
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }

    let clone_reference_urls_result =
        load_clone_reference_image_urls(db, env, user_id, clone_id, clone_reference_limit).await;
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }
    let compatibility_result = match clone_reference_urls_result {
        Ok(clone_reference_urls) if clone_reference_urls.is_empty() => Err(Error::RustError(
            "clone_compatibility_reference_missing".to_string(),
        )),
        Ok(clone_reference_urls) => {
            let mut image_urls = vec![candidate.cleaned_image_url.clone()];
            image_urls.extend(clone_reference_urls);
            let prompt = clone_compatibility_prompt(image_urls.len().saturating_sub(1));
            match env.ai("AI") {
                Ok(ai) => {
                    run_multi_vision_json::<CloneCompatibilityReview>(&ai, &prompt, &image_urls)
                        .await
                }
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    };
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }

    match compatibility_result {
        Ok(review) => {
            let compatibility_json =
                serde_json::to_string(&review).unwrap_or_else(|_| "{}".to_string());
            match accept_clone_compatibility(&review) {
                Ok(()) => {
                    let marked = mark_candidate_compatibility_succeeded(
                        db,
                        clone_id,
                        &run_id,
                        &candidate.id,
                        &compatibility_json,
                        expected_compatibility_attempt,
                    )
                    .await?;
                    if !marked {
                        return Ok(());
                    }
                    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                        .await?
                        .is_none()
                    {
                        return Ok(());
                    }
                    env.queue("NICHE_RESEARCH_QUEUE")?
                        .send(NicheResearchMessage::CacheApprovedReference {
                            user_id: user_id.to_string(),
                            clone_id: clone_id.to_string(),
                            run_id: Some(run_id),
                            candidate_id: candidate.id,
                        })
                        .await?;
                }
                Err(reason) => {
                    let marked = mark_candidate_clone_mismatch(
                        db,
                        clone_id,
                        &run_id,
                        &candidate.id,
                        &compatibility_json,
                        reason,
                        expected_compatibility_attempt,
                    )
                    .await?;
                    if !marked {
                        return Ok(());
                    }
                    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                        .await?
                        .is_none()
                    {
                        return Ok(());
                    }
                    env.queue("NICHE_RESEARCH_QUEUE")?
                        .send(NicheResearchMessage::ReviewVisualCandidates {
                            user_id: user_id.to_string(),
                            clone_id: clone_id.to_string(),
                            run_id: Some(run_id.clone()),
                            limit: review_limit,
                        })
                        .await?;
                    enqueue_delayed_finalize_reference_pool(
                        env,
                        user_id,
                        clone_id,
                        &run_id,
                        "clone_compatibility_mismatch",
                    )
                    .await?;
                }
            }
        }
        Err(error) => {
            let error_detail = error.to_string();
            let code = clone_compatibility_error_code(&error_detail);
            let marked = mark_candidate_compatibility_failed(
                db,
                clone_id,
                &run_id,
                &candidate.id,
                compatibility_retry_limit,
                expected_compatibility_attempt,
                code,
                &error_detail,
            )
            .await?;
            if !marked {
                return Ok(());
            }
            if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                .await?
                .is_none()
            {
                return Ok(());
            }
            if expected_compatibility_attempt < compatibility_retry_limit {
                env.queue("NICHE_RESEARCH_QUEUE")?
                    .send(NicheResearchMessage::ValidateCloneCompatibility {
                        user_id: user_id.to_string(),
                        clone_id: clone_id.to_string(),
                        run_id: Some(run_id.clone()),
                        candidate_id: candidate.id.clone(),
                    })
                    .await?;
            }
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::ReviewVisualCandidates {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id.clone()),
                    limit: review_limit,
                })
                .await?;
            enqueue_delayed_finalize_reference_pool(
                env,
                user_id,
                clone_id,
                &run_id,
                "clone_compatibility_failed",
            )
            .await?;
        }
    }

    Ok(())
}

async fn cache_approved_reference_message(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    candidate_id: &str,
) -> WorkerResult<()> {
    let Some(run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    let status_write = set_clone_research_status_with_run_result(
        db,
        user_id,
        clone_id,
        research_status_for_phase(ResearchPhase::Reviewing),
        &format!("caching approved visual reference candidate={candidate_id}"),
        Some(&run_id),
        Some(&run_id),
    )
    .await?;
    if !status_write_allows_side_effects(status_write) {
        return Ok(());
    }

    let Some(candidate) =
        load_approved_candidate_for_cache(db, clone_id, &run_id, candidate_id).await?
    else {
        return Ok(());
    };
    let claimed = claim_visual_candidate_for_cache(db, clone_id, &run_id, &candidate.id).await?;
    match cache_claim_action(claimed) {
        CacheClaimAction::Cache => {}
        CacheClaimAction::EnqueueDelayedFinalize => {
            if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                .await?
                .is_some()
            {
                repair_cached_visual_reference_inspiration_pool(
                    db, user_id, clone_id, &run_id, &candidate,
                )
                .await?;
            }
            if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                .await?
                .is_some()
            {
                mark_candidate_cache_succeeded(db, clone_id, &run_id, &candidate.id).await?;
            }
            enqueue_delayed_finalize_reference_pool(
                env,
                user_id,
                clone_id,
                &run_id,
                "approved_visual_reference_cache_claim_pending",
            )
            .await?;
            return Ok(());
        }
    }
    let review = serde_json::from_str::<VisualReferenceReview>(&candidate.review_json)
        .map_err(|error| Error::RustError(format!("approved_review_json_invalid:{error}")))?;
    let moodboard_id = candidate
        .moodboard_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("approved_candidate_missing_moodboard_id".to_string()))?;
    let moodboard_slug = candidate
        .moodboard_slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("approved_candidate_missing_moodboard_slug".to_string()))?;
    let now = now_iso_string();
    let visual_reference_id = visual_reference_id_for(clone_id, &candidate.id);
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }
    let cached = match cache_approved_visual_reference(
        db,
        env,
        user_id,
        clone_id,
        &visual_reference_id,
        &candidate.cleaned_image_url,
        candidate.image_width,
        candidate.image_height,
    )
    .await
    {
        Ok(cached) => cached,
        Err(error) => {
            if current_message_run_id(db, user_id, clone_id, Some(&run_id))
                .await?
                .is_none()
            {
                return Ok(());
            }
            mark_candidate_cache_failed(db, clone_id, &run_id, &candidate.id, &error.to_string())
                .await?;
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::FinalizeReferencePool {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id),
                    reason: "approved_visual_reference_cache_failed".to_string(),
                })
                .await?;
            return Ok(());
        }
    };
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }
    let inserted_reference = insert_approved_visual_reference(
        db,
        user_id,
        clone_id,
        &run_id,
        &visual_reference_id,
        &cached.media_asset_id,
        &candidate,
        moodboard_id,
        moodboard_slug,
        &review,
        &now,
    )
    .await?;
    if !inserted_reference
        && current_message_run_id(db, user_id, clone_id, Some(&run_id))
            .await?
            .is_none()
    {
        return Ok(());
    }
    db::exec(
        db,
        r#"
        UPDATE visual_references
        SET media_asset_id = ?
        WHERE id = ?
          AND clone_id = ?
        "#,
        vec![
            json!(cached.media_asset_id),
            json!(visual_reference_id),
            json!(clone_id),
        ],
    )
    .await?;
    insert_visual_reference_inspiration_pool_row(
        db,
        user_id,
        clone_id,
        &run_id,
        moodboard_id,
        &visual_reference_id,
        review.visual_fit_score,
        &now,
    )
    .await?;
    mark_candidate_cache_succeeded(db, clone_id, &run_id, &candidate.id).await?;
    if current_message_run_id(db, user_id, clone_id, Some(&run_id))
        .await?
        .is_none()
    {
        return Ok(());
    }

    env.queue("NICHE_RESEARCH_QUEUE")?
        .send(NicheResearchMessage::FinalizeReferencePool {
            user_id: user_id.to_string(),
            clone_id: clone_id.to_string(),
            run_id: Some(run_id),
            reason: "approved_visual_reference_cached".to_string(),
        })
        .await?;

    Ok(())
}

async fn enqueue_delayed_finalize_reference_pool(
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    reason: impl Into<String>,
) -> WorkerResult<()> {
    env.queue("NICHE_RESEARCH_QUEUE")?
        .send(
            MessageBuilder::new(NicheResearchMessage::FinalizeReferencePool {
                user_id: user_id.to_string(),
                clone_id: clone_id.to_string(),
                run_id: Some(run_id.to_string()),
                reason: reason.into(),
            })
            .delay_seconds(VISUAL_REFERENCE_DRAIN_RETRY_DELAY_SECONDS)
            .build(),
        )
        .await
}

async fn finalize_reference_pool_message(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
    reason: &str,
) -> WorkerResult<()> {
    let Some(run_id) = current_message_run_id(db, user_id, clone_id, run_id).await? else {
        return Ok(());
    };
    let config = load_config_map(db).await?;
    let target = config_u32(&config, "accepted_refs_per_moodboard_target", 5).max(1);
    let review_limit = config_u32(&config, "instagram_candidate_review_limit", 60).max(1);
    let review_retry_limit =
        config_u32(&config, "instagram_candidate_review_retry_limit", 2).max(1);
    let cleanup_retry_limit = config_u32(&config, "visual_reference_cleanup_retry_limit", 3).max(1);
    let compatibility_retry_limit =
        config_u32(&config, "visual_reference_compatibility_retry_limit", 3).max(1);
    let drain_state = load_finalize_drain_state(
        db,
        clone_id,
        &run_id,
        review_retry_limit,
        cleanup_retry_limit,
        compatibility_retry_limit,
        review_limit,
    )
    .await?;
    match finalize_drain_action(&drain_state) {
        FinalizeDrainAction::Proceed => {}
        FinalizeDrainAction::EnqueueCleanup => {
            for candidate_id in drain_state.cleanup_candidate_ids {
                env.queue("NICHE_RESEARCH_QUEUE")?
                    .send(NicheResearchMessage::CleanupApprovedReference {
                        user_id: user_id.to_string(),
                        clone_id: clone_id.to_string(),
                        run_id: Some(run_id.clone()),
                        candidate_id,
                    })
                    .await?;
            }
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::FinalizeReferencePool {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id),
                    reason: format!("{reason}:cleanup_work_pending"),
                })
                .await?;
            return Ok(());
        }
        FinalizeDrainAction::EnqueueCompatibility => {
            for candidate_id in drain_state.compatibility_candidate_ids {
                env.queue("NICHE_RESEARCH_QUEUE")?
                    .send(NicheResearchMessage::ValidateCloneCompatibility {
                        user_id: user_id.to_string(),
                        clone_id: clone_id.to_string(),
                        run_id: Some(run_id.clone()),
                        candidate_id,
                    })
                    .await?;
            }
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::FinalizeReferencePool {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id),
                    reason: format!("{reason}:compatibility_work_pending"),
                })
                .await?;
            return Ok(());
        }
        FinalizeDrainAction::EnqueueCache => {
            for candidate_id in drain_state.approved_uncached_candidate_ids {
                env.queue("NICHE_RESEARCH_QUEUE")?
                    .send(NicheResearchMessage::CacheApprovedReference {
                        user_id: user_id.to_string(),
                        clone_id: clone_id.to_string(),
                        run_id: Some(run_id.clone()),
                        candidate_id,
                    })
                    .await?;
            }
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::FinalizeReferencePool {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id),
                    reason: format!("{reason}:cache_work_pending"),
                })
                .await?;
            return Ok(());
        }
        FinalizeDrainAction::EnqueueReview => {
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::ReviewVisualCandidates {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id),
                    limit: review_limit,
                })
                .await?;
            return Ok(());
        }
        FinalizeDrainAction::EnqueueFinalize => {
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::FinalizeReferencePool {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id),
                    reason: format!("{reason}:discovery_work_pending"),
                })
                .await?;
            return Ok(());
        }
        FinalizeDrainAction::EnqueueDelayedFinalize => {
            enqueue_delayed_finalize_reference_pool(
                env,
                user_id,
                clone_id,
                &run_id,
                format!("{reason}:visual_work_in_progress"),
            )
            .await?;
            return Ok(());
        }
    }
    let selected_moodboard_ids = load_selected_moodboard_ids(db, user_id, clone_id).await?;
    if !selected_moodboard_count_is_valid(selected_moodboard_ids.len()) {
        return set_clone_research_status_with_run(
            db,
            user_id,
            clone_id,
            research_status_for_phase(ResearchPhase::InsufficientRefs),
            &format!(
                "{reason}: selected_moodboards={}, required=1..10",
                selected_moodboard_ids.len()
            ),
            Some(&run_id),
            Some(&run_id),
        )
        .await;
    }

    let counts = accepted_counts_by_moodboard(db, clone_id).await?;
    let counts_by_moodboard = counts
        .into_iter()
        .map(|row| (row.moodboard_id, row.count))
        .collect::<HashMap<_, _>>();
    let ready_count = selected_moodboard_ids
        .iter()
        .filter(|id| counts_by_moodboard.get(*id).copied().unwrap_or_default() >= target)
        .count();
    let total_refs = selected_moodboard_ids
        .iter()
        .map(|id| counts_by_moodboard.get(id).copied().unwrap_or_default())
        .sum::<u32>();
    let failures = load_candidate_failure_counts(db, clone_id, &run_id).await?;
    let phase =
        reference_pool_readiness_phase(total_refs, ready_count, selected_moodboard_ids.len());
    let batch_provider_soul_id = if phase != ResearchPhase::InsufficientRefs {
        load_clone_for_research(db, user_id, clone_id)
            .await?
            .and_then(|clone| {
                let provider_soul_id = clone
                    .provider_soul_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if finalize_readiness_action(phase, &clone.soul_status, provider_soul_id)
                    == FinalizeReadinessAction::OrchestrateBatchBeforeStatus
                {
                    provider_soul_id.map(str::to_string)
                } else {
                    None
                }
            })
    } else {
        None
    };
    if let Some(provider_soul_id) = batch_provider_soul_id {
        if current_message_run_id(db, user_id, clone_id, Some(&run_id))
            .await?
            .is_none()
        {
            return Ok(());
        }
        create_next_batch(db, env, user_id, clone_id, &provider_soul_id).await?;
        if current_message_run_id(db, user_id, clone_id, Some(&run_id))
            .await?
            .is_none()
        {
            return Ok(());
        }
    }
    let readiness_write = set_clone_research_status_with_run_result(
        db,
        user_id,
        clone_id,
        research_status_for_phase(phase),
        &format!(
            "{reason}: accepted_refs={total_refs}, ready_moodboards={ready_count}, selected_moodboards={}, target_refs_per_moodboard={target}, cleanup_failed={}, compatibility_failed={}, clone_mismatch={}, cache_failed={}",
            selected_moodboard_ids.len(),
            failures.cleanup_failed,
            failures.compatibility_failed,
            failures.clone_mismatch,
            failures.cache_failed,
        ),
        Some(&run_id),
        Some(&run_id),
    )
    .await?;
    if !finalize_side_effect_allowed(readiness_write) {
        return Ok(());
    }

    Ok(())
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

async fn load_clone_for_research(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
) -> WorkerResult<Option<CloneResearchRow>> {
    db::first(
        db,
        r#"
        SELECT soul_status, provider_soul_id
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
        SELECT id, slug, title, vibe_summary, search_queries_json
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

fn insert_visual_candidate_sql() -> &'static str {
    r#"
        INSERT INTO visual_reference_candidates (
          id,
          user_id,
          clone_id,
          platform,
          source_platform,
          source_handle,
          source_profile_id,
          source_post_id,
          source_post_code,
          source_image_index,
          source_url,
          source_published_at,
          source_caption,
          media_type,
          image_url,
          image_width,
          image_height,
          like_count,
          comment_count,
          play_count,
          moodboard_id,
          moodboard_slug,
          discovered_via,
          review_json,
          raw_json,
          metadata_json,
          created_at
        )
        VALUES (?, ?, ?, 'instagram', 'instagram', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '{}', ?, ?, ?)
        ON CONFLICT(clone_id, platform, source_handle, source_post_code, source_image_index) DO UPDATE SET
          image_url = excluded.image_url,
          image_width = excluded.image_width,
          image_height = excluded.image_height,
          like_count = excluded.like_count,
          comment_count = excluded.comment_count,
          play_count = excluded.play_count,
          source_caption = excluded.source_caption,
          raw_json = excluded.raw_json,
          metadata_json = CASE
            WHEN visual_reference_candidates.review_status IN ('unreviewed', 'review_retryable')
            THEN excluded.metadata_json
            ELSE visual_reference_candidates.metadata_json
          END
        "#
}

fn insert_visual_reference_sql() -> &'static str {
    r#"
        INSERT OR IGNORE INTO visual_references (
          id,
          user_id,
          clone_id,
          candidate_id,
          media_asset_id,
          source_platform,
          source_handle,
          source_post_code,
          source_url,
          source_published_at,
          image_width,
          image_height,
          moodboard_id,
          moodboard_slug,
          niche_cluster,
          visual_fit_score,
          pose,
          scene,
          lighting,
          framing,
          camera_feel,
          styling_direction,
          aesthetic_tags_json,
          source_caption_removed,
          status,
          created_at
        )
        SELECT ?, ?, ?, ?, ?, 'instagram', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 'active', ?
        WHERE EXISTS (
          SELECT 1
          FROM clone_profiles cp
          WHERE cp.user_id = ?
            AND cp.id = ?
            AND cp.deleted_at IS NULL
            AND json_valid(cp.provider_config_json)
            AND CAST(json_extract(cp.provider_config_json, '$.nicheResearchRunId') AS TEXT) = ?
            AND CAST(json_extract(cp.provider_config_json, '$.nicheResearchStatus') AS TEXT)
              IN ('queued', 'scraping', 'reviewing')
          LIMIT 1
        )
        "#
}

fn accepted_handles_sql() -> &'static str {
    r#"
        SELECT vr.source_handle
        FROM visual_references vr
        INNER JOIN media_assets ma
          ON ma.id = vr.media_asset_id
         AND ma.storage_key IS NOT NULL
         AND TRIM(ma.storage_key) <> ''
         AND ma.deleted_at IS NULL
        WHERE vr.clone_id = ?
          AND vr.moodboard_id = ?
          AND vr.status = 'active'
          AND vr.source_handle IS NOT NULL
          AND TRIM(vr.source_handle) <> ''
        GROUP BY vr.source_handle
        ORDER BY MAX(vr.created_at) DESC
        LIMIT ?
        "#
}

fn insert_visual_reference_inspiration_pool_sql() -> &'static str {
    r#"
        INSERT OR IGNORE INTO user_inspiration_pool (
          id, user_id, clone_id, moodboard_id, visual_reference_id, discovery_item_id, score, created_at
        )
        SELECT ?, ?, ?, ?, ?, NULL, ?, ?
        WHERE EXISTS (
          SELECT 1
          FROM clone_profiles cp
          WHERE cp.user_id = ?
            AND cp.id = ?
            AND cp.deleted_at IS NULL
            AND json_valid(cp.provider_config_json)
            AND CAST(json_extract(cp.provider_config_json, '$.nicheResearchRunId') AS TEXT) = ?
            AND CAST(json_extract(cp.provider_config_json, '$.nicheResearchStatus') AS TEXT)
              IN ('queued', 'scraping', 'reviewing')
          LIMIT 1
        )
        AND EXISTS (
          SELECT 1
          FROM visual_references vr
          INNER JOIN media_assets ma
            ON ma.id = vr.media_asset_id
           AND ma.storage_key IS NOT NULL
           AND TRIM(ma.storage_key) <> ''
           AND ma.deleted_at IS NULL
          WHERE vr.id = ?
            AND vr.clone_id = ?
            AND vr.status = 'active'
            AND vr.media_asset_id IS NOT NULL
          LIMIT 1
        )
        "#
}

fn load_visual_candidates_for_review_sql() -> &'static str {
    r#"
        SELECT
          id,
          source_handle,
          source_caption,
          source_published_at,
          media_type,
          image_url,
          like_count,
          comment_count,
          moodboard_slug,
          discovered_via,
          review_status,
          review_json
        FROM visual_reference_candidates
        WHERE clone_id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND (
            review_status = 'unreviewed'
            OR (
              review_status = 'review_retryable'
              AND COALESCE(CAST(json_extract(
                CASE WHEN json_valid(review_json) THEN review_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) < ?
            )
            OR (
              review_status = 'reviewing'
              AND reviewed_at IS NOT NULL
              AND reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
            )
          )
          AND image_url IS NOT NULL
          AND TRIM(image_url) <> ''
        ORDER BY
          CASE review_status
            WHEN 'unreviewed' THEN 0
            WHEN 'review_retryable' THEN 1
            ELSE 2
          END ASC,
          reviewed_at ASC,
          created_at ASC
        LIMIT ?
        "#
}

fn claim_visual_candidate_for_review_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = 'reviewing',
            reviewed_at = ?,
            review_json = json_set(
              CASE WHEN json_valid(review_json) THEN review_json ELSE '{}' END,
              '$.claimStatus',
              'reviewing',
              '$.claimStartedAt',
              ?
            )
        WHERE clone_id = ?
          AND id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND (
            review_status IN ('unreviewed', 'review_retryable')
            OR (
              review_status = 'reviewing'
              AND reviewed_at IS NOT NULL
              AND reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
            )
          )
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(review_json) THEN review_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) = ?
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(review_json) THEN review_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) < ?
          AND image_url IS NOT NULL
          AND TRIM(image_url) <> ''
        "#
}

fn cached_accepted_counts_by_moodboard_sql() -> &'static str {
    r#"
        SELECT vr.moodboard_id, COUNT(*) AS count
        FROM visual_references vr
        INNER JOIN media_assets ma
          ON ma.id = vr.media_asset_id
         AND ma.storage_key IS NOT NULL
         AND TRIM(ma.storage_key) <> ''
         AND ma.deleted_at IS NULL
        INNER JOIN user_inspiration_pool uip
          ON uip.clone_id = vr.clone_id
         AND uip.visual_reference_id = vr.id
        WHERE vr.clone_id = ?
          AND vr.status = 'active'
          AND vr.media_asset_id IS NOT NULL
          AND vr.moodboard_id IS NOT NULL
          AND TRIM(vr.moodboard_id) <> ''
        GROUP BY vr.moodboard_id
        "#
}

fn accepted_cached_reference_count_for_handle_sql() -> &'static str {
    r#"
        SELECT COUNT(*) AS count
        FROM visual_references vr
        INNER JOIN media_assets ma
          ON ma.id = vr.media_asset_id
         AND ma.storage_key IS NOT NULL
         AND TRIM(ma.storage_key) <> ''
         AND ma.deleted_at IS NULL
        WHERE vr.clone_id = ?
          AND vr.status = 'active'
          AND vr.media_asset_id IS NOT NULL
          AND lower(vr.source_handle) = lower(?)
        "#
}

fn approved_candidate_count_for_run_sql() -> &'static str {
    r#"
        SELECT COUNT(*) AS count
        FROM visual_reference_candidates
        WHERE clone_id = ?
          AND review_status IN ('cleanup_pending', 'cleanup_retryable', 'compatibility_pending', 'compatibility_retryable', 'cache_pending', 'caching', 'cached')
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
        "#
}

fn approved_candidate_count_for_run_and_handle_sql() -> &'static str {
    r#"
        SELECT COUNT(*) AS count
        FROM visual_reference_candidates
        WHERE clone_id = ?
          AND review_status IN ('cleanup_pending', 'cleanup_retryable', 'compatibility_pending', 'compatibility_retryable', 'cache_pending', 'caching', 'cached')
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND lower(source_handle) = lower(?)
        "#
}

fn approve_visual_candidate_with_cap_guards_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = 'cleanup_pending',
            review_json = ?,
            moodboard_id = ?,
            moodboard_slug = ?,
            rejection_reason = NULL,
            reviewed_at = ?,
            metadata_json = json_set(
              CASE WHEN json_valid(metadata_json) THEN metadata_json ELSE '{}' END,
              '$.runId',
              ?,
              '$.approvedRunId',
              ?
            )
        WHERE clone_id = ?
          AND id = ?
          AND review_status = 'reviewing'
          AND json_valid(visual_reference_candidates.metadata_json)
          AND CAST(json_extract(visual_reference_candidates.metadata_json, '$.runId') AS TEXT) = ?
          AND (
            SELECT COUNT(*)
            FROM visual_reference_candidates approved
            WHERE approved.clone_id = visual_reference_candidates.clone_id
              AND approved.review_status IN ('cleanup_pending', 'cleanup_retryable', 'compatibility_pending', 'compatibility_retryable', 'cache_pending', 'caching', 'cached')
              AND json_valid(approved.metadata_json)
              AND CAST(json_extract(approved.metadata_json, '$.runId') AS TEXT) = ?
              AND CAST(json_extract(approved.metadata_json, '$.approvedRunId') AS TEXT) = ?
          ) < ?
          AND (
            visual_reference_candidates.source_handle IS NULL
            OR TRIM(visual_reference_candidates.source_handle) = ''
            OR (
              (
                SELECT COUNT(*)
                FROM visual_reference_candidates approved_handle
                WHERE approved_handle.clone_id = visual_reference_candidates.clone_id
                  AND approved_handle.review_status IN ('cleanup_pending', 'cleanup_retryable', 'compatibility_pending', 'compatibility_retryable', 'cache_pending', 'caching', 'cached')
                  AND json_valid(approved_handle.metadata_json)
                  AND CAST(json_extract(approved_handle.metadata_json, '$.runId') AS TEXT) = ?
                  AND CAST(json_extract(approved_handle.metadata_json, '$.approvedRunId') AS TEXT) = ?
                  AND lower(approved_handle.source_handle) = lower(visual_reference_candidates.source_handle)
              ) + (
                SELECT COUNT(*)
                FROM visual_references vr
                INNER JOIN media_assets ma
                  ON ma.id = vr.media_asset_id
                 AND ma.storage_key IS NOT NULL
                 AND TRIM(ma.storage_key) <> ''
                 AND ma.deleted_at IS NULL
                WHERE vr.clone_id = visual_reference_candidates.clone_id
                  AND vr.status = 'active'
                  AND vr.media_asset_id IS NOT NULL
                  AND lower(vr.source_handle) = lower(visual_reference_candidates.source_handle)
              )
            ) < ?
          )
        "#
}

fn claim_visual_candidate_for_cleanup_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET cleanup_json = json_set(
              CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
              '$.claimStatus',
              'cleanup_in_progress',
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
          AND (
            COALESCE(CAST(json_extract(
              CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
              '$.claimStatus'
            ) AS TEXT), '') <> 'cleanup_in_progress'
            OR (
              reviewed_at IS NOT NULL
              AND reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
            )
          )
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
              '$.claimStatus',
              'cleanup_completed',
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
          AND CAST(json_extract(
            CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
            '$.claimStatus'
          ) AS TEXT) = 'cleanup_in_progress'
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) = ?
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
              '$.claimStatus',
              CASE
                WHEN COALESCE(CAST(json_extract(
                  CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
                  '$.attempts'
                ) AS INTEGER), 0) < ?
                THEN 'cleanup_retryable'
                ELSE 'cleanup_failed'
              END,
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
          AND CAST(json_extract(
            CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
            '$.claimStatus'
          ) AS TEXT) = 'cleanup_in_progress'
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(cleanup_json) THEN cleanup_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) = ?
        "#
}

fn claim_visual_candidate_for_compatibility_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET compatibility_json = json_set(
              CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
              '$.claimStatus',
              'compatibility_in_progress',
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
          AND (
            COALESCE(CAST(json_extract(
              CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
              '$.claimStatus'
            ) AS TEXT), '') <> 'compatibility_in_progress'
            OR (
              reviewed_at IS NOT NULL
              AND reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
            )
          )
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
            compatibility_json = json_set(
              CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
              '$.claimStatus',
              'compatibility_completed',
              '$.review',
              json(?),
              '$.completedAt',
              ?
            ),
            rejection_reason = NULL,
            reviewed_at = ?
        WHERE id = ?
          AND clone_id = ?
          AND review_status IN ('compatibility_pending', 'compatibility_retryable')
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND CAST(json_extract(
            CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
            '$.claimStatus'
          ) AS TEXT) = 'compatibility_in_progress'
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) = ?
        "#
}

fn mark_candidate_clone_mismatch_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = 'clone_mismatch',
            compatibility_json = json_set(
              CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
              '$.claimStatus',
              'compatibility_mismatched',
              '$.review',
              json(?),
              '$.mismatchedAt',
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
          AND CAST(json_extract(
            CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
            '$.claimStatus'
          ) AS TEXT) = 'compatibility_in_progress'
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) = ?
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
              '$.claimStatus',
              'compatibility_failed',
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
          AND CAST(json_extract(
            CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
            '$.claimStatus'
          ) AS TEXT) = 'compatibility_in_progress'
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) = ?
        "#
}

fn remaining_retryable_visual_candidates_sql() -> &'static str {
    r#"
        SELECT COUNT(*) AS count
        FROM visual_reference_candidates
        WHERE clone_id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND review_status = 'review_retryable'
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(review_json) THEN review_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) < ?
          AND image_url IS NOT NULL
          AND TRIM(image_url) <> ''
        "#
}

fn mark_candidate_review_failed_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = CASE
              WHEN ? = 'ai_upstream_timeout'
               AND COALESCE(CAST(json_extract(
                 CASE WHEN json_valid(review_json) THEN review_json ELSE '{}' END,
                 '$.attempts'
               ) AS INTEGER), 0) + 1 < ?
              THEN 'review_retryable'
              ELSE 'review_failed'
            END,
            review_json = json_set(
              CASE WHEN json_valid(review_json) THEN review_json ELSE '{}' END,
              '$.errorCode',
              ?,
              '$.error',
              ?,
              '$.attempts',
              COALESCE(CAST(json_extract(
                CASE WHEN json_valid(review_json) THEN review_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) + 1
            ),
            rejection_reason = ?,
            reviewed_at = ?
        WHERE id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND review_status = 'reviewing'
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(review_json) THEN review_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) = ?
        "#
}

fn mark_candidate_rejected_with_review_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = 'rejected',
            review_json = ?,
            rejection_reason = ?,
            reviewed_at = ?
        WHERE id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND review_status = 'reviewing'
        "#
}

fn claim_visual_candidate_for_cache_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = 'caching',
            reviewed_at = ?,
            metadata_json = json_set(
              CASE WHEN json_valid(metadata_json) THEN metadata_json ELSE '{}' END,
              '$.cacheRunId',
              ?,
              '$.cacheClaimedAt',
              ?
            )
        WHERE id = ?
          AND clone_id = ?
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND (
            review_status = 'cache_pending'
            OR (
              review_status = 'caching'
              AND reviewed_at IS NOT NULL
              AND reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
            )
          )
          AND NOT EXISTS (
            SELECT 1
            FROM visual_references vr
            INNER JOIN media_assets ma
              ON ma.id = vr.media_asset_id
             AND ma.storage_key IS NOT NULL
             AND TRIM(ma.storage_key) <> ''
             AND ma.deleted_at IS NULL
            WHERE vr.clone_id = ?
              AND vr.candidate_id = ?
              AND vr.status = 'active'
              AND vr.media_asset_id IS NOT NULL
            LIMIT 1
          )
        "#
}

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
          AND json_valid(compatibility_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND CAST(json_extract(compatibility_json, '$.claimStatus') AS TEXT) = 'compatibility_completed'
          AND COALESCE(CAST(json_extract(compatibility_json, '$.review.compatible') AS INTEGER), 0) = 1
          AND cleaned_image_url IS NOT NULL
          AND TRIM(cleaned_image_url) <> ''
        LIMIT 1
        "#
}

fn mark_candidate_cache_failed_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = 'cache_failed',
            rejection_reason = ?,
            reviewed_at = ?
        WHERE id = ?
          AND clone_id = ?
          AND review_status = 'caching'
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.cacheRunId') AS TEXT) = ?
          AND NOT EXISTS (
            SELECT 1
            FROM visual_references vr
            INNER JOIN media_assets ma
              ON ma.id = vr.media_asset_id
             AND ma.storage_key IS NOT NULL
             AND TRIM(ma.storage_key) <> ''
             AND ma.deleted_at IS NULL
            WHERE vr.clone_id = ?
              AND vr.candidate_id = ?
              AND vr.status = 'active'
              AND vr.media_asset_id IS NOT NULL
            LIMIT 1
          )
        "#
}

fn mark_candidate_cache_succeeded_sql() -> &'static str {
    r#"
        UPDATE visual_reference_candidates
        SET review_status = 'cached',
            reviewed_at = ?,
            metadata_json = json_set(
              CASE WHEN json_valid(metadata_json) THEN metadata_json ELSE '{}' END,
              '$.cacheRunId',
              ?,
              '$.cachedRunId',
              ?
            )
        WHERE id = ?
          AND clone_id = ?
          AND review_status = 'caching'
          AND json_valid(metadata_json)
          AND CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND CAST(json_extract(metadata_json, '$.cacheRunId') AS TEXT) = ?
          AND EXISTS (
            SELECT 1
            FROM visual_references vr
            INNER JOIN media_assets ma
              ON ma.id = vr.media_asset_id
             AND ma.storage_key IS NOT NULL
             AND TRIM(ma.storage_key) <> ''
             AND ma.deleted_at IS NULL
            WHERE vr.clone_id = ?
              AND vr.candidate_id = ?
              AND vr.status = 'active'
              AND vr.media_asset_id IS NOT NULL
            LIMIT 1
          )
        "#
}

fn finalize_pending_discovery_work_sql() -> &'static str {
    r#"
        SELECT COUNT(*) AS count
        FROM (
          SELECT ds.id
          FROM discovery_sources ds
          WHERE ds.provider = 'scrapecreators'
            AND ds.status = 'refreshing'
            AND json_valid(ds.params_json)
            AND CAST(json_extract(ds.params_json, '$.cloneId') AS TEXT) = ?
            AND CAST(json_extract(ds.params_json, '$.runId') AS TEXT) = ?
            AND CAST(json_extract(ds.params_json, '$.requestType') AS TEXT)
              IN ('instagram_reels_search', 'instagram_profile', 'instagram_user_posts', 'instagram_post_detail')
          UNION ALL
          SELECT profile.id
          FROM discovery_sources profile
          WHERE profile.provider = 'scrapecreators'
            AND profile.status = 'fresh'
            AND json_valid(profile.params_json)
            AND CAST(json_extract(profile.params_json, '$.cloneId') AS TEXT) = ?
            AND CAST(json_extract(profile.params_json, '$.runId') AS TEXT) = ?
            AND CAST(json_extract(profile.params_json, '$.requestType') AS TEXT) = 'instagram_profile'
            AND NOT EXISTS (
              SELECT 1
              FROM discovery_sources posts
              WHERE posts.provider = 'scrapecreators'
                AND json_valid(posts.params_json)
                AND CAST(json_extract(posts.params_json, '$.cloneId') AS TEXT)
                  = CAST(json_extract(profile.params_json, '$.cloneId') AS TEXT)
                AND CAST(json_extract(posts.params_json, '$.runId') AS TEXT)
                  = CAST(json_extract(profile.params_json, '$.runId') AS TEXT)
                AND CAST(json_extract(posts.params_json, '$.requestType') AS TEXT)
                  = 'instagram_user_posts'
                AND lower(CAST(json_extract(posts.params_json, '$.handle') AS TEXT))
                  = lower(CAST(json_extract(profile.params_json, '$.handle') AS TEXT))
                AND CAST(json_extract(posts.params_json, '$.moodboardId') AS TEXT)
                  = CAST(json_extract(profile.params_json, '$.moodboardId') AS TEXT)
              LIMIT 1
            )
        ) pending
        "#
}

fn finalize_pending_visual_work_sql() -> &'static str {
    r#"
        SELECT COUNT(*) AS count
        FROM visual_reference_candidates vc
        WHERE vc.clone_id = ?
          AND json_valid(vc.metadata_json)
          AND CAST(json_extract(vc.metadata_json, '$.runId') AS TEXT) = ?
          AND vc.image_url IS NOT NULL
          AND TRIM(vc.image_url) <> ''
          AND (
            vc.review_status = 'unreviewed'
            OR (
              vc.review_status = 'review_retryable'
              AND COALESCE(CAST(json_extract(
                CASE WHEN json_valid(vc.review_json) THEN vc.review_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) < ?
            )
            OR (
              vc.review_status = 'reviewing'
              AND vc.reviewed_at IS NOT NULL
              AND vc.reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
            )
            OR (
              vc.review_status = 'cleanup_pending'
              AND COALESCE(CAST(json_extract(
                CASE WHEN json_valid(vc.cleanup_json) THEN vc.cleanup_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) < ?
              AND (
                json_extract(
                  CASE WHEN json_valid(vc.cleanup_json) THEN vc.cleanup_json ELSE '{}' END,
                  '$.claimStartedAt'
                ) IS NULL
                OR vc.reviewed_at IS NULL
                OR vc.reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
              )
            )
            OR (
              vc.review_status = 'cleanup_retryable'
              AND COALESCE(CAST(json_extract(
                CASE WHEN json_valid(vc.cleanup_json) THEN vc.cleanup_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) < ?
              AND (
                json_extract(
                  CASE WHEN json_valid(vc.cleanup_json) THEN vc.cleanup_json ELSE '{}' END,
                  '$.claimStartedAt'
                ) IS NULL
                OR vc.reviewed_at IS NULL
                OR vc.reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
              )
            )
            OR (
              vc.review_status = 'compatibility_pending'
              AND COALESCE(CAST(json_extract(
                CASE WHEN json_valid(vc.compatibility_json) THEN vc.compatibility_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) < ?
              AND (
                json_extract(
                  CASE WHEN json_valid(vc.compatibility_json) THEN vc.compatibility_json ELSE '{}' END,
                  '$.claimStartedAt'
                ) IS NULL
                OR vc.reviewed_at IS NULL
                OR vc.reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
              )
            )
            OR (
              vc.review_status = 'compatibility_retryable'
              AND COALESCE(CAST(json_extract(
                CASE WHEN json_valid(vc.compatibility_json) THEN vc.compatibility_json ELSE '{}' END,
                '$.attempts'
              ) AS INTEGER), 0) < ?
              AND (
                json_extract(
                  CASE WHEN json_valid(vc.compatibility_json) THEN vc.compatibility_json ELSE '{}' END,
                  '$.claimStartedAt'
                ) IS NULL
                OR vc.reviewed_at IS NULL
                OR vc.reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
              )
            )
            OR (
              (
                vc.review_status = 'cache_pending'
                OR (
                  vc.review_status = 'caching'
                  AND vc.reviewed_at IS NOT NULL
                  AND vc.reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
                )
              )
              AND CAST(json_extract(vc.metadata_json, '$.approvedRunId') AS TEXT) = ?
              AND NOT EXISTS (
                SELECT 1
                FROM visual_references vr
                INNER JOIN media_assets ma
                  ON ma.id = vr.media_asset_id
                 AND ma.storage_key IS NOT NULL
                 AND TRIM(ma.storage_key) <> ''
                 AND ma.deleted_at IS NULL
                INNER JOIN user_inspiration_pool uip
                  ON uip.clone_id = vr.clone_id
                 AND uip.visual_reference_id = vr.id
                WHERE vr.clone_id = vc.clone_id
                  AND vr.candidate_id = vc.id
                  AND vr.status = 'active'
                  AND vr.media_asset_id IS NOT NULL
                LIMIT 1
              )
            )
          )
        "#
}

fn finalize_in_progress_visual_work_sql() -> &'static str {
    r#"
        SELECT COUNT(*) AS count
        FROM visual_reference_candidates vc
        WHERE vc.clone_id = ?
          AND json_valid(vc.metadata_json)
          AND CAST(json_extract(vc.metadata_json, '$.runId') AS TEXT) = ?
          AND (
            vc.reviewed_at IS NULL
            OR vc.reviewed_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
          )
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
          AND (
            vc.review_status <> 'caching'
            OR NOT EXISTS (
              SELECT 1
              FROM visual_references vr
              INNER JOIN media_assets ma
                ON ma.id = vr.media_asset_id
               AND ma.storage_key IS NOT NULL
               AND TRIM(ma.storage_key) <> ''
               AND ma.deleted_at IS NULL
              INNER JOIN user_inspiration_pool uip
                ON uip.clone_id = vr.clone_id
               AND uip.visual_reference_id = vr.id
              WHERE vr.clone_id = vc.clone_id
                AND vr.candidate_id = vc.id
                AND vr.status = 'active'
                AND vr.media_asset_id IS NOT NULL
              LIMIT 1
            )
          )
        "#
}

fn finalize_cleanup_candidates_sql() -> &'static str {
    r#"
        SELECT vc.id
        FROM visual_reference_candidates vc
        WHERE vc.clone_id = ?
          AND vc.review_status IN ('cleanup_pending', 'cleanup_retryable')
          AND json_valid(vc.metadata_json)
          AND CAST(json_extract(vc.metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(vc.metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(vc.cleanup_json) THEN vc.cleanup_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) < ?
          AND (
            json_extract(
              CASE WHEN json_valid(vc.cleanup_json) THEN vc.cleanup_json ELSE '{}' END,
              '$.claimStartedAt'
            ) IS NULL
            OR vc.reviewed_at IS NULL
            OR vc.reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
          )
          AND vc.image_url IS NOT NULL
          AND TRIM(vc.image_url) <> ''
        ORDER BY vc.reviewed_at ASC, vc.created_at ASC
        LIMIT ?
        "#
}

fn finalize_compatibility_candidates_sql() -> &'static str {
    r#"
        SELECT vc.id
        FROM visual_reference_candidates vc
        WHERE vc.clone_id = ?
          AND vc.review_status IN ('compatibility_pending', 'compatibility_retryable')
          AND json_valid(vc.metadata_json)
          AND CAST(json_extract(vc.metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(vc.metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND COALESCE(CAST(json_extract(
            CASE WHEN json_valid(vc.compatibility_json) THEN vc.compatibility_json ELSE '{}' END,
            '$.attempts'
          ) AS INTEGER), 0) < ?
          AND (
            json_extract(
              CASE WHEN json_valid(vc.compatibility_json) THEN vc.compatibility_json ELSE '{}' END,
              '$.claimStartedAt'
            ) IS NULL
            OR vc.reviewed_at IS NULL
            OR vc.reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
          )
          AND vc.cleaned_image_url IS NOT NULL
          AND TRIM(vc.cleaned_image_url) <> ''
        ORDER BY vc.reviewed_at ASC, vc.created_at ASC
        LIMIT ?
        "#
}

fn finalize_approved_uncached_candidates_sql() -> &'static str {
    r#"
        SELECT vc.id
        FROM visual_reference_candidates vc
        WHERE vc.clone_id = ?
          AND (
            vc.review_status = 'cache_pending'
            OR (
              vc.review_status = 'caching'
              AND vc.reviewed_at IS NOT NULL
              AND vc.reviewed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
            )
          )
          AND json_valid(vc.metadata_json)
          AND CAST(json_extract(vc.metadata_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(vc.metadata_json, '$.approvedRunId') AS TEXT) = ?
          AND vc.image_url IS NOT NULL
          AND TRIM(vc.image_url) <> ''
          AND NOT EXISTS (
            SELECT 1
            FROM visual_references vr
            INNER JOIN media_assets ma
              ON ma.id = vr.media_asset_id
             AND ma.storage_key IS NOT NULL
             AND TRIM(ma.storage_key) <> ''
             AND ma.deleted_at IS NULL
            INNER JOIN user_inspiration_pool uip
              ON uip.clone_id = vr.clone_id
             AND uip.visual_reference_id = vr.id
            WHERE vr.clone_id = vc.clone_id
              AND vr.candidate_id = vc.id
              AND vr.status = 'active'
              AND vr.media_asset_id IS NOT NULL
            LIMIT 1
          )
        ORDER BY vc.reviewed_at ASC, vc.created_at ASC
        LIMIT ?
        "#
}

fn instagram_profile_sources_for_run_sql() -> &'static str {
    r#"
        SELECT COUNT(*) AS count
        FROM discovery_sources
        WHERE provider = 'scrapecreators'
          AND json_valid(params_json)
          AND CAST(json_extract(params_json, '$.cloneId') AS TEXT) = ?
          AND CAST(json_extract(params_json, '$.runId') AS TEXT) = ?
          AND CAST(json_extract(params_json, '$.requestType') AS TEXT) = 'instagram_profile'
        "#
}

fn reserve_instagram_profile_source_sql() -> &'static str {
    r#"
        INSERT OR IGNORE INTO discovery_sources (
          id, provider, source, params_json, refreshed_at, status
        )
        SELECT ?, 'scrapecreators', ?, ?, ?, 'refreshing'
        WHERE (
          SELECT COUNT(*)
          FROM discovery_sources
          WHERE provider = 'scrapecreators'
            AND json_valid(params_json)
            AND CAST(json_extract(params_json, '$.cloneId') AS TEXT) = ?
            AND CAST(json_extract(params_json, '$.runId') AS TEXT) = ?
            AND CAST(json_extract(params_json, '$.requestType') AS TEXT) = 'instagram_profile'
        ) < ?
        "#
}

fn discovery_source_status_sql() -> &'static str {
    r#"
        SELECT status
        FROM discovery_sources
        WHERE id = ?
          AND provider = 'scrapecreators'
          AND source = ?
          AND params_json = ?
        LIMIT 1
        "#
}

async fn load_accepted_handles(
    db: &D1Database,
    clone_id: &str,
    moodboard_id: &str,
    limit: u32,
) -> WorkerResult<Vec<String>> {
    let rows = db::all::<AcceptedHandleRow>(
        db,
        accepted_handles_sql(),
        vec![json!(clone_id), json!(moodboard_id), json!(limit)],
    )
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| normalize_instagram_handle(&row.source_handle))
        .collect())
}

fn instagram_profile_source_params(
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    discovered_via: &str,
    related_depth: u8,
) -> Value {
    json!({
        "cloneId": clone_id,
        "userId": user_id,
        "runId": run_id,
        "platform": "instagram",
        "moodboardId": moodboard_id,
        "moodboardSlug": moodboard_slug,
        "handle": normalize_instagram_handle(handle).unwrap_or_else(|| handle.to_string()),
        "discoveredVia": discovered_via,
        "relatedDepth": related_depth,
        "requestType": "instagram_profile",
    })
}

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

fn instagram_posts_source_params(
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    discovered_via: &str,
    next_max_id: Option<&str>,
    page: u8,
) -> Value {
    json!({
        "cloneId": clone_id,
        "userId": user_id,
        "runId": run_id,
        "platform": "instagram",
        "moodboardId": moodboard_id,
        "moodboardSlug": moodboard_slug,
        "handle": normalize_instagram_handle(handle).unwrap_or_else(|| handle.to_string()),
        "discoveredVia": discovered_via,
        "nextMaxId": next_max_id,
        "page": page,
        "requestType": "instagram_user_posts",
    })
}

fn instagram_post_detail_source_params(
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    discovered_via: &str,
    source_url: &str,
) -> Value {
    json!({
        "cloneId": clone_id,
        "userId": user_id,
        "runId": run_id,
        "platform": "instagram",
        "moodboardId": moodboard_id,
        "moodboardSlug": moodboard_slug,
        "handle": normalize_instagram_handle(handle).unwrap_or_else(|| handle.to_string()),
        "discoveredVia": discovered_via,
        "sourceUrl": source_url,
        "requestType": "instagram_post_detail",
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiscoveryReservationAction {
    Send,
    Skip,
}

fn discovery_reservation_action(
    inserted: bool,
    existing_status: Option<&str>,
) -> DiscoveryReservationAction {
    if inserted || matches!(existing_status.map(str::trim), Some("refreshing")) {
        DiscoveryReservationAction::Send
    } else {
        DiscoveryReservationAction::Skip
    }
}

async fn reserve_instagram_profile_source(
    db: &D1Database,
    base_url: &str,
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    discovered_via: &str,
    related_depth: u8,
    max_profiles_per_run: usize,
    now: &str,
) -> WorkerResult<bool> {
    let request_url = build_instagram_profile_url(base_url, handle)
        .map_err(|error| Error::RustError(error.to_string()))?;
    let params = instagram_profile_source_params(
        user_id,
        clone_id,
        run_id,
        moodboard_id,
        moodboard_slug,
        handle,
        discovered_via,
        related_depth,
    );
    let params_json = params.to_string();
    let source_id = deterministic_id(
        "discovery_source",
        &["scrapecreators", &request_url, &params_json],
    );
    let result = db::run(
        db,
        reserve_instagram_profile_source_sql(),
        vec![
            json!(source_id),
            json!(request_url),
            json!(params_json),
            json!(now),
            json!(clone_id),
            json!(run_id),
            json!(max_profiles_per_run as u32),
        ],
    )
    .await?;
    discovery_reservation_should_send(
        db,
        &source_id,
        &request_url,
        &params_json,
        changed_rows(&result)? > 0,
    )
    .await
}

async fn reserve_instagram_reels_search_source(
    db: &D1Database,
    base_url: &str,
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    search_term: &str,
    page: u32,
    now: &str,
) -> WorkerResult<bool> {
    let request_url = build_instagram_reels_search_url(base_url, search_term, Some(page))
        .map_err(|error| Error::RustError(error.to_string()))?;
    let params = instagram_reels_search_source_params(
        user_id,
        clone_id,
        run_id,
        moodboard_id,
        moodboard_slug,
        search_term,
        page,
    );
    reserve_discovery_source_if_missing(db, &request_url, &params, now).await
}

async fn reserve_instagram_posts_source(
    db: &D1Database,
    base_url: &str,
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    discovered_via: &str,
    next_max_id: Option<&str>,
    page: u8,
    now: &str,
) -> WorkerResult<String> {
    let request_url = build_instagram_user_posts_url(base_url, handle, next_max_id)
        .map_err(|error| Error::RustError(error.to_string()))?;
    let params = instagram_posts_source_params(
        user_id,
        clone_id,
        run_id,
        moodboard_id,
        moodboard_slug,
        handle,
        discovered_via,
        next_max_id,
        page,
    );
    reserve_discovery_source(db, &request_url, &params, now).await
}

async fn reserve_instagram_post_detail_source(
    db: &D1Database,
    base_url: &str,
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    moodboard_id: &str,
    moodboard_slug: &str,
    handle: &str,
    discovered_via: &str,
    source_url: &str,
    now: &str,
) -> WorkerResult<bool> {
    let request_url = build_instagram_post_url(base_url, source_url, "US")
        .map_err(|error| Error::RustError(error.to_string()))?;
    let params = instagram_post_detail_source_params(
        user_id,
        clone_id,
        run_id,
        moodboard_id,
        moodboard_slug,
        handle,
        discovered_via,
        source_url,
    );
    reserve_discovery_source_if_missing(db, &request_url, &params, now).await
}

async fn reserve_discovery_source(
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
        INSERT OR IGNORE INTO discovery_sources (
          id, provider, source, params_json, refreshed_at, status
        )
        VALUES (?, 'scrapecreators', ?, ?, ?, 'refreshing')
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

async fn reserve_discovery_source_if_missing(
    db: &D1Database,
    source: &str,
    params: &Value,
    now: &str,
) -> WorkerResult<bool> {
    let params_json = params.to_string();
    let source_id = deterministic_id(
        "discovery_source",
        &["scrapecreators", source, &params_json],
    );
    let result = db::run(
        db,
        r#"
        INSERT OR IGNORE INTO discovery_sources (
          id, provider, source, params_json, refreshed_at, status
        )
        VALUES (?, 'scrapecreators', ?, ?, ?, 'refreshing')
        "#,
        vec![
            json!(source_id),
            json!(source),
            json!(params_json),
            json!(now),
        ],
    )
    .await?;
    discovery_reservation_should_send(
        db,
        &source_id,
        source,
        &params_json,
        changed_rows(&result)? > 0,
    )
    .await
}

async fn discovery_reservation_should_send(
    db: &D1Database,
    source_id: &str,
    source: &str,
    params_json: &str,
    inserted: bool,
) -> WorkerResult<bool> {
    let existing_status = if inserted {
        None
    } else {
        load_discovery_source_status(db, source_id, source, params_json).await?
    };
    Ok(matches!(
        discovery_reservation_action(inserted, existing_status.as_deref()),
        DiscoveryReservationAction::Send
    ))
}

async fn load_discovery_source_status(
    db: &D1Database,
    source_id: &str,
    source: &str,
    params_json: &str,
) -> WorkerResult<Option<String>> {
    Ok(db::first::<DiscoverySourceStatusRow>(
        db,
        discovery_source_status_sql(),
        vec![json!(source_id), json!(source), json!(params_json)],
    )
    .await?
    .map(|row| row.status))
}

async fn count_instagram_profile_sources_for_run(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
) -> WorkerResult<usize> {
    let row = db::first::<CountRow>(
        db,
        instagram_profile_sources_for_run_sql(),
        vec![json!(clone_id), json!(run_id)],
    )
    .await?;
    Ok(row.map(|row| row.count as usize).unwrap_or_default())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrapeSourceFailureAction {
    FinalizeRun,
}

fn scrapecreators_source_failure_action(_error: &ScrapeCreatorsError) -> ScrapeSourceFailureAction {
    ScrapeSourceFailureAction::FinalizeRun
}

async fn handle_scrapecreators_source_failure(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    source_id: &str,
    params: &Value,
    error: &ScrapeCreatorsError,
    now: &str,
    reason: &str,
) -> WorkerResult<()> {
    mark_discovery_source_failed(db, source_id, params, &error.to_string(), now).await?;
    match scrapecreators_source_failure_action(error) {
        ScrapeSourceFailureAction::FinalizeRun => {
            env.queue("NICHE_RESEARCH_QUEUE")?
                .send(NicheResearchMessage::FinalizeReferencePool {
                    user_id: user_id.to_string(),
                    clone_id: clone_id.to_string(),
                    run_id: Some(run_id.to_string()),
                    reason: format!("{reason}:{}", queue_error_code(&error.to_string())),
                })
                .await?;
            Ok(())
        }
    }
}

async fn insert_instagram_candidate(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    source_id: &str,
    candidate: &InstagramImageCandidate,
    now: &str,
) -> WorkerResult<String> {
    let candidate_id = deterministic_id(
        "visual_candidate",
        &[
            clone_id,
            &candidate.source_handle,
            &candidate.source_post_code,
            &candidate.source_image_index.to_string(),
        ],
    );
    db::exec(
        db,
        insert_visual_candidate_sql(),
        vec![
            json!(candidate_id),
            json!(user_id),
            json!(clone_id),
            json!(candidate.source_handle),
            json!(candidate.source_profile_id),
            json!(candidate.source_post_id),
            json!(candidate.source_post_code),
            json!(candidate.source_image_index),
            json!(candidate.source_url),
            json!(candidate.source_published_at),
            json!(candidate.source_caption),
            json!(candidate.media_type),
            json!(candidate.image_url),
            json!(candidate.image_width),
            json!(candidate.image_height),
            json!(candidate.like_count),
            json!(candidate.comment_count),
            json!(candidate.play_count),
            json!(candidate.moodboard_id),
            json!(candidate.moodboard_slug),
            json!(candidate.discovered_via),
            json!(candidate.raw_json.to_string()),
            json!(json!({
                "sourceId": source_id,
                "sourcePlatform": candidate.platform,
                "runId": run_id,
            })
            .to_string()),
            json!(now),
        ],
    )
    .await?;
    Ok(candidate_id)
}

async fn load_unreviewed_visual_candidates(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    review_retry_limit: u32,
    limit: u32,
) -> WorkerResult<Vec<VisualCandidateReviewRow>> {
    db::all(
        db,
        load_visual_candidates_for_review_sql(),
        vec![
            json!(clone_id),
            json!(run_id),
            json!(review_retry_limit),
            json!(limit),
        ],
    )
    .await
}

async fn has_remaining_retryable_visual_candidates(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    review_retry_limit: u32,
) -> WorkerResult<bool> {
    let row = db::first::<CountRow>(
        db,
        remaining_retryable_visual_candidates_sql(),
        vec![json!(clone_id), json!(run_id), json!(review_retry_limit)],
    )
    .await?;
    Ok(row.map(|row| row.count > 0).unwrap_or_default())
}

async fn claim_visual_candidate_for_review(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
    observed_attempts: u32,
    review_retry_limit: u32,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        claim_visual_candidate_for_review_sql(),
        vec![
            json!(now),
            json!(now),
            json!(clone_id),
            json!(candidate_id),
            json!(run_id),
            json!(observed_attempts),
            json!(review_retry_limit),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

#[derive(Debug, Default, PartialEq, Eq)]
struct FinalizeDrainState {
    pending_discovery: u32,
    pending_visual_work: u32,
    in_progress_visual_work: u32,
    cleanup_candidate_ids: Vec<String>,
    compatibility_candidate_ids: Vec<String>,
    approved_uncached_candidate_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinalizeDrainAction {
    Proceed,
    EnqueueCleanup,
    EnqueueCompatibility,
    EnqueueCache,
    EnqueueReview,
    EnqueueFinalize,
    EnqueueDelayedFinalize,
}

fn finalize_drain_action(state: &FinalizeDrainState) -> FinalizeDrainAction {
    if !state.cleanup_candidate_ids.is_empty() {
        FinalizeDrainAction::EnqueueCleanup
    } else if !state.compatibility_candidate_ids.is_empty() {
        FinalizeDrainAction::EnqueueCompatibility
    } else if !state.approved_uncached_candidate_ids.is_empty() {
        FinalizeDrainAction::EnqueueCache
    } else if state.pending_visual_work > 0 {
        FinalizeDrainAction::EnqueueReview
    } else if state.in_progress_visual_work > 0 {
        FinalizeDrainAction::EnqueueDelayedFinalize
    } else if state.pending_discovery > 0 {
        FinalizeDrainAction::EnqueueFinalize
    } else {
        FinalizeDrainAction::Proceed
    }
}

fn reference_pool_readiness_phase(
    _accepted_refs: u32,
    ready_moodboards: usize,
    selected_moodboards: usize,
) -> ResearchPhase {
    if selected_moodboards > 0 && ready_moodboards >= selected_moodboards {
        ResearchPhase::PoolReady
    } else if ready_moodboards > 0 {
        ResearchPhase::PartialPoolReady
    } else {
        ResearchPhase::InsufficientRefs
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinalizeReadinessAction {
    OrchestrateBatchBeforeStatus,
    WriteStatusOnly,
}

fn finalize_readiness_action(
    phase: ResearchPhase,
    soul_status: &str,
    provider_soul_id: Option<&str>,
) -> FinalizeReadinessAction {
    if matches!(
        phase,
        ResearchPhase::PoolReady | ResearchPhase::PartialPoolReady
    ) && soul_status == "ready"
        && provider_soul_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
    {
        FinalizeReadinessAction::OrchestrateBatchBeforeStatus
    } else {
        FinalizeReadinessAction::WriteStatusOnly
    }
}

async fn load_finalize_drain_state(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    review_retry_limit: u32,
    cleanup_retry_limit: u32,
    compatibility_retry_limit: u32,
    cache_limit: u32,
) -> WorkerResult<FinalizeDrainState> {
    let pending_discovery = db::first::<CountRow>(
        db,
        finalize_pending_discovery_work_sql(),
        vec![
            json!(clone_id),
            json!(run_id),
            json!(clone_id),
            json!(run_id),
        ],
    )
    .await?
    .map(|row| row.count)
    .unwrap_or_default();
    let pending_visual_work = db::first::<CountRow>(
        db,
        finalize_pending_visual_work_sql(),
        vec![
            json!(clone_id),
            json!(run_id),
            json!(review_retry_limit),
            json!(cleanup_retry_limit),
            json!(cleanup_retry_limit),
            json!(compatibility_retry_limit),
            json!(compatibility_retry_limit),
            json!(run_id),
        ],
    )
    .await?
    .map(|row| row.count)
    .unwrap_or_default();
    let in_progress_visual_work = db::first::<CountRow>(
        db,
        finalize_in_progress_visual_work_sql(),
        vec![json!(clone_id), json!(run_id)],
    )
    .await?
    .map(|row| row.count)
    .unwrap_or_default();
    let cleanup_candidate_ids = db::all::<IdRow>(
        db,
        finalize_cleanup_candidates_sql(),
        vec![
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(cleanup_retry_limit),
            json!(cache_limit),
        ],
    )
    .await?
    .into_iter()
    .map(|row| row.id)
    .collect();
    let compatibility_candidate_ids = db::all::<IdRow>(
        db,
        finalize_compatibility_candidates_sql(),
        vec![
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(compatibility_retry_limit),
            json!(cache_limit),
        ],
    )
    .await?
    .into_iter()
    .map(|row| row.id)
    .collect();
    let approved_uncached_candidate_ids = db::all::<IdRow>(
        db,
        finalize_approved_uncached_candidates_sql(),
        vec![
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(cache_limit),
        ],
    )
    .await?
    .into_iter()
    .map(|row| row.id)
    .collect();

    Ok(FinalizeDrainState {
        pending_discovery,
        pending_visual_work,
        in_progress_visual_work,
        cleanup_candidate_ids,
        compatibility_candidate_ids,
        approved_uncached_candidate_ids,
    })
}

async fn load_candidate_failure_counts(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
) -> WorkerResult<CandidateFailureCountsRow> {
    Ok(db::first(
        db,
        r#"
        SELECT
          COALESCE(SUM(CASE WHEN review_status = 'cleanup_failed' THEN 1 ELSE 0 END), 0) AS cleanup_failed,
          COALESCE(SUM(CASE WHEN review_status = 'compatibility_failed' THEN 1 ELSE 0 END), 0) AS compatibility_failed,
          COALESCE(SUM(CASE WHEN review_status = 'clone_mismatch' THEN 1 ELSE 0 END), 0) AS clone_mismatch,
          COALESCE(SUM(CASE WHEN review_status = 'cache_failed' THEN 1 ELSE 0 END), 0) AS cache_failed
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReviewFailureOutcome {
    Retryable,
    Failed,
}

fn review_failure_outcome(
    error_code: &str,
    current_attempts: u32,
    review_retry_limit: u32,
) -> ReviewFailureOutcome {
    if error_code == "ai_upstream_timeout"
        && current_attempts.saturating_add(1) < review_retry_limit
    {
        ReviewFailureOutcome::Retryable
    } else {
        ReviewFailureOutcome::Failed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReviewCompletionAction {
    WaitForCache,
    EnqueueRetry,
    Finalize,
}

fn review_completion_action(
    cache_messages_enqueued: usize,
    retryable_follow_up_needed: bool,
) -> ReviewCompletionAction {
    if retryable_follow_up_needed {
        ReviewCompletionAction::EnqueueRetry
    } else if cache_messages_enqueued > 0 {
        ReviewCompletionAction::WaitForCache
    } else {
        ReviewCompletionAction::Finalize
    }
}

fn review_completion_schedules_finalize_nudge(action: ReviewCompletionAction) -> bool {
    matches!(action, ReviewCompletionAction::WaitForCache)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CacheClaimAction {
    Cache,
    EnqueueDelayedFinalize,
}

fn cache_claim_action(claimed: bool) -> CacheClaimAction {
    if claimed {
        CacheClaimAction::Cache
    } else {
        CacheClaimAction::EnqueueDelayedFinalize
    }
}

fn review_attempts_from_json(review_json: &str) -> u32 {
    serde_json::from_str::<Value>(review_json)
        .ok()
        .and_then(|value| {
            value
                .get("attempts")
                .and_then(Value::as_u64)
                .and_then(|attempts| u32::try_from(attempts).ok())
        })
        .unwrap_or_default()
}

fn attempts_from_json(value: &str) -> u32 {
    serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|value| value.get("attempts").and_then(Value::as_u64))
        .and_then(|attempts| u32::try_from(attempts).ok())
        .unwrap_or_default()
}

async fn mark_candidate_review_failed(
    db: &D1Database,
    candidate_id: &str,
    run_id: &str,
    code: &str,
    error: &str,
    review_retry_limit: u32,
    observed_attempts: u32,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        mark_candidate_review_failed_sql(),
        vec![
            json!(code),
            json!(review_retry_limit),
            json!(code),
            json!(compact_error_detail(error)),
            json!(code),
            json!(now),
            json!(candidate_id),
            json!(run_id),
            json!(observed_attempts),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn load_candidate_for_cleanup(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
) -> WorkerResult<Option<CleanupCandidateRow>> {
    db::first(
        db,
        r#"
        SELECT id, image_url, cleanup_json
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
        vec![
            json!(clone_id),
            json!(candidate_id),
            json!(run_id),
            json!(run_id),
        ],
    )
    .await
}

async fn claim_visual_candidate_for_cleanup(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
    observed_attempts: u32,
    cleanup_retry_limit: u32,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        claim_visual_candidate_for_cleanup_sql(),
        vec![
            json!(now),
            json!(now),
            json!(candidate_id),
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(observed_attempts),
            json!(cleanup_retry_limit),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn mark_candidate_cleanup_succeeded(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
    cleaned_image_url: &str,
    provider_job_id: &str,
    expected_attempts: u32,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        mark_candidate_cleanup_succeeded_sql(),
        vec![
            json!(cleaned_image_url),
            json!(cleaned_image_url),
            json!(provider_job_id),
            json!(now),
            json!(now),
            json!(candidate_id),
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(expected_attempts),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn mark_candidate_cleanup_failed(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
    cleanup_retry_limit: u32,
    expected_attempts: u32,
    code: &str,
    error: &str,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let compact_error = compact_error_detail(error);
    let result = db::run(
        db,
        mark_candidate_cleanup_failed_sql(),
        vec![
            json!(cleanup_retry_limit),
            json!(cleanup_retry_limit),
            json!(code),
            json!(compact_error),
            json!(now),
            json!(format!("visual_reference_cleanup_failed:{compact_error}")),
            json!(now),
            json!(candidate_id),
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(expected_attempts),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

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
        vec![
            json!(clone_id),
            json!(candidate_id),
            json!(run_id),
            json!(run_id),
        ],
    )
    .await
}

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
        let body = object.body().ok_or_else(|| {
            Error::RustError("clone_compatibility_reference_body_missing".to_string())
        })?;
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

async fn claim_visual_candidate_for_compatibility(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
    observed_attempts: u32,
    compatibility_retry_limit: u32,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        claim_visual_candidate_for_compatibility_sql(),
        vec![
            json!(now),
            json!(now),
            json!(candidate_id),
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(observed_attempts),
            json!(compatibility_retry_limit),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn mark_candidate_compatibility_succeeded(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
    compatibility_json: &str,
    expected_attempts: u32,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        mark_candidate_compatibility_succeeded_sql(),
        vec![
            json!(compatibility_json),
            json!(now),
            json!(now),
            json!(candidate_id),
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(expected_attempts),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn mark_candidate_clone_mismatch(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
    compatibility_json: &str,
    reason: &str,
    expected_attempts: u32,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        mark_candidate_clone_mismatch_sql(),
        vec![
            json!(compatibility_json),
            json!(now),
            json!(reason),
            json!(now),
            json!(candidate_id),
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(expected_attempts),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn mark_candidate_compatibility_failed(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
    compatibility_retry_limit: u32,
    expected_attempts: u32,
    code: &str,
    error: &str,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let compact_error = compact_error_detail(error);
    let result = db::run(
        db,
        mark_candidate_compatibility_failed_sql(),
        vec![
            json!(compatibility_retry_limit),
            json!(code),
            json!(compact_error),
            json!(now),
            json!(format!("clone_compatibility_failed:{compact_error}")),
            json!(now),
            json!(candidate_id),
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(expected_attempts),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

fn clone_compatibility_error_code(error: &str) -> &'static str {
    if error.contains("clone_compatibility_reference_missing") {
        "clone_compatibility_reference_missing"
    } else if error.contains("clone_compatibility_reference_body_missing") {
        "clone_compatibility_reference_body_missing"
    } else {
        queue_error_code(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuardedCandidateApproval {
    Approved,
    RunCapReached,
    HandleCapReached,
    Skipped,
}

async fn mark_candidate_approved_with_cap_guards(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
    source_handle: Option<&str>,
    review_json: &str,
    accepted: &crate::domain::visual_reference::AcceptedVisualReview,
    max_accepted_refs_per_run: u32,
    accepted_refs_per_profile_cap: u32,
) -> WorkerResult<GuardedCandidateApproval> {
    let now = now_iso_string();
    let result = db::run(
        db,
        approve_visual_candidate_with_cap_guards_sql(),
        vec![
            json!(review_json),
            json!(accepted.moodboard_id),
            json!(accepted.moodboard_slug),
            json!(now),
            json!(run_id),
            json!(run_id),
            json!(clone_id),
            json!(candidate_id),
            json!(run_id),
            json!(run_id),
            json!(run_id),
            json!(max_accepted_refs_per_run),
            json!(run_id),
            json!(run_id),
            json!(accepted_refs_per_profile_cap),
        ],
    )
    .await?;
    if changed_rows(&result)? > 0 {
        return Ok(GuardedCandidateApproval::Approved);
    }

    let handle_count = accepted_cached_reference_count_for_handle(db, clone_id, source_handle)
        .await?
        + approved_candidate_count_for_run_and_handle(db, clone_id, run_id, source_handle).await?;
    let run_count = approved_candidate_count_for_run(db, clone_id, run_id).await?;
    Ok(
        match accepted_reference_cap_decision(
            run_count,
            max_accepted_refs_per_run,
            handle_count,
            accepted_refs_per_profile_cap,
        ) {
            AcceptedReferenceCapDecision::Allow => GuardedCandidateApproval::Skipped,
            AcceptedReferenceCapDecision::RunCapReached => GuardedCandidateApproval::RunCapReached,
            AcceptedReferenceCapDecision::HandleCapReached => {
                GuardedCandidateApproval::HandleCapReached
            }
        },
    )
}

async fn mark_candidate_rejected_with_review(
    db: &D1Database,
    candidate_id: &str,
    run_id: &str,
    review_json: &str,
    reason: &str,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        mark_candidate_rejected_with_review_sql(),
        vec![
            json!(review_json),
            json!(reason),
            json!(now),
            json!(candidate_id),
            json!(run_id),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn mark_candidate_cache_failed(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
    error: &str,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        mark_candidate_cache_failed_sql(),
        vec![
            json!(format!(
                "visual_reference_cache_failed:{}",
                compact_error_detail(error)
            )),
            json!(now),
            json!(candidate_id),
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(run_id),
            json!(clone_id),
            json!(candidate_id),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn mark_candidate_cache_succeeded(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        mark_candidate_cache_succeeded_sql(),
        vec![
            json!(now),
            json!(run_id),
            json!(run_id),
            json!(candidate_id),
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(run_id),
            json!(clone_id),
            json!(candidate_id),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn claim_visual_candidate_for_cache(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        claim_visual_candidate_for_cache_sql(),
        vec![
            json!(now),
            json!(run_id),
            json!(now),
            json!(candidate_id),
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(clone_id),
            json!(candidate_id),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn load_approved_candidate_for_cache(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    candidate_id: &str,
) -> WorkerResult<Option<ApprovedVisualCandidateRow>> {
    db::first(
        db,
        load_approved_candidate_for_cache_sql(),
        vec![
            json!(clone_id),
            json!(candidate_id),
            json!(run_id),
            json!(run_id),
        ],
    )
    .await
}

async fn insert_approved_visual_reference(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    visual_reference_id: &str,
    media_asset_id: &str,
    candidate: &ApprovedVisualCandidateRow,
    moodboard_id: &str,
    moodboard_slug: &str,
    review: &VisualReferenceReview,
    now: &str,
) -> WorkerResult<bool> {
    let result = db::run(
        db,
        insert_visual_reference_sql(),
        vec![
            json!(visual_reference_id),
            json!(user_id),
            json!(clone_id),
            json!(candidate.id),
            json!(media_asset_id),
            json!(candidate.source_handle),
            json!(candidate.source_post_code),
            json!(candidate.source_url),
            json!(candidate.source_published_at),
            json!(candidate.image_width),
            json!(candidate.image_height),
            json!(moodboard_id),
            json!(moodboard_slug),
            json!(moodboard_slug),
            json!(review.visual_fit_score),
            json!(review.pose),
            json!(review.scene),
            json!(review.lighting),
            json!(review.framing),
            json!(review.camera_feel),
            json!(review.styling_direction),
            json!(serde_json::to_string(&visual_review_tags(review))
                .unwrap_or_else(|_| "[]".to_string())),
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(run_id),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn accepted_cached_reference_count_for_handle(
    db: &D1Database,
    clone_id: &str,
    source_handle: Option<&str>,
) -> WorkerResult<u32> {
    let Some(source_handle) = source_handle
        .and_then(normalize_instagram_handle)
        .filter(|handle| !handle.trim().is_empty())
    else {
        return Ok(0);
    };
    let row = db::first::<CountRow>(
        db,
        accepted_cached_reference_count_for_handle_sql(),
        vec![json!(clone_id), json!(source_handle)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or_default())
}

async fn approved_candidate_count_for_run(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        approved_candidate_count_for_run_sql(),
        vec![json!(clone_id), json!(run_id), json!(run_id)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or_default())
}

async fn approved_candidate_count_for_run_and_handle(
    db: &D1Database,
    clone_id: &str,
    run_id: &str,
    source_handle: Option<&str>,
) -> WorkerResult<u32> {
    let Some(source_handle) = source_handle
        .and_then(normalize_instagram_handle)
        .filter(|handle| !handle.trim().is_empty())
    else {
        return Ok(0);
    };
    let row = db::first::<CountRow>(
        db,
        approved_candidate_count_for_run_and_handle_sql(),
        vec![
            json!(clone_id),
            json!(run_id),
            json!(run_id),
            json!(source_handle),
        ],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or_default())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AcceptedReferenceCapDecision {
    Allow,
    RunCapReached,
    HandleCapReached,
}

fn accepted_reference_cap_decision(
    accepted_refs_this_run: u32,
    max_accepted_refs_per_run: u32,
    accepted_refs_for_handle: u32,
    accepted_refs_per_profile_cap: u32,
) -> AcceptedReferenceCapDecision {
    if accepted_refs_this_run >= max_accepted_refs_per_run {
        AcceptedReferenceCapDecision::RunCapReached
    } else if accepted_refs_for_handle >= accepted_refs_per_profile_cap {
        AcceptedReferenceCapDecision::HandleCapReached
    } else {
        AcceptedReferenceCapDecision::Allow
    }
}

async fn insert_visual_reference_inspiration_pool_row(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    moodboard_id: &str,
    visual_reference_id: &str,
    score: f64,
    now: &str,
) -> WorkerResult<bool> {
    let id = deterministic_id("inspiration_pool", &[clone_id, visual_reference_id]);
    let result = db::run(
        db,
        insert_visual_reference_inspiration_pool_sql(),
        vec![
            json!(id),
            json!(user_id),
            json!(clone_id),
            json!(moodboard_id),
            json!(visual_reference_id),
            json!(score),
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(run_id),
            json!(visual_reference_id),
            json!(clone_id),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn repair_cached_visual_reference_inspiration_pool(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    run_id: &str,
    candidate: &ApprovedVisualCandidateRow,
) -> WorkerResult<bool> {
    let review = serde_json::from_str::<VisualReferenceReview>(&candidate.review_json)
        .map_err(|error| Error::RustError(format!("approved_review_json_invalid:{error}")))?;
    let moodboard_id = candidate
        .moodboard_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("approved_candidate_missing_moodboard_id".to_string()))?;
    let visual_reference_id = visual_reference_id_for(clone_id, &candidate.id);
    let now = now_iso_string();
    insert_visual_reference_inspiration_pool_row(
        db,
        user_id,
        clone_id,
        run_id,
        moodboard_id,
        &visual_reference_id,
        review.visual_fit_score,
        &now,
    )
    .await
}

async fn accepted_counts_by_moodboard(
    db: &D1Database,
    clone_id: &str,
) -> WorkerResult<Vec<MoodboardCountRow>> {
    db::all(
        db,
        cached_accepted_counts_by_moodboard_sql(),
        vec![json!(clone_id)],
    )
    .await
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
            ResearchPhase::Scraping
                | ResearchPhase::Reviewing
                | ResearchPhase::PoolReady
                | ResearchPhase::PartialPoolReady
                | ResearchPhase::InsufficientRefs
                | ResearchPhase::Failed
        ),
        ResearchPhase::PoolReady => matches!(next_phase, ResearchPhase::PoolReady),
        ResearchPhase::PartialPoolReady => matches!(
            next_phase,
            ResearchPhase::PartialPoolReady | ResearchPhase::PoolReady
        ),
        ResearchPhase::InsufficientRefs => matches!(
            next_phase,
            ResearchPhase::InsufficientRefs
                | ResearchPhase::PartialPoolReady
                | ResearchPhase::PoolReady
        ),
        ResearchPhase::Failed => false,
    }
}

fn research_status_allows_chunk_processing(current: Option<&str>) -> bool {
    !matches!(
        current.and_then(research_phase_from_status),
        Some(
            ResearchPhase::PoolReady
                | ResearchPhase::PartialPoolReady
                | ResearchPhase::InsufficientRefs
                | ResearchPhase::Failed
        )
    )
}

fn research_chunk_processing_allowed(
    current: ResearchStatusSnapshot<'_>,
    message_run_id: &str,
) -> bool {
    current.run_id == Some(message_run_id)
        && research_status_allows_chunk_processing(current.status)
}

fn status_write_allows_side_effects(result: ResearchStatusWriteResult) -> bool {
    result == ResearchStatusWriteResult::Written
}

fn finalize_side_effect_allowed(result: ResearchStatusWriteResult) -> bool {
    status_write_allows_side_effects(result)
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

async fn current_message_run_id(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    run_id: Option<&str>,
) -> WorkerResult<Option<String>> {
    let Some(run_id) = run_id.map(str::trim).filter(|value| !value.is_empty()) else {
        web_sys::console::log_1(
            &format!("skip tokenless niche research chunk for clone={clone_id}").into(),
        );
        return Ok(None);
    };
    let Some(state) = load_clone_research_state(db, user_id, clone_id).await? else {
        return Ok(None);
    };
    if state.run_id.as_deref() != Some(run_id) {
        web_sys::console::log_1(
            &format!(
                "skip stale niche research chunk clone={clone_id} current_run={} message_run={run_id}",
                state.run_id.as_deref().unwrap_or("")
            )
            .into(),
        );
        return Ok(None);
    }
    if !research_chunk_processing_allowed(
        ResearchStatusSnapshot {
            status: state.status.as_deref(),
            run_id: state.run_id.as_deref(),
        },
        run_id,
    ) {
        web_sys::console::log_1(
            &format!(
                "skip terminal niche research chunk clone={clone_id} status={} run={run_id}",
                state.status.as_deref().unwrap_or("")
            )
            .into(),
        );
        return Ok(None);
    }
    Ok(Some(run_id.to_string()))
}

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

fn dedupe_handle_seeds(handles: Vec<HandleSeed>) -> Vec<HandleSeed> {
    let mut seen = HashSet::new();
    handles
        .into_iter()
        .filter_map(|seed| {
            normalize_instagram_handle(&seed.handle).map(|handle| HandleSeed {
                handle,
                discovered_via: seed.discovered_via,
            })
        })
        .filter(|seed| seen.insert(seed.handle.to_ascii_lowercase()))
        .collect()
}

fn normalize_instagram_handle(handle: &str) -> Option<String> {
    let handle = handle.trim().trim_start_matches('@');
    (!handle.is_empty()
        && handle.len() <= 30
        && handle != "_"
        && !handle.starts_with('.')
        && !handle.ends_with('.')
        && !handle.contains("..")
        && handle
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_'))
    .then(|| handle.to_string())
}

fn instagram_posts_more_available(raw: &Value) -> bool {
    bool_at_any(
        raw,
        &[
            &["more_available"],
            &["moreAvailable"],
            &["has_more"],
            &["hasMore"],
            &["data", "more_available"],
            &["data", "moreAvailable"],
            &["data", "has_more"],
            &["data", "hasMore"],
            &["pagination", "more_available"],
            &["pagination", "has_more"],
            &["paging", "more_available"],
            &["paging", "has_more"],
            &["page_info", "has_next_page"],
            &["data", "page_info", "has_next_page"],
        ],
    )
}

fn instagram_posts_next_max_id(raw: &Value) -> Option<String> {
    text_at_any(
        raw,
        &[
            &["next_max_id"],
            &["nextMaxId"],
            &["data", "next_max_id"],
            &["data", "nextMaxId"],
            &["pagination", "next_max_id"],
            &["pagination", "nextMaxId"],
            &["paging", "next_max_id"],
            &["paging", "nextMaxId"],
            &["paging", "cursors", "after"],
            &["page_info", "end_cursor"],
            &["data", "page_info", "end_cursor"],
        ],
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InstagramPostDetailTarget {
    source_url: String,
}

fn instagram_post_detail_targets(
    raw: &Value,
    normalized_candidates: &[InstagramImageCandidate],
    images_per_post: usize,
    limit: usize,
) -> Vec<InstagramPostDetailTarget> {
    if images_per_post == 0 || limit == 0 {
        return Vec::new();
    }

    let mut candidate_counts_by_code = HashMap::<String, usize>::new();
    let mut candidate_counts_by_url = HashMap::<String, usize>::new();
    for candidate in normalized_candidates {
        *candidate_counts_by_code
            .entry(candidate.source_post_code.to_ascii_lowercase())
            .or_default() += 1;
        if let Some(source_url) = candidate.source_url.as_deref() {
            *candidate_counts_by_url
                .entry(source_url.to_ascii_lowercase())
                .or_default() += 1;
        }
    }

    let mut seen = HashSet::new();
    instagram_feed_items_for_detail(raw)
        .into_iter()
        .filter_map(|item| {
            let source_url = instagram_feed_item_source_url(item)?;
            if instagram_feed_item_skipped_by_detail_policy(item, &source_url) {
                return None;
            }
            if !seen.insert(source_url.to_ascii_lowercase()) {
                return None;
            }
            let candidate_count = instagram_feed_item_post_code(item)
                .and_then(|code| {
                    candidate_counts_by_code
                        .get(&code.to_ascii_lowercase())
                        .copied()
                })
                .or_else(|| {
                    candidate_counts_by_url
                        .get(&source_url.to_ascii_lowercase())
                        .copied()
                })
                .unwrap_or_default();
            let needs_detail = instagram_feed_item_needs_detail(item);
            let required_images = if needs_detail { images_per_post } else { 1 };
            (candidate_count < required_images || needs_detail)
                .then_some(InstagramPostDetailTarget { source_url })
        })
        .take(limit)
        .collect()
}

fn instagram_feed_items_for_detail(raw: &Value) -> Vec<&Value> {
    json_value_at_path(raw, &["items"])
        .or_else(|| json_value_at_path(raw, &["data", "items"]))
        .or_else(|| json_value_at_path(raw, &["data"]))
        .and_then(Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn instagram_feed_item_source_url(item: &Value) -> Option<String> {
    text_at_any(item, &[&["url"], &["permalink"]])
        .filter(|url| instagram_source_url_is_post(url))
        .or_else(|| {
            instagram_feed_item_public_shortcode(item)
                .filter(|code| instagram_shortcode_is_safe(code))
                .map(|code| format!("https://www.instagram.com/p/{code}/"))
        })
}

fn instagram_feed_item_public_shortcode(item: &Value) -> Option<String> {
    text_at_any(item, &[&["code"], &["shortcode"]])
}

fn instagram_feed_item_post_code(item: &Value) -> Option<String> {
    text_at_any(item, &[&["code"], &["shortcode"], &["id"]])
}

fn instagram_feed_item_needs_detail(item: &Value) -> bool {
    text_at_any(item, &[&["product_type"]])
        .map(|value| value.eq_ignore_ascii_case("carousel_container"))
        .unwrap_or(false)
        || matches!(json_value_at_path(item, &["media_type"]), Some(Value::Number(value)) if value.as_u64() == Some(8))
        || json_array_at_path(item, &["carousel_media"]).is_some()
        || json_array_at_path(item, &["edge_sidecar_to_children", "edges"]).is_some()
        || json_array_at_path(item, &["resources"]).is_some()
}

fn instagram_feed_item_skipped_by_detail_policy(item: &Value, source_url: &str) -> bool {
    instagram_source_url_is_reel_or_tv(source_url)
        || text_at_any(item, &[&["media_type"]])
            .map(|value| value == "2" || value.eq_ignore_ascii_case("video"))
            .unwrap_or(false)
        || text_at_any(
            item,
            &[
                &["product_type"],
                &["media_product_type"],
                &["__typename"],
                &["typename"],
                &["type"],
            ],
        )
        .map(|value| {
            let value = value.to_ascii_lowercase();
            matches!(value.as_str(), "clips" | "igtv" | "reel" | "reels") || value.contains("video")
        })
        .unwrap_or(false)
        || bool_at_any(
            item,
            &[&["is_video"], &["isVideo"], &["is_reel"], &["isReel"]],
        )
        || instagram_feed_item_has_meaningful_video_marker(item)
}

fn instagram_feed_item_has_meaningful_video_marker(item: &Value) -> bool {
    [
        &["video_versions"][..],
        &["videoVersions"][..],
        &["video"][..],
        &["video_url"][..],
        &["videoUrl"][..],
        &["video_dash_manifest"][..],
        &["videoDashManifest"][..],
        &["dash_manifest"][..],
        &["dashManifest"][..],
        &["clips_metadata"][..],
        &["clipsMetadata"][..],
    ]
    .iter()
    .any(|path| json_meaningful_value_at_path(item, path))
}

fn json_meaningful_value_at_path(value: &Value, path: &[&str]) -> bool {
    match json_value_at_path(value, path) {
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Bool(true)) => true,
        Some(Value::Number(_)) => true,
        _ => false,
    }
}

fn json_array_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    json_value_at_path(value, path)
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
}

fn instagram_source_url_is_post(url: &str) -> bool {
    let Some(rest) = url.trim().strip_prefix("https://") else {
        return false;
    };
    let without_fragment = rest.split('#').next().unwrap_or(rest);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let Some((host, path)) = without_query.split_once('/') else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    if host != "instagram.com" && host != "www.instagram.com" {
        return false;
    }
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    matches!(segments.next(), Some("p" | "reel" | "tv"))
        && segments.next().map(instagram_shortcode_is_safe) == Some(true)
        && segments.next().is_none()
}

fn instagram_source_url_is_reel_or_tv(url: &str) -> bool {
    let Some(rest) = url.trim().strip_prefix("https://") else {
        return false;
    };
    let without_fragment = rest.split('#').next().unwrap_or(rest);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let Some((host, path)) = without_query.split_once('/') else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    if host != "instagram.com" && host != "www.instagram.com" {
        return false;
    }
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    matches!(segments.next(), Some("reel" | "tv"))
        && segments.next().map(instagram_shortcode_is_safe) == Some(true)
}

fn instagram_shortcode_is_safe(code: &str) -> bool {
    !code.is_empty()
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn bool_at_any(value: &Value, paths: &[&[&str]]) -> bool {
    paths
        .iter()
        .filter_map(|path| json_value_at_path(value, path))
        .find_map(|value| match value {
            Value::Bool(value) => Some(*value),
            Value::Number(value) => Some(value.as_u64().unwrap_or_default() > 0),
            Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" => Some(true),
                "0" | "false" | "no" => Some(false),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or(false)
}

fn text_at_any(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths
        .iter()
        .find_map(|path| {
            json_value_at_path(value, path).and_then(|value| match value {
                Value::String(value) => Some(value.trim().to_string()),
                Value::Number(value) => Some(value.to_string()),
                _ => None,
            })
        })
        .filter(|value| !value.is_empty())
}

fn json_value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
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

async fn set_clone_research_status_with_run(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    status: &str,
    detail: &str,
    expected_run_id: Option<&str>,
    run_id_to_store: Option<&str>,
) -> WorkerResult<()> {
    set_clone_research_status_with_run_result(
        db,
        user_id,
        clone_id,
        status,
        detail,
        expected_run_id,
        run_id_to_store,
    )
    .await?;
    Ok(())
}

async fn set_clone_research_status_with_run_result(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    status: &str,
    detail: &str,
    expected_run_id: Option<&str>,
    run_id_to_store: Option<&str>,
) -> WorkerResult<ResearchStatusWriteResult> {
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
        QueueMessageAction::Ack => Ok(result),
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
    slug: String,
    title: String,
    vibe_summary: String,
    search_queries_json: String,
}

#[derive(Debug, Deserialize)]
struct CloneResearchRow {
    soul_status: String,
    provider_soul_id: Option<String>,
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

#[derive(Debug, Default, Deserialize)]
struct CandidateFailureCountsRow {
    cleanup_failed: u32,
    compatibility_failed: u32,
    clone_mismatch: u32,
    cache_failed: u32,
}

#[derive(Debug, Deserialize)]
struct MoodboardCountRow {
    moodboard_id: String,
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
struct DiscoverySourceStatusRow {
    status: String,
}

#[derive(Clone, Debug)]
struct HandleSeed {
    handle: String,
    discovered_via: String,
}

#[derive(Debug, Deserialize)]
struct AcceptedHandleRow {
    source_handle: String,
}

#[derive(Clone, Debug, Deserialize)]
struct VisualCandidateReviewRow {
    id: String,
    source_handle: Option<String>,
    source_caption: Option<String>,
    source_published_at: Option<String>,
    media_type: Option<u32>,
    image_url: String,
    like_count: Option<u64>,
    comment_count: Option<u64>,
    moodboard_slug: Option<String>,
    discovered_via: String,
    review_status: String,
    review_json: String,
}

impl VisualCandidateReviewRow {
    fn for_ranking(&self) -> VisualCandidateForRanking {
        VisualCandidateForRanking {
            id: self.id.clone(),
            discovered_via: self.discovered_via.clone(),
            moodboard_slug: self.moodboard_slug.clone().unwrap_or_default(),
            source_handle: self.source_handle.clone().unwrap_or_default(),
            media_type: self.media_type.unwrap_or_default().min(u32::from(u8::MAX)) as u8,
            like_count: self.like_count,
            comment_count: self.comment_count,
            source_published_at: self.source_published_at.clone(),
        }
    }

    fn review_attempts(&self) -> u32 {
        review_attempts_from_json(&self.review_json)
    }

    fn is_review_retryable(&self) -> bool {
        self.review_status == "review_retryable"
    }
}

#[derive(Debug, Deserialize)]
struct ApprovedVisualCandidateRow {
    id: String,
    source_handle: Option<String>,
    source_post_code: Option<String>,
    source_url: Option<String>,
    source_published_at: Option<String>,
    cleaned_image_url: String,
    image_width: Option<u32>,
    image_height: Option<u32>,
    moodboard_id: Option<String>,
    moodboard_slug: Option<String>,
    review_json: String,
    compatibility_json: String,
}

#[derive(Clone, Debug, Deserialize)]
struct CleanupCandidateRow {
    id: String,
    image_url: String,
    cleanup_json: String,
}

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

#[derive(Clone, Debug)]
struct SeedreamCleanupRequest {
    access_token: String,
    reference_value: String,
    tool_name: String,
    model: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn function_body<'a>(source: &'a str, marker: &str) -> &'a str {
        let start = source.find(marker).expect("function marker");
        let rest = &source[start..];
        let open = rest.find('{').expect("function body") + 1;
        let mut depth = 1usize;
        for (offset, ch) in rest[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &rest[..open + offset + 1];
                    }
                }
                _ => {}
            }
        }
        panic!("unterminated function body for {marker}");
    }

    fn assert_no_old_text_research_or_reels_search(body: &str) {
        for forbidden in [
            "seed_extraction_prompt(",
            "knowledge_extraction_prompt(",
            "clustering_prompt(",
            "build_scrape_request(",
            "InstagramReels",
            "/v2/instagram/reels/search",
        ] {
            assert!(!body.contains(forbidden), "{forbidden} should not be used");
        }
    }

    #[test]
    fn onboarding_research_kickoff_uses_visual_reference_pipeline_contract() {
        let source = include_str!("niche_research.rs");
        let handle_message = function_body(source, "async fn handle_message");
        let research_kickoff = function_body(source, "async fn research_moodboard_references");
        let enqueue_research =
            function_body(source, "async fn enqueue_moodboard_reference_research");

        assert!(handle_message.contains("NicheResearchMessage::RefreshPool"));
        assert!(handle_message.contains("research_moodboard_references("));
        assert!(research_kickoff.contains("start_visual_reference_research_run("));
        assert!(research_kickoff.contains("enqueue_moodboard_reference_research("));
        assert!(enqueue_research.contains("NicheResearchMessage::DiscoverInstagramHandles"));
        assert!(enqueue_research.contains("selected_search_terms("));
        assert!(!enqueue_research.contains("moodboard_handle_map("));
        assert!(!enqueue_research.contains("configured_handle"));

        assert_no_old_text_research_or_reels_search(handle_message);
        assert_no_old_text_research_or_reels_search(research_kickoff);
        assert_no_old_text_research_or_reels_search(enqueue_research);
    }

    #[test]
    fn search_term_selection_is_trimmed_deduped_and_bounded() {
        let terms = selected_search_terms(
            r#"[" flash fashion ", "Flash Fashion", "", "street creator"]"#,
            "flash-editorial",
            "Flash Editorial",
            2,
        );

        assert_eq!(
            terms,
            vec!["flash fashion".to_string(), "street creator".to_string()]
        );
    }

    #[test]
    fn handle_discovery_does_not_recap_combined_reels_and_learned_handles() {
        let source = include_str!("niche_research.rs");
        let discover = function_body(source, "async fn discover_instagram_handles_message");

        assert!(
            discover.contains("extract_instagram_reels_owner_handles(&raw, max_handles as usize)")
        );
        assert!(discover.contains("load_accepted_handles(db, clone_id, moodboard_id, max_handles)"));
        assert!(!discover.contains(".take(max_handles as usize)"));
    }

    #[test]
    fn kickoff_reserves_reels_search_source_before_queue_send() {
        let source = include_str!("niche_research.rs");
        let enqueue_research =
            function_body(source, "async fn enqueue_moodboard_reference_research");

        let reserve_pos = enqueue_research
            .find("reserve_instagram_reels_search_source(")
            .expect("kickoff should reserve reels search discovery source");
        let send_pos = enqueue_research
            .find(".send(NicheResearchMessage::DiscoverInstagramHandles")
            .expect("kickoff should send reels discovery message");

        assert!(reserve_pos < send_pos);
        assert!(enqueue_research.contains("if !reserved_search {"));
    }

    #[test]
    fn handle_discovery_rechecks_run_after_reels_fetch_before_side_effects() {
        let source = include_str!("niche_research.rs");
        let discover = function_body(source, "async fn discover_instagram_handles_message");

        let fetch_pos = discover
            .find("fetch_scrapecreators_json(&request_url, &api_key)")
            .expect("reels handler should fetch scrapecreators json");
        let stale_guard_pos = discover[fetch_pos..]
            .find("current_message_run_id(db, user_id, clone_id, Some(&run_id))")
            .map(|offset| fetch_pos + offset)
            .expect("reels handler should recheck current run after fetch");
        let mark_fresh_pos = discover
            .find("mark_discovery_source_fresh(")
            .expect("reels handler should mark source fresh");
        let reserve_profile_pos = discover
            .find("reserve_instagram_profile_source(")
            .expect("reels handler should reserve profile sources");

        assert!(fetch_pos < stale_guard_pos);
        assert!(stale_guard_pos < mark_fresh_pos);
        assert!(stale_guard_pos < reserve_profile_pos);
    }

    #[test]
    fn handle_discovery_marks_reels_source_fresh_after_profile_reservations() {
        let source = include_str!("niche_research.rs");
        let discover = function_body(source, "async fn discover_instagram_handles_message");

        let reserve_profile_pos = discover
            .find("reserve_instagram_profile_source(")
            .expect("reels handler should reserve profile sources");
        let mark_fresh_pos = discover
            .find("mark_discovery_source_fresh(")
            .expect("reels handler should mark source fresh");

        assert!(reserve_profile_pos < mark_fresh_pos);
    }

    #[test]
    fn posts_message_applies_min_dimension_gate_before_insert() {
        let source = include_str!("niche_research.rs");
        let body = function_body(source, "async fn fetch_instagram_posts_message");

        assert!(body.contains("instagram_min_image_width"));
        assert!(body.contains("instagram_min_image_height"));
        assert!(body.contains("instagram_candidate_meets_min_dimensions"));
    }

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

    #[test]
    fn kickoff_failure_context_uses_created_run_id_for_failure_recording() {
        let message = NicheResearchMessage::ResearchMoodboardReferences {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            moodboard_ids: vec!["moodboard_1".to_string()],
            reason: "onboarding_selection".to_string(),
        };
        let run_id = kickoff_message_run_id(&message).expect("kickoff run id");
        let context = message_failure_context(&message, Some(&run_id));

        assert_eq!(context.run_id.as_deref(), Some(run_id.as_str()));
        assert_eq!(
            research_status_write_decision(
                Some(ResearchStatusSnapshot {
                    status: Some("queued"),
                    run_id: Some(&run_id),
                }),
                "research_failed",
                ResearchStatusWriteMode::Failure,
                context.run_id.as_deref(),
            ),
            ResearchStatusWriteDecision::Write
        );

        let refresh = NicheResearchMessage::RefreshPool {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            reason: "pool_depleted".to_string(),
        };
        let refresh_run_id = kickoff_message_run_id(&refresh).expect("refresh run id");
        let refresh_context = message_failure_context(&refresh, Some(&refresh_run_id));

        assert_eq!(
            refresh_context.run_id.as_deref(),
            Some(refresh_run_id.as_str())
        );
    }

    #[test]
    fn tokenless_non_kickoff_failure_still_skips_active_tokened_run() {
        let message = NicheResearchMessage::ReviewVisualCandidates {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            run_id: None,
            limit: 10,
        };
        let context = message_failure_context(&message, None);

        assert!(context.run_id.is_none());
        assert_eq!(
            research_status_write_decision(
                Some(ResearchStatusSnapshot {
                    status: Some("reviewing"),
                    run_id: Some("run_active"),
                }),
                "research_failed",
                ResearchStatusWriteMode::Failure,
                context.run_id.as_deref(),
            ),
            ResearchStatusWriteDecision::SkipStale
        );
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
    fn post_detail_message_serializes_run_id_and_source_url() {
        let message = NicheResearchMessage::FetchInstagramPostDetail {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            run_id: Some("run_1".to_string()),
            moodboard_id: "moodboard_1".to_string(),
            moodboard_slug: "flash-editorial".to_string(),
            handle: "creator".to_string(),
            discovered_via: "configured_handle".to_string(),
            source_url: "https://www.instagram.com/p/ABC123/".to_string(),
        };

        assert_eq!(
            serde_json::to_value(&message).unwrap(),
            json!({
                "type": "fetch_instagram_post_detail",
                "userId": "user_1",
                "cloneId": "clone_1",
                "runId": "run_1",
                "moodboardId": "moodboard_1",
                "moodboardSlug": "flash-editorial",
                "handle": "creator",
                "discoveredVia": "configured_handle",
                "sourceUrl": "https://www.instagram.com/p/ABC123/"
            })
        );

        let parsed: NicheResearchMessage = serde_json::from_value(json!({
            "type": "fetch_instagram_post_detail",
            "userId": "user_1",
            "cloneId": "clone_1",
            "runId": "run_1",
            "moodboardId": "moodboard_1",
            "moodboardSlug": "flash-editorial",
            "handle": "creator",
            "discoveredVia": "configured_handle",
            "sourceUrl": "https://www.instagram.com/p/ABC123/"
        }))
        .unwrap();
        assert!(matches!(
            parsed,
            NicheResearchMessage::FetchInstagramPostDetail {
                run_id,
                source_url,
                ..
            } if run_id.as_deref() == Some("run_1")
                && source_url == "https://www.instagram.com/p/ABC123/"
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
    fn visual_candidate_insert_sql_preserves_caption_but_reference_insert_removes_it() {
        assert!(insert_visual_candidate_sql().contains("source_caption"));
        assert!(insert_visual_candidate_sql().contains("review_json"));
        assert!(insert_visual_reference_sql().contains("source_caption_removed"));
        assert!(!insert_visual_reference_sql().contains("source_caption,"));
    }

    #[test]
    fn visual_candidate_conflict_update_preserves_terminal_run_metadata() {
        let sql = insert_visual_candidate_sql();

        assert!(sql.contains("metadata_json = CASE"));
        assert!(sql.contains(
            "visual_reference_candidates.review_status IN ('unreviewed', 'review_retryable')"
        ));
        assert!(sql.contains("THEN excluded.metadata_json"));
        assert!(sql.contains("ELSE visual_reference_candidates.metadata_json"));
    }

    #[test]
    fn accepted_handle_sql_scopes_by_clone_and_moodboard() {
        let sql = accepted_handles_sql();

        assert!(sql.contains("WHERE vr.clone_id = ?"));
        assert!(sql.contains("AND vr.moodboard_id = ?"));
        assert!(sql.contains("source_handle IS NOT NULL"));
    }

    #[test]
    fn cached_reference_count_sql_requires_media_asset_storage() {
        let sql = cached_accepted_counts_by_moodboard_sql();

        assert!(sql.contains("INNER JOIN media_assets ma"));
        assert!(sql.contains("ma.id = vr.media_asset_id"));
        assert!(sql.contains("vr.media_asset_id IS NOT NULL"));
        assert!(sql.contains("ma.storage_key IS NOT NULL"));
        assert!(sql.contains("TRIM(ma.storage_key) <> ''"));
        assert!(sql.contains("ma.deleted_at IS NULL"));
        assert!(sql.contains("INNER JOIN user_inspiration_pool uip"));
        assert!(sql.contains("uip.visual_reference_id = vr.id"));
    }

    #[test]
    fn cache_success_insert_sql_is_active_run_status_guarded() {
        let reference_sql = insert_visual_reference_sql();
        let pool_sql = insert_visual_reference_inspiration_pool_sql();

        for sql in [reference_sql, pool_sql] {
            assert!(sql.contains("FROM clone_profiles cp"));
            assert!(sql.contains("cp.user_id = ?"));
            assert!(sql.contains("cp.id = ?"));
            assert!(sql.contains("$.nicheResearchRunId"));
            assert!(sql.contains("$.nicheResearchStatus"));
            assert!(sql.contains("IN ('queued', 'scraping', 'reviewing')"));
            assert!(!sql.contains("research_failed"));
            assert!(!sql.contains("pool_ready"));
            assert!(!sql.contains("partial_pool_ready"));
            assert!(!sql.contains("insufficient_refs"));
        }
        assert!(pool_sql.contains("FROM visual_references vr"));
        assert!(pool_sql.contains("INNER JOIN media_assets ma"));
        assert!(pool_sql.contains("ma.storage_key IS NOT NULL"));
        assert!(pool_sql.contains("vr.status = 'active'"));
        assert!(pool_sql.contains("vr.media_asset_id IS NOT NULL"));
    }

    #[test]
    fn cache_repair_pool_sql_upserts_missing_pool_for_cached_reference() {
        let sql = insert_visual_reference_inspiration_pool_sql();

        assert!(sql.contains("INSERT OR IGNORE INTO user_inspiration_pool"));
        assert!(sql.contains("WHERE EXISTS"));
        assert!(sql.contains("FROM visual_references vr"));
        assert!(sql.contains("INNER JOIN media_assets ma"));
        assert!(sql.contains("ma.storage_key IS NOT NULL"));
        assert!(sql.contains("vr.id = ?"));
        assert!(sql.contains("vr.status = 'active'"));
        assert!(sql.contains("vr.media_asset_id IS NOT NULL"));
    }

    #[test]
    fn profile_reservation_sql_guards_run_profile_cap() {
        let sql = reserve_instagram_profile_source_sql();

        assert!(sql.contains("INSERT OR IGNORE INTO discovery_sources"));
        assert!(sql.contains("SELECT COUNT(*)"));
        assert!(sql.contains("$.runId"));
        assert!(sql.contains("$.requestType"));
        assert!(sql.contains("= 'instagram_profile'"));
        assert!(sql.contains(") < ?"));
    }

    #[test]
    fn approved_candidate_guard_sql_enforces_run_and_handle_caps() {
        let sql = approve_visual_candidate_with_cap_guards_sql();

        assert!(sql.contains("metadata_json = json_set"));
        assert!(sql.contains("$.approvedRunId"));
        assert!(sql.contains(
            "CAST(json_extract(visual_reference_candidates.metadata_json, '$.runId') AS TEXT) = ?"
        ));
        assert!(sql.contains("review_status = 'reviewing'"));
        assert!(sql.contains("approved.review_status IN ('cleanup_pending', 'cleanup_retryable', 'compatibility_pending', 'compatibility_retryable', 'cache_pending', 'caching', 'cached')"));
        assert!(sql.contains("$.runId"));
        assert!(sql.contains(") < ?"));
        assert!(sql.contains(
            "lower(approved_handle.source_handle) = lower(visual_reference_candidates.source_handle)"
        ));
        assert!(sql.contains("INNER JOIN media_assets ma"));
        assert!(sql.contains("vr.status = 'active'"));
    }

    #[test]
    fn approved_candidate_count_sql_requires_current_approval_run_marker() {
        let run_sql = approved_candidate_count_for_run_sql();
        let handle_sql = approved_candidate_count_for_run_and_handle_sql();

        assert!(run_sql.contains("CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(run_sql.contains("review_status IN ('cleanup_pending', 'cleanup_retryable', 'compatibility_pending', 'compatibility_retryable', 'cache_pending', 'caching', 'cached')"));
        assert!(
            run_sql.contains("CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?")
        );
        assert!(handle_sql.contains("CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(handle_sql.contains("review_status IN ('cleanup_pending', 'cleanup_retryable', 'compatibility_pending', 'compatibility_retryable', 'cache_pending', 'caching', 'cached')"));
        assert!(
            handle_sql.contains("CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?")
        );
        assert!(handle_sql.contains("lower(source_handle) = lower(?)"));
    }

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
        assert!(mark_candidate_cleanup_succeeded_sql()
            .contains("review_status = 'compatibility_pending'"));
        assert!(mark_candidate_cleanup_failed_sql().contains("THEN 'cleanup_retryable'"));
        assert!(mark_candidate_cleanup_failed_sql().contains("ELSE 'cleanup_failed'"));
    }

    #[test]
    fn cleanup_claim_sql_uses_reclaimable_in_progress_json_lease() {
        let sql = claim_visual_candidate_for_cleanup_sql();

        assert!(sql.contains("'$.claimStatus'"));
        assert!(sql.contains("'cleanup_in_progress'"));
        assert!(sql.contains("$.claimStartedAt"));
        assert!(sql.contains("<> 'cleanup_in_progress'"));
        assert!(sql.contains("reviewed_at <= strftime"));
    }

    #[test]
    fn cleanup_completion_sql_guards_claimed_attempt() {
        for sql in [
            mark_candidate_cleanup_succeeded_sql(),
            mark_candidate_cleanup_failed_sql(),
        ] {
            let where_clause = sql.split("WHERE id = ?").nth(1).expect("where clause");

            assert!(where_clause.contains("cleanup_json"));
            assert!(where_clause.contains("$.claimStatus"));
            assert!(where_clause.contains("'cleanup_in_progress'"));
            assert!(where_clause.contains("$.attempts"));
            assert!(where_clause.contains(") = ?"));
        }
    }

    #[test]
    fn cleanup_message_gates_followup_side_effects_on_mark_result() {
        let source = include_str!("niche_research.rs");
        let body = function_body(source, "async fn cleanup_approved_reference_message");

        let success_mark = body
            .find("let marked = mark_candidate_cleanup_succeeded(")
            .expect("success mark result should be captured");
        let compatibility_enqueue = body
            .find("NicheResearchMessage::ValidateCloneCompatibility")
            .expect("compatibility enqueue");
        assert!(success_mark < compatibility_enqueue);
        assert!(body[success_mark..compatibility_enqueue].contains("if !marked {"));

        let failure_mark = body
            .find("let marked = mark_candidate_cleanup_failed(")
            .expect("failure mark result should be captured");
        let review_enqueue = body
            .find("NicheResearchMessage::ReviewVisualCandidates")
            .expect("review enqueue");
        assert!(failure_mark < review_enqueue);
        assert!(body[failure_mark..review_enqueue].contains("if !marked {"));
    }

    #[test]
    fn cleanup_message_rechecks_current_run_after_upload_before_seedream_call() {
        let source = include_str!("niche_research.rs");
        let body = function_body(source, "async fn cleanup_approved_reference_message");

        let prepare_pos = body
            .find("prepare_seedream_cleanup(")
            .expect("cleanup upload prepare helper");
        let seedream_call_pos = body
            .find("call_seedream_cleanup(")
            .expect("seedream cleanup call helper");
        let stale_guard_pos = body[prepare_pos..seedream_call_pos]
            .find("current_message_run_id(db, user_id, clone_id, Some(&run_id))")
            .map(|offset| prepare_pos + offset)
            .expect("run guard between upload and seedream call");

        assert!(prepare_pos < stale_guard_pos);
        assert!(stale_guard_pos < seedream_call_pos);
    }

    #[test]
    fn compatibility_sql_uses_single_review_status_lifecycle() {
        assert!(claim_visual_candidate_for_compatibility_sql()
            .contains("review_status IN ('compatibility_pending', 'compatibility_retryable')"));
        assert!(mark_candidate_compatibility_succeeded_sql()
            .contains("review_status = 'cache_pending'"));
        assert!(mark_candidate_clone_mismatch_sql().contains("review_status = 'clone_mismatch'"));
        assert!(
            mark_candidate_compatibility_failed_sql().contains("THEN 'compatibility_retryable'")
        );
        assert!(mark_candidate_compatibility_failed_sql().contains("ELSE 'compatibility_failed'"));
    }

    #[test]
    fn compatibility_claim_sql_uses_in_progress_json_lease() {
        let sql = claim_visual_candidate_for_compatibility_sql();

        assert!(sql.contains("'$.claimStatus'"));
        assert!(sql.contains("'compatibility_in_progress'"));
        assert!(sql.contains("$.claimStartedAt"));
        assert!(sql.contains("<> 'compatibility_in_progress'"));
        assert!(sql.contains("reviewed_at <= strftime"));
    }

    #[test]
    fn compatibility_completion_sql_guards_claimed_attempt() {
        for sql in [
            mark_candidate_compatibility_succeeded_sql(),
            mark_candidate_clone_mismatch_sql(),
            mark_candidate_compatibility_failed_sql(),
        ] {
            let where_clause = sql.split("WHERE id = ?").nth(1).expect("where clause");

            assert!(where_clause.contains("compatibility_json"));
            assert!(where_clause.contains("$.attempts"));
            assert!(where_clause.contains(") = ?"));
        }
    }

    #[test]
    fn compatibility_success_and_mismatch_preserve_attempt_audit_metadata() {
        for sql in [
            mark_candidate_compatibility_succeeded_sql(),
            mark_candidate_clone_mismatch_sql(),
        ] {
            assert!(sql.contains(
                "compatibility_json = json_set(\n              CASE WHEN json_valid(compatibility_json) THEN compatibility_json ELSE '{}' END"
            ));
            assert!(sql.contains("'$.review'"));
            assert!(sql.contains("$.attempts"));

            let where_clause = sql.split("WHERE id = ?").nth(1).expect("where clause");
            assert!(where_clause.contains("compatibility_json"));
            assert!(where_clause.contains("$.attempts"));
            assert!(where_clause.contains(") = ?"));
        }
    }

    #[test]
    fn compatibility_completion_sql_moves_claim_out_of_in_progress() {
        assert!(mark_candidate_compatibility_succeeded_sql().contains("'compatibility_completed'"));
        assert!(mark_candidate_clone_mismatch_sql().contains("'compatibility_mismatched'"));
        assert!(mark_candidate_compatibility_failed_sql().contains("'compatibility_failed'"));
    }

    #[test]
    fn compatibility_message_gates_followup_side_effects_on_mark_result() {
        let source = include_str!("niche_research.rs");
        let body = function_body(source, "async fn validate_clone_compatibility_message");

        let success_mark = body
            .find("let marked = mark_candidate_compatibility_succeeded(")
            .expect("success mark result should be captured");
        let cache_enqueue = body
            .find("NicheResearchMessage::CacheApprovedReference")
            .expect("cache enqueue");
        assert!(success_mark < cache_enqueue);
        assert!(body[success_mark..cache_enqueue].contains("if !marked {"));

        let mismatch_mark = body
            .find("let marked = mark_candidate_clone_mismatch(")
            .expect("mismatch mark result should be captured");
        let review_enqueue = body
            .find("NicheResearchMessage::ReviewVisualCandidates")
            .expect("review enqueue");
        assert!(mismatch_mark < review_enqueue);
        assert!(body[mismatch_mark..review_enqueue].contains("if !marked {"));

        let failure_mark = body
            .find("let marked = mark_candidate_compatibility_failed(")
            .expect("failure mark result should be captured");
        let failure_review_enqueue = body[failure_mark..]
            .find("NicheResearchMessage::ReviewVisualCandidates")
            .map(|offset| failure_mark + offset)
            .expect("failure review enqueue");
        assert!(failure_mark < failure_review_enqueue);
        assert!(body[failure_mark..failure_review_enqueue].contains("if !marked {"));
    }

    #[test]
    fn compatibility_message_rechecks_current_run_after_reference_load_before_ai_call() {
        let source = include_str!("niche_research.rs");
        let body = function_body(source, "async fn validate_clone_compatibility_message");

        let reference_load_pos = body
            .find("load_clone_reference_image_urls(")
            .expect("reference load helper");
        let ai_call_pos = body
            .find("run_multi_vision_json::<CloneCompatibilityReview>")
            .expect("compatibility ai call");
        let stale_guard_pos = body[reference_load_pos..ai_call_pos]
            .find("current_message_run_id(db, user_id, clone_id, Some(&run_id))")
            .map(|offset| reference_load_pos + offset)
            .expect("run guard between reference load and ai call");

        assert!(reference_load_pos < stale_guard_pos);
        assert!(stale_guard_pos < ai_call_pos);
    }

    #[test]
    fn cache_load_requires_cleaned_compatible_candidate() {
        let sql = load_approved_candidate_for_cache_sql();

        assert!(sql.contains("review_status IN ('cache_pending', 'caching')"));
        assert!(sql.contains("cleaned_image_url"));
        assert!(sql.contains("compatibility_json"));
        assert!(sql.contains("json_valid(compatibility_json)"));
        assert!(sql.contains("json_extract(compatibility_json, '$.claimStatus')"));
        assert!(sql.contains("'compatibility_completed'"));
        assert!(sql.contains("json_extract(compatibility_json, '$.review.compatible')"));
        assert!(!sql.contains("review_status IN ('approved', 'caching')"));
    }

    #[test]
    fn review_claim_sql_is_run_status_and_attempt_guarded() {
        let sql = claim_visual_candidate_for_review_sql();

        assert!(sql.contains("SET review_status = 'reviewing'"));
        assert!(sql.contains("CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(sql.contains("review_status IN ('unreviewed', 'review_retryable')"));
        assert!(sql.contains("review_status = 'reviewing'"));
        assert!(sql.contains("strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')"));
        assert!(sql.contains(") AS INTEGER), 0) = ?"));
        assert!(sql.contains(") AS INTEGER), 0) < ?"));
        assert!(sql.contains("$.claimStartedAt"));
    }

    #[test]
    fn failed_review_write_sql_is_run_scoped_and_status_guarded() {
        let sql = mark_candidate_review_failed_sql();

        assert!(sql.contains("CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(sql.contains("review_status = 'reviewing'"));
        assert!(sql.contains(") AS INTEGER), 0) = ?"));
        assert!(sql.contains("THEN 'review_retryable'"));
        assert!(sql.contains("ELSE 'review_failed'"));
    }

    #[test]
    fn rejected_review_write_sql_is_run_scoped_and_status_guarded() {
        let sql = mark_candidate_rejected_with_review_sql();

        assert!(sql.contains("review_status = 'rejected'"));
        assert!(sql.contains("CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(sql.contains("review_status = 'reviewing'"));
    }

    #[test]
    fn cache_claim_sql_is_run_status_and_cached_reference_guarded() {
        let sql = claim_visual_candidate_for_cache_sql();

        assert!(sql.contains("SET review_status = 'caching'"));
        assert!(sql.contains("review_status = 'cache_pending'"));
        assert!(sql.contains("review_status = 'caching'"));
        assert!(sql.contains("strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')"));
        assert!(sql.contains("CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(sql.contains("CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?"));
        assert!(sql.contains("$.cacheRunId"));
        assert!(sql.contains("AND NOT EXISTS"));
        assert!(sql.contains("INNER JOIN media_assets ma"));
        assert!(sql.contains("ma.storage_key IS NOT NULL"));
        assert!(sql.contains("vr.candidate_id = ?"));
        assert!(sql.contains("vr.status = 'active'"));
    }

    #[test]
    fn cache_failure_sql_is_run_scoped_status_guarded_and_cached_reference_safe() {
        let sql = mark_candidate_cache_failed_sql();

        assert!(sql.contains("review_status = 'cache_failed'"));
        assert!(sql.contains("AND clone_id = ?"));
        assert!(sql.contains("AND review_status = 'caching'"));
        assert!(sql.contains("CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(sql.contains("CAST(json_extract(metadata_json, '$.approvedRunId') AS TEXT) = ?"));
        assert!(sql.contains("CAST(json_extract(metadata_json, '$.cacheRunId') AS TEXT) = ?"));
        assert!(sql.contains("AND NOT EXISTS"));
        assert!(sql.contains("INNER JOIN media_assets ma"));
        assert!(sql.contains("ma.storage_key IS NOT NULL"));
        assert!(sql.contains("vr.candidate_id = ?"));
        assert!(sql.contains("vr.status = 'active'"));
        assert!(sql.contains("vr.media_asset_id IS NOT NULL"));
    }

    #[test]
    fn retryable_review_load_sql_is_bounded_by_attempt_metadata() {
        let sql = load_visual_candidates_for_review_sql();

        assert!(sql.contains("CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(sql.contains("review_status = 'unreviewed'"));
        assert!(sql.contains("review_status = 'review_retryable'"));
        assert!(sql.contains("review_status = 'reviewing'"));
        assert!(sql.contains("strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')"));
        assert!(sql.contains("$.attempts"));
        assert!(sql.contains(") < ?"));
        assert!(sql.contains("reviewed_at ASC"));
    }

    #[test]
    fn remaining_retryable_review_sql_checks_global_budget_without_load_window() {
        let sql = remaining_retryable_visual_candidates_sql();

        assert!(sql.contains("SELECT COUNT(*) AS count"));
        assert!(sql.contains("CAST(json_extract(metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(sql.contains("review_status = 'review_retryable'"));
        assert!(!sql.contains("review_status IN ('unreviewed', 'review_retryable')"));
        assert!(sql.contains("$.attempts"));
        assert!(sql.contains(") < ?"));
        assert!(!sql.contains("ORDER BY"));
        assert!(!sql.contains("LIMIT"));
    }

    #[test]
    fn finalize_discovery_drain_sql_detects_inflight_and_unstarted_posts() {
        let sql = finalize_pending_discovery_work_sql();

        assert!(sql.contains("ds.status = 'refreshing'"));
        assert!(sql.contains(
            "'instagram_reels_search', 'instagram_profile', 'instagram_user_posts', 'instagram_post_detail'"
        ));
        assert!(sql.contains("profile.status = 'fresh'"));
        assert!(sql.contains("NOT EXISTS"));
        assert!(sql.contains("= 'instagram_user_posts'"));
        assert!(sql.contains("$.runId"));
        assert!(sql.contains("$.cloneId"));
        assert!(sql.contains("$.handle"));
        assert!(sql.contains("$.moodboardId"));
    }

    #[test]
    fn finalize_visual_drain_sql_detects_reviewable_and_cacheable_work() {
        let sql = finalize_pending_visual_work_sql();

        assert!(sql.contains("vc.review_status = 'unreviewed'"));
        assert!(sql.contains("vc.review_status = 'review_retryable'"));
        assert!(sql.contains("vc.review_status = 'reviewing'"));
        assert!(sql.contains("vc.review_status = 'cleanup_pending'"));
        assert!(sql.contains("vc.review_status = 'cleanup_retryable'"));
        assert!(sql.contains("vc.review_status = 'compatibility_pending'"));
        assert!(sql.contains("vc.review_status = 'compatibility_retryable'"));
        assert!(sql.contains("vc.review_status = 'cache_pending'"));
        assert!(sql.contains("vc.review_status = 'caching'"));
        assert!(sql.contains("$.attempts"));
        assert!(sql.contains("$.approvedRunId"));
        assert!(sql.contains("NOT EXISTS"));
        assert!(sql.contains("INNER JOIN media_assets ma"));
        assert!(sql.contains("INNER JOIN user_inspiration_pool uip"));
        assert!(sql.contains("vr.candidate_id = vc.id"));
        assert!(sql.contains("ma.storage_key IS NOT NULL"));
    }

    #[test]
    fn finalize_visual_drain_sql_counts_only_actionable_cleanup_and_compatibility_work() {
        let sql = finalize_pending_visual_work_sql();

        let cleanup_pending = sql
            .find("vc.review_status = 'cleanup_pending'")
            .expect("cleanup pending branch");
        let cleanup_retryable = sql
            .find("vc.review_status = 'cleanup_retryable'")
            .expect("cleanup retryable branch");
        let compatibility_pending = sql
            .find("vc.review_status = 'compatibility_pending'")
            .expect("compatibility pending branch");
        let compatibility_retryable = sql
            .find("vc.review_status = 'compatibility_retryable'")
            .expect("compatibility retryable branch");
        let cache_pending = sql
            .find("vc.review_status = 'cache_pending'")
            .expect("cache pending branch");

        for branch in [
            &sql[cleanup_pending..cleanup_retryable],
            &sql[cleanup_retryable..compatibility_pending],
        ] {
            assert!(branch.contains("vc.cleanup_json"));
            assert!(branch.contains("$.attempts"));
            assert!(branch.contains(") < ?"));
            assert!(branch.contains("$.claimStartedAt"));
            assert!(branch.contains("vc.reviewed_at <= strftime"));
        }

        for branch in [
            &sql[compatibility_pending..compatibility_retryable],
            &sql[compatibility_retryable..cache_pending],
        ] {
            assert!(branch.contains("vc.compatibility_json"));
            assert!(branch.contains("$.attempts"));
            assert!(branch.contains(") < ?"));
            assert!(branch.contains("$.claimStartedAt"));
            assert!(branch.contains("vc.reviewed_at <= strftime"));
        }
    }

    #[test]
    fn final_status_detail_counts_cleanup_and_compatibility_failures() {
        let source = include_str!("niche_research.rs");
        let body = function_body(source, "async fn finalize_reference_pool_message");

        assert!(body.contains("cleanup_failed"));
        assert!(body.contains("compatibility_failed"));
        assert!(body.contains("clone_mismatch"));
        assert!(body.contains("cache_failed"));
    }

    #[test]
    fn finalize_in_progress_visual_work_sql_detects_reviewing_and_caching_claims() {
        let sql = finalize_in_progress_visual_work_sql();

        assert!(sql.contains("vc.review_status IN ('reviewing', 'caching')"));
        assert!(sql.contains("vc.review_status IN ('cleanup_pending', 'cleanup_retryable')"));
        assert!(sql
            .contains("vc.review_status IN ('compatibility_pending', 'compatibility_retryable')"));
        assert!(sql.contains("vc.cleanup_json"));
        assert!(sql.contains("vc.compatibility_json"));
        assert!(sql.contains("$.claimStartedAt"));
        assert!(sql.contains("vc.reviewed_at > strftime"));
        assert!(sql.contains("NOT EXISTS"));
        assert!(sql.contains("INNER JOIN media_assets ma"));
        assert!(sql.contains("INNER JOIN user_inspiration_pool uip"));
        assert!(sql.contains("vr.candidate_id = vc.id"));
    }

    #[test]
    fn finalize_approved_uncached_sql_loads_cacheable_candidates_for_run() {
        let sql = finalize_approved_uncached_candidates_sql();

        assert!(sql.contains("SELECT vc.id"));
        assert!(sql.contains("vc.review_status = 'cache_pending'"));
        assert!(sql.contains("vc.review_status = 'caching'"));
        assert!(sql.contains("strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')"));
        assert!(sql.contains("CAST(json_extract(vc.metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(sql.contains("CAST(json_extract(vc.metadata_json, '$.approvedRunId') AS TEXT) = ?"));
        assert!(sql.contains("NOT EXISTS"));
        assert!(sql.contains("INNER JOIN user_inspiration_pool uip"));
        assert!(sql.contains("LIMIT ?"));
    }

    #[test]
    fn finalize_cleanup_candidates_sql_loads_retryable_cleanup_for_run() {
        let sql = finalize_cleanup_candidates_sql();

        assert!(sql.contains("SELECT vc.id"));
        assert!(sql.contains("vc.review_status IN ('cleanup_pending', 'cleanup_retryable')"));
        assert!(sql.contains("CAST(json_extract(vc.metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(sql.contains("CAST(json_extract(vc.metadata_json, '$.approvedRunId') AS TEXT) = ?"));
        assert!(sql.contains("vc.cleanup_json"));
        assert!(sql.contains("$.attempts"));
        assert!(sql.contains(") < ?"));
        assert!(sql.contains("vc.reviewed_at <= strftime"));
        assert!(sql.contains("LIMIT ?"));
    }

    #[test]
    fn finalize_compatibility_candidates_sql_loads_retryable_compatibility_for_run() {
        let sql = finalize_compatibility_candidates_sql();

        assert!(sql.contains("SELECT vc.id"));
        assert!(sql
            .contains("vc.review_status IN ('compatibility_pending', 'compatibility_retryable')"));
        assert!(sql.contains("CAST(json_extract(vc.metadata_json, '$.runId') AS TEXT) = ?"));
        assert!(sql.contains("CAST(json_extract(vc.metadata_json, '$.approvedRunId') AS TEXT) = ?"));
        assert!(sql.contains("vc.compatibility_json"));
        assert!(sql.contains("$.attempts"));
        assert!(sql.contains("$.claimStartedAt"));
        assert!(sql.contains("vc.reviewed_at <= strftime"));
        assert!(sql.contains("LIMIT ?"));
    }

    #[test]
    fn finalize_drain_detects_cached_references_missing_pool_rows() {
        for sql in [
            finalize_pending_visual_work_sql(),
            finalize_in_progress_visual_work_sql(),
            finalize_approved_uncached_candidates_sql(),
        ] {
            assert!(sql.contains("INNER JOIN media_assets ma"));
            assert!(sql.contains("ma.storage_key IS NOT NULL"));
            assert!(sql.contains("INNER JOIN user_inspiration_pool uip"));
            assert!(sql.contains("uip.clone_id = vr.clone_id"));
            assert!(sql.contains("uip.visual_reference_id = vr.id"));
            assert!(sql.contains("NOT EXISTS"));
        }
    }

    #[test]
    fn review_completion_action_retries_before_finalizing() {
        assert_eq!(
            review_completion_action(0, true),
            ReviewCompletionAction::EnqueueRetry
        );
        assert_eq!(
            review_completion_action(0, false),
            ReviewCompletionAction::Finalize
        );
        assert_eq!(
            review_completion_action(1, true),
            ReviewCompletionAction::EnqueueRetry
        );
        assert_eq!(
            review_completion_action(1, false),
            ReviewCompletionAction::WaitForCache
        );
        assert!(review_completion_schedules_finalize_nudge(
            ReviewCompletionAction::WaitForCache
        ));
        assert!(!review_completion_schedules_finalize_nudge(
            ReviewCompletionAction::Finalize
        ));
    }

    #[test]
    fn cache_claim_noop_action_schedules_finalize() {
        assert_eq!(cache_claim_action(true), CacheClaimAction::Cache);
        assert_eq!(
            cache_claim_action(false),
            CacheClaimAction::EnqueueDelayedFinalize
        );
    }

    #[test]
    fn child_discovery_reservation_action_sends_existing_refreshing_same_run() {
        assert_eq!(
            discovery_reservation_action(true, None),
            DiscoveryReservationAction::Send
        );
        assert_eq!(
            discovery_reservation_action(false, Some("refreshing")),
            DiscoveryReservationAction::Send
        );
        assert_eq!(
            discovery_reservation_action(false, Some("fresh")),
            DiscoveryReservationAction::Skip
        );
        assert_eq!(
            discovery_reservation_action(false, Some("failed")),
            DiscoveryReservationAction::Skip
        );
        assert_eq!(
            discovery_reservation_action(false, None),
            DiscoveryReservationAction::Skip
        );
    }

    #[test]
    fn discovery_source_status_sql_scopes_existing_reservation_by_params() {
        let sql = discovery_source_status_sql();

        assert!(sql.contains("SELECT status"));
        assert!(sql.contains("provider = 'scrapecreators'"));
        assert!(sql.contains("source = ?"));
        assert!(sql.contains("params_json = ?"));
        assert!(sql.contains("LIMIT 1"));
    }

    #[test]
    fn review_failure_outcome_respects_retry_budget() {
        assert_eq!(
            review_failure_outcome("ai_upstream_timeout", 0, 2),
            ReviewFailureOutcome::Retryable
        );
        assert_eq!(
            review_failure_outcome("ai_upstream_timeout", 1, 2),
            ReviewFailureOutcome::Failed
        );
        assert_eq!(
            review_failure_outcome("research_message_failed", 0, 2),
            ReviewFailureOutcome::Failed
        );
        assert_eq!(review_attempts_from_json(r#"{"attempts":2}"#), 2);
    }

    #[test]
    fn accepted_reference_cap_helper_blocks_over_cap_approvals() {
        assert_eq!(
            accepted_reference_cap_decision(0, 10, 0, 3),
            AcceptedReferenceCapDecision::Allow
        );
        assert_eq!(
            accepted_reference_cap_decision(10, 10, 0, 3),
            AcceptedReferenceCapDecision::RunCapReached
        );
        assert_eq!(
            accepted_reference_cap_decision(2, 10, 3, 3),
            AcceptedReferenceCapDecision::HandleCapReached
        );
    }

    #[test]
    fn scrapecreators_source_failure_action_finalizes_run() {
        let error = ScrapeCreatorsError::HttpStatus {
            status: 404,
            raw_json: None,
        };

        assert_eq!(
            scrapecreators_source_failure_action(&error),
            ScrapeSourceFailureAction::FinalizeRun
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
    fn matching_run_id_with_failed_status_skips_chunk_processing() {
        let current = ResearchStatusSnapshot {
            status: Some("research_failed"),
            run_id: Some("run_1"),
        };

        assert!(!research_chunk_processing_allowed(current, "run_1"));
    }

    #[test]
    fn terminal_statuses_skip_same_run_chunk_processing() {
        for current_status in [
            "pool_ready",
            "partial_pool_ready",
            "insufficient_refs",
            "research_failed",
        ] {
            let current = ResearchStatusSnapshot {
                status: Some(current_status),
                run_id: Some("run_1"),
            };

            assert!(!research_chunk_processing_allowed(current, "run_1"));
        }

        assert!(research_chunk_processing_allowed(
            ResearchStatusSnapshot {
                status: Some("scraping"),
                run_id: Some("run_1"),
            },
            "run_1"
        ));
    }

    #[test]
    fn finalization_side_effect_requires_written_status_transition() {
        assert!(status_write_allows_side_effects(
            ResearchStatusWriteResult::Written
        ));
        assert!(!status_write_allows_side_effects(
            ResearchStatusWriteResult::SkippedStale
        ));
        assert!(finalize_side_effect_allowed(
            ResearchStatusWriteResult::Written
        ));
        assert!(!finalize_side_effect_allowed(
            ResearchStatusWriteResult::SkippedStale
        ));
        assert!(!finalize_side_effect_allowed(
            ResearchStatusWriteResult::SkippedRaced
        ));
        assert!(!finalize_side_effect_allowed(
            ResearchStatusWriteResult::MissingClone
        ));
    }

    #[test]
    fn finalize_drain_action_only_proceeds_when_work_is_drained() {
        assert_eq!(
            finalize_drain_action(&FinalizeDrainState::default()),
            FinalizeDrainAction::Proceed
        );
        assert_eq!(
            finalize_drain_action(&FinalizeDrainState {
                pending_discovery: 1,
                ..FinalizeDrainState::default()
            }),
            FinalizeDrainAction::EnqueueFinalize
        );
        assert_eq!(
            finalize_drain_action(&FinalizeDrainState {
                pending_visual_work: 1,
                ..FinalizeDrainState::default()
            }),
            FinalizeDrainAction::EnqueueReview
        );
        assert_eq!(
            finalize_drain_action(&FinalizeDrainState {
                in_progress_visual_work: 1,
                ..FinalizeDrainState::default()
            }),
            FinalizeDrainAction::EnqueueDelayedFinalize
        );
        assert_eq!(
            finalize_drain_action(&FinalizeDrainState {
                cleanup_candidate_ids: vec!["candidate_1".to_string()],
                compatibility_candidate_ids: vec!["candidate_2".to_string()],
                approved_uncached_candidate_ids: vec!["candidate_3".to_string()],
                pending_visual_work: 1,
                ..FinalizeDrainState::default()
            }),
            FinalizeDrainAction::EnqueueCleanup
        );
        assert_eq!(
            finalize_drain_action(&FinalizeDrainState {
                compatibility_candidate_ids: vec!["candidate_1".to_string()],
                approved_uncached_candidate_ids: vec!["candidate_2".to_string()],
                pending_visual_work: 1,
                ..FinalizeDrainState::default()
            }),
            FinalizeDrainAction::EnqueueCompatibility
        );
        assert_eq!(
            finalize_drain_action(&FinalizeDrainState {
                approved_uncached_candidate_ids: vec!["candidate_1".to_string()],
                pending_visual_work: 1,
                ..FinalizeDrainState::default()
            }),
            FinalizeDrainAction::EnqueueCache
        );
    }

    #[test]
    fn reference_pool_readiness_requires_at_least_one_ready_moodboard_for_partial() {
        assert_eq!(
            reference_pool_readiness_phase(4, 0, 5),
            ResearchPhase::InsufficientRefs
        );
        assert_eq!(
            reference_pool_readiness_phase(5, 1, 5),
            ResearchPhase::PartialPoolReady
        );
        assert_eq!(
            reference_pool_readiness_phase(25, 5, 5),
            ResearchPhase::PoolReady
        );
    }

    #[test]
    fn final_ready_statuses_require_batch_orchestration_before_terminal_write() {
        for phase in [ResearchPhase::PoolReady, ResearchPhase::PartialPoolReady] {
            assert_eq!(
                finalize_readiness_action(phase, "ready", Some("soul_1")),
                FinalizeReadinessAction::OrchestrateBatchBeforeStatus
            );
        }
        assert_eq!(
            finalize_readiness_action(ResearchPhase::InsufficientRefs, "ready", Some("soul_1")),
            FinalizeReadinessAction::WriteStatusOnly
        );
        assert_eq!(
            finalize_readiness_action(ResearchPhase::PoolReady, "training", Some("soul_1")),
            FinalizeReadinessAction::WriteStatusOnly
        );
        assert_eq!(
            finalize_readiness_action(ResearchPhase::PoolReady, "ready", None),
            FinalizeReadinessAction::WriteStatusOnly
        );
        assert_eq!(
            finalize_readiness_action(ResearchPhase::PartialPoolReady, "ready", Some("   ")),
            FinalizeReadinessAction::WriteStatusOnly
        );
    }

    #[test]
    fn post_detail_fallback_targets_sidecar_and_underfilled_feed_items() {
        let raw = json!({
            "items": [
                {
                    "code": "CAR123",
                    "media_type": 8,
                    "carousel_media": [{"id": "child_1"}]
                },
                {
                    "code": "IMG123",
                    "media_type": 1,
                    "url": "https://www.instagram.com/p/IMG123/"
                }
            ]
        });
        let targets = instagram_post_detail_targets(&raw, &[], 3, 10);

        assert_eq!(
            targets,
            vec![
                InstagramPostDetailTarget {
                    source_url: "https://www.instagram.com/p/CAR123/".to_string(),
                },
                InstagramPostDetailTarget {
                    source_url: "https://www.instagram.com/p/IMG123/".to_string(),
                },
            ]
        );
    }

    #[test]
    fn post_detail_fallback_does_not_synthesize_source_url_from_id() {
        let raw = json!({
            "items": [
                {
                    "id": "1234567890123456789",
                    "media_type": 1
                },
                {
                    "code": "CODE123",
                    "media_type": 1
                },
                {
                    "shortcode": "SHORT123",
                    "media_type": 1
                },
                {
                    "id": "9876543210987654321",
                    "permalink": "https://www.instagram.com/p/PERM123/",
                    "media_type": 1
                }
            ]
        });
        let targets = instagram_post_detail_targets(&raw, &[], 3, 10);

        assert_eq!(
            targets,
            vec![
                InstagramPostDetailTarget {
                    source_url: "https://www.instagram.com/p/CODE123/".to_string(),
                },
                InstagramPostDetailTarget {
                    source_url: "https://www.instagram.com/p/SHORT123/".to_string(),
                },
                InstagramPostDetailTarget {
                    source_url: "https://www.instagram.com/p/PERM123/".to_string(),
                },
            ]
        );
    }

    #[test]
    fn post_detail_fallback_skips_reel_tv_and_video_items() {
        let raw = json!({
            "items": [
                {
                    "code": "REEL123",
                    "media_type": 2,
                    "url": "https://www.instagram.com/reel/REEL123/"
                },
                {
                    "code": "TV123",
                    "url": "https://www.instagram.com/tv/TV123/"
                },
                {
                    "code": "VID123",
                    "media_type": 2,
                    "url": "https://www.instagram.com/p/VID123/"
                },
                {
                    "code": "CLIP123",
                    "product_type": "clips",
                    "url": "https://www.instagram.com/p/CLIP123/"
                },
                {
                    "code": "CAR123",
                    "media_type": 8,
                    "url": "https://www.instagram.com/p/CAR123/",
                    "carousel_media": [{"id": "child_1"}]
                }
            ]
        });
        let targets = instagram_post_detail_targets(&raw, &[], 3, 10);

        assert_eq!(
            targets,
            vec![InstagramPostDetailTarget {
                source_url: "https://www.instagram.com/p/CAR123/".to_string(),
            }]
        );
    }

    #[test]
    fn post_detail_fallback_skips_meaningful_video_markers() {
        let raw = json!({
            "items": [
                {
                    "code": "VERSIONS123",
                    "url": "https://www.instagram.com/p/VERSIONS123/",
                    "video_versions": [{"url": "https://cdn.example/video.mp4"}]
                },
                {
                    "code": "VIDEOURL123",
                    "url": "https://www.instagram.com/p/VIDEOURL123/",
                    "video_url": "https://cdn.example/video.mp4"
                },
                {
                    "code": "STATIC123",
                    "url": "https://www.instagram.com/p/STATIC123/",
                    "video_versions": [],
                    "video_url": "",
                    "video_dash_manifest": null
                }
            ]
        });
        let targets = instagram_post_detail_targets(&raw, &[], 3, 10);

        assert_eq!(
            targets,
            vec![InstagramPostDetailTarget {
                source_url: "https://www.instagram.com/p/STATIC123/".to_string(),
            }]
        );
        assert!(instagram_feed_item_skipped_by_detail_policy(
            &raw["items"][0],
            "https://www.instagram.com/p/VERSIONS123/"
        ));
        assert!(instagram_feed_item_skipped_by_detail_policy(
            &raw["items"][1],
            "https://www.instagram.com/p/VIDEOURL123/"
        ));
        assert!(!instagram_feed_item_skipped_by_detail_policy(
            &raw["items"][2],
            "https://www.instagram.com/p/STATIC123/"
        ));
    }

    #[test]
    fn post_detail_source_params_carry_run_id_and_request_type() {
        let params = instagram_post_detail_source_params(
            "user_1",
            "clone_1",
            "run_1",
            "moodboard_1",
            "flash-editorial",
            "@creator",
            "configured_handle",
            "https://www.instagram.com/p/ABC123/",
        );

        assert_eq!(params.get("runId").and_then(Value::as_str), Some("run_1"));
        assert_eq!(
            params.get("requestType").and_then(Value::as_str),
            Some("instagram_post_detail")
        );
        assert_eq!(
            params.get("sourceUrl").and_then(Value::as_str),
            Some("https://www.instagram.com/p/ABC123/")
        );
    }

    #[test]
    fn stale_terminal_status_does_not_proceed_to_batch_creation() {
        let current = Some(ResearchStatusSnapshot {
            status: Some("research_failed"),
            run_id: Some("run_1"),
        });

        assert_eq!(
            research_status_write_decision(
                current,
                "pool_ready",
                ResearchStatusWriteMode::Normal,
                Some("run_1")
            ),
            ResearchStatusWriteDecision::SkipStale
        );
        assert!(!finalize_side_effect_allowed(
            ResearchStatusWriteResult::SkippedStale
        ));
    }

    #[test]
    fn same_run_scraping_status_can_continue_after_reviewing_status() {
        assert!(research_status_transition_allowed(
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
