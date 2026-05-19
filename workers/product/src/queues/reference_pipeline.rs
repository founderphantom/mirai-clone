use crate::ai::workers_ai::{global_visual_reference_review_prompt, run_vision_json};
use crate::db;
use crate::domain::global_reference::{
    accept_global_visual_review, instagram_source_image_key, GlobalVisualReferenceReview,
};
use crate::domain::moodboards::default_moodboards;
use crate::domain::visual_reference::MoodboardBrief;
use crate::instagram_references::{
    build_instagram_post_url, build_instagram_profile_url,
    build_instagram_reels_search_url_with_date_window, build_instagram_user_posts_url,
    extract_instagram_reels_owner_handles, instagram_candidate_meets_min_dimensions,
    normalize_instagram_post_detail, normalize_instagram_profile_related_handles,
    normalize_instagram_user_posts, InstagramFallbackPolicy, InstagramImageCandidate,
};
use crate::providers::higgsfield_auth::provider_account_access_token;
use crate::providers::higgsfield_mcp::{call_tool, upload_media_files, HiggsfieldMcpMediaFile};
use crate::providers::seedream::{
    extract_seedream_cleaned_image_url, seedream_cleanup_arguments_with_model,
    SEEDREAM_CLEANUP_MODEL,
};
use crate::queues::messages::ReferencePipelineMessage;
use crate::scrapecreators::fetch_scrapecreators_json;
use crate::services::global_reference_discovery::{
    audit_global_candidate_discovery_sql, bootstrap_global_search_state_sql,
    select_global_handle_work_sql, select_global_search_work_sql, source_key_for_instagram_handle,
    source_key_for_reels_search, upsert_global_candidate_sql, upsert_global_handle_sql,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use worker::{D1Database, Env, Error, MessageBatch, MessageExt, Result as WorkerResult};

const REFERENCE_PIPELINE_QUEUE_BINDING: &str = "REFERENCE_PIPELINE_QUEUE";
const HIGGSFIELD_REFRESH_SECRET_NAME: &str = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER";
const HIGGSFIELD_PROVIDER_ACCOUNT_ID: &str = "pa_higgsfield_founder";
const HIGGSFIELD_CLEANUP_TOOL_VAR: &str = "HIGGSFIELD_MCP_CLEANUP_TOOL";
const HIGGSFIELD_CLEANUP_MODEL_VAR: &str = "HIGGSFIELD_MCP_CLEANUP_MODEL";

pub async fn handle_batch(batch: MessageBatch<Value>, env: Env) -> WorkerResult<()> {
    let db = env.d1("DB")?;
    for raw_message in batch.raw_iter() {
        let message =
            match serde_wasm_bindgen::from_value::<ReferencePipelineMessage>(raw_message.body()) {
                Ok(message) => message,
                Err(error) => {
                    web_sys::console::error_1(
                        &format!(
                            "failed to deserialize reference pipeline queue message: {error:?}"
                        )
                        .into(),
                    );
                    raw_message.ack();
                    continue;
                }
            };

        match handle_message(&db, &env, message).await {
            Ok(()) => raw_message.ack(),
            Err(error) => {
                web_sys::console::error_1(
                    &format!("reference pipeline queue message failed: {error}").into(),
                );
                raw_message.retry();
            }
        }
    }

    Ok(())
}

async fn handle_message(
    db: &D1Database,
    env: &Env,
    message: ReferencePipelineMessage,
) -> WorkerResult<()> {
    match message {
        ReferencePipelineMessage::EnsureGlobalMoodboardLibrary {
            moodboard_slug,
            reason,
        } => ensure_global_moodboard_library(db, env, &moodboard_slug, &reason).await,
        ReferencePipelineMessage::DiscoverGlobalInstagramHandles {
            moodboard_slug,
            run_id,
            search_term,
            date_window,
            page,
        } => {
            discover_global_instagram_handles(
                db,
                env,
                &moodboard_slug,
                &run_id,
                &search_term,
                &date_window,
                page,
            )
            .await
        }
        ReferencePipelineMessage::FetchGlobalInstagramProfile {
            moodboard_slug,
            run_id,
            handle,
            discovered_via,
            related_depth,
        } => {
            fetch_global_instagram_profile(
                db,
                env,
                &moodboard_slug,
                &run_id,
                &handle,
                &discovered_via,
                related_depth,
            )
            .await
        }
        ReferencePipelineMessage::FetchGlobalInstagramPosts {
            moodboard_slug,
            run_id,
            handle,
            discovered_via,
            next_max_id,
            page,
        } => {
            fetch_global_instagram_posts(
                db,
                env,
                &moodboard_slug,
                &run_id,
                &handle,
                &discovered_via,
                next_max_id.as_deref(),
                page,
            )
            .await
        }
        ReferencePipelineMessage::FetchGlobalInstagramPostDetail {
            moodboard_slug,
            run_id,
            handle,
            discovered_via,
            source_url,
        } => {
            fetch_global_instagram_post_detail(
                db,
                env,
                &moodboard_slug,
                &run_id,
                &handle,
                &discovered_via,
                &source_url,
            )
            .await
        }
        ReferencePipelineMessage::ReviewGlobalVisualCandidates {
            moodboard_slug,
            run_id,
            limit,
        } => review_global_visual_candidates(db, env, &moodboard_slug, &run_id, limit).await,
        ReferencePipelineMessage::CleanupGlobalMoodboardReference {
            moodboard_slug,
            run_id,
            candidate_id,
        } => {
            cleanup_global_moodboard_reference(db, env, &moodboard_slug, &run_id, &candidate_id)
                .await
        }
        ReferencePipelineMessage::FinalizeGlobalMoodboardLibrary {
            moodboard_slug,
            run_id,
            reason,
        } => finalize_global_moodboard_library(db, env, &moodboard_slug, &run_id, &reason).await,
        ReferencePipelineMessage::BuildCloneReferencePool { .. }
        | ReferencePipelineMessage::RefreshPool { .. }
        | ReferencePipelineMessage::ValidateCloneCompatibility { .. }
        | ReferencePipelineMessage::FinalizeCloneReferencePool { .. } => {
            clone_pool_messages_are_enabled_in_part_three();
            Ok(())
        }
    }
}

async fn ensure_global_moodboard_library(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    reason: &str,
) -> WorkerResult<()> {
    sync_default_global_moodboard_definitions(db).await?;
    let Some(definition) = load_active_global_moodboard_definition(db, moodboard_slug).await?
    else {
        return Ok(());
    };

    let now = now_iso_string();
    ensure_global_reference_state(db, &definition.slug, &now).await?;
    let target = config_value_u32(db, "global_refs_per_moodboard_target", 25).await?;
    if active_global_reference_count(db, &definition.slug).await? >= target {
        return enqueue_finalize_global_moodboard_library(
            env,
            &definition.slug,
            "",
            "already_at_target",
        )
        .await;
    }
    if let Some(run) = load_reusable_current_global_source_run(db, &definition.slug).await? {
        return enqueue_global_source_work_for_run(db, env, &definition.slug, &run.id).await;
    }
    if !global_reference_state_retry_is_due(db, &definition.slug, &now).await? {
        return Ok(());
    }

    let run_id = new_global_run_id();
    let selected_terms = selected_search_terms(
        &definition.search_queries_json,
        definition.title.as_str(),
        2,
    );
    let date_windows = vec!["last-month".to_string()];
    create_global_source_run(
        db,
        &run_id,
        &definition.slug,
        reason,
        &selected_terms,
        &date_windows,
    )
    .await?;
    set_current_global_source_run(db, &definition.slug, &run_id, target, &now).await?;

    let reels_pages_per_term = config_value_u32(db, "instagram_reels_pages_per_term", 1)
        .await?
        .max(1);
    for search_term in &selected_terms {
        for date_window in &date_windows {
            for page in 1..=reels_pages_per_term {
                db::exec(
                    db,
                    bootstrap_global_search_state_sql(),
                    vec![
                        json!(deterministic_id(
                            "global_search",
                            &[
                                &definition.slug,
                                search_term,
                                date_window,
                                &page.to_string()
                            ],
                        )),
                        json!(definition.slug),
                        json!(search_term),
                        json!(date_window),
                        json!(page),
                        json!(now),
                        json!(now),
                    ],
                )
                .await?;
            }
        }
    }

    enqueue_global_source_work_for_run(db, env, &definition.slug, &run_id).await
}

async fn discover_global_instagram_handles(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    search_term: &str,
    date_window: &str,
    page: u32,
) -> WorkerResult<()> {
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
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
    let request_url = match build_instagram_reels_search_url_with_date_window(
        &base_url,
        search_term,
        Some(page),
        Some(date_window),
    ) {
        Ok(url) => url,
        Err(error) => {
            record_scrapecreators_search_failure_and_enqueue_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                search_term,
                date_window,
                page,
                "instagram_reels_search_url_failed",
                error,
            )
            .await?;
            return Ok(());
        }
    };
    let raw = match fetch_scrapecreators_json(&request_url, &api_key).await {
        Ok(raw) => raw,
        Err(error) => {
            if !global_run_is_current(db, moodboard_slug, run_id).await? {
                return Ok(());
            }
            record_scrapecreators_search_failure_and_enqueue_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                search_term,
                date_window,
                page,
                "scrapecreators_search_failed",
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    };
    if !ensure_current_global_run_after_provider_fetch(db, moodboard_slug, run_id).await? {
        return Ok(());
    }
    let max_handles = config_value_u32(db, "instagram_max_handles_per_moodboard", 20)
        .await?
        .max(1);
    let handles = extract_instagram_reels_owner_handles(&raw, max_handles as usize);
    let now = now_iso_string();
    let source_key = source_key_for_reels_search(search_term, date_window, page);

    for handle in &handles {
        upsert_global_handle(db, moodboard_slug, handle, &source_key, 0, &now).await?;
    }
    mark_global_search_state_seen(
        db,
        moodboard_slug,
        search_term,
        date_window,
        page,
        handles.len() as u32,
        &now,
    )
    .await?;
    increment_global_source_run_count(db, run_id, "discovered_handle_count", handles.len() as u32)
        .await?;

    for handle in handles {
        env.queue(REFERENCE_PIPELINE_QUEUE_BINDING)?
            .send(ReferencePipelineMessage::FetchGlobalInstagramProfile {
                moodboard_slug: moodboard_slug.to_string(),
                run_id: run_id.to_string(),
                handle,
                discovered_via: source_key.clone(),
                related_depth: 0,
            })
            .await?;
    }

    Ok(())
}

async fn fetch_global_instagram_profile(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    handle: &str,
    discovered_via: &str,
    related_depth: u8,
) -> WorkerResult<()> {
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
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
    let request_url = match build_instagram_profile_url(&base_url, handle) {
        Ok(url) => url,
        Err(error) => {
            record_scrapecreators_handle_failure_and_enqueue_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                handle,
                "instagram_profile_url_failed",
                error,
            )
            .await?;
            return Ok(());
        }
    };
    let raw = match fetch_scrapecreators_json(&request_url, &api_key).await {
        Ok(raw) => raw,
        Err(error) => {
            if !global_run_is_current(db, moodboard_slug, run_id).await? {
                return Ok(());
            }
            record_scrapecreators_handle_failure_and_enqueue_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                handle,
                "scrapecreators_profile_failed",
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    };
    if !ensure_current_global_run_after_provider_fetch(db, moodboard_slug, run_id).await? {
        return Ok(());
    }
    let now = now_iso_string();

    if related_depth == 0 {
        let related_limit = config_value_u32(db, "instagram_related_handles_per_profile", 8)
            .await?
            .max(1) as usize;
        for related in normalize_instagram_profile_related_handles(&raw, related_limit) {
            let source_key = source_key_for_instagram_handle(handle, "profile_related");
            upsert_global_handle(
                db,
                moodboard_slug,
                &related,
                &source_key,
                related_depth.saturating_add(1),
                &now,
            )
            .await?;
        }
    }

    env.queue(REFERENCE_PIPELINE_QUEUE_BINDING)?
        .send(ReferencePipelineMessage::FetchGlobalInstagramPosts {
            moodboard_slug: moodboard_slug.to_string(),
            run_id: run_id.to_string(),
            handle: handle.to_string(),
            discovered_via: discovered_via.to_string(),
            next_max_id: None,
            page: 1,
        })
        .await?;

    Ok(())
}

async fn fetch_global_instagram_posts(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    handle: &str,
    discovered_via: &str,
    next_max_id: Option<&str>,
    page: u8,
) -> WorkerResult<()> {
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
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
    let request_url = match build_instagram_user_posts_url(&base_url, handle, next_max_id) {
        Ok(url) => url,
        Err(error) => {
            record_scrapecreators_handle_failure_and_enqueue_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                handle,
                "instagram_posts_url_failed",
                error,
            )
            .await?;
            return Ok(());
        }
    };
    let raw = match fetch_scrapecreators_json(&request_url, &api_key).await {
        Ok(raw) => raw,
        Err(error) => {
            if !global_run_is_current(db, moodboard_slug, run_id).await? {
                return Ok(());
            }
            record_scrapecreators_handle_failure_and_enqueue_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                handle,
                "scrapecreators_posts_failed",
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    };
    if !ensure_current_global_run_after_provider_fetch(db, moodboard_slug, run_id).await? {
        return Ok(());
    }
    let images_per_post = config_value_u32(db, "instagram_images_per_post", 3)
        .await?
        .max(1) as usize;
    let min_width = config_value_u32(db, "instagram_min_image_width", 512).await?;
    let min_height = config_value_u32(db, "instagram_min_image_height", 512).await?;
    let candidates = normalize_instagram_user_posts(
        &raw,
        handle,
        "",
        moodboard_slug,
        discovered_via,
        InstagramFallbackPolicy::SkipVideos,
        images_per_post,
    )
    .into_iter()
    .filter(|candidate| instagram_candidate_meets_min_dimensions(candidate, min_width, min_height))
    .collect::<Vec<_>>();
    let source_key = source_key_for_instagram_handle(handle, next_max_id.unwrap_or("posts:first"));
    upsert_global_candidates_and_audit(db, moodboard_slug, run_id, &source_key, &candidates)
        .await?;
    mark_global_handle_fetch_result(
        db,
        moodboard_slug,
        handle,
        next_max_id_from(&raw),
        &now_iso_string(),
    )
    .await?;
    increment_global_source_run_count(db, run_id, "candidate_count", candidates.len() as u32)
        .await?;

    let detail_limit = config_value_u32(db, "instagram_post_detail_limit_per_profile", 4)
        .await?
        .max(1) as usize;
    for target in instagram_post_detail_targets(&raw, &candidates, images_per_post, detail_limit) {
        env.queue(REFERENCE_PIPELINE_QUEUE_BINDING)?
            .send(ReferencePipelineMessage::FetchGlobalInstagramPostDetail {
                moodboard_slug: moodboard_slug.to_string(),
                run_id: run_id.to_string(),
                handle: handle.to_string(),
                discovered_via: discovered_via.to_string(),
                source_url: target.source_url,
            })
            .await?;
    }

    let pages_per_profile = config_value_u32(db, "instagram_pages_per_profile", 1)
        .await?
        .max(1) as u8;
    if page < pages_per_profile {
        if let Some(cursor) = next_max_id_from(&raw) {
            env.queue(REFERENCE_PIPELINE_QUEUE_BINDING)?
                .send(ReferencePipelineMessage::FetchGlobalInstagramPosts {
                    moodboard_slug: moodboard_slug.to_string(),
                    run_id: run_id.to_string(),
                    handle: handle.to_string(),
                    discovered_via: discovered_via.to_string(),
                    next_max_id: Some(cursor),
                    page: page.saturating_add(1),
                })
                .await?;
        }
    }

    enqueue_review_global_visual_candidates(env, moodboard_slug, run_id, 60).await?;
    enqueue_finalize_global_moodboard_library(env, moodboard_slug, run_id, "posts_fetched").await
}

async fn fetch_global_instagram_post_detail(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    handle: &str,
    discovered_via: &str,
    source_url: &str,
) -> WorkerResult<()> {
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
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
    let request_url = match build_instagram_post_url(&base_url, source_url, "US") {
        Ok(url) => url,
        Err(error) => {
            record_scrapecreators_handle_failure_and_enqueue_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                handle,
                "instagram_post_detail_url_failed",
                error,
            )
            .await?;
            return Ok(());
        }
    };
    let raw = match fetch_scrapecreators_json(&request_url, &api_key).await {
        Ok(raw) => raw,
        Err(error) => {
            if !global_run_is_current(db, moodboard_slug, run_id).await? {
                return Ok(());
            }
            record_scrapecreators_handle_failure_and_enqueue_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                handle,
                "scrapecreators_post_detail_failed",
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    };
    if !ensure_current_global_run_after_provider_fetch(db, moodboard_slug, run_id).await? {
        return Ok(());
    }
    let images_per_post = config_value_u32(db, "instagram_images_per_post", 3)
        .await?
        .max(1) as usize;
    let min_width = config_value_u32(db, "instagram_min_image_width", 512).await?;
    let min_height = config_value_u32(db, "instagram_min_image_height", 512).await?;
    let candidates = normalize_instagram_post_detail(
        &raw,
        handle,
        source_url,
        "",
        moodboard_slug,
        discovered_via,
        images_per_post,
    )
    .into_iter()
    .filter(|candidate| instagram_candidate_meets_min_dimensions(candidate, min_width, min_height))
    .collect::<Vec<_>>();
    let source_key = source_key_for_instagram_handle(handle, source_url);
    upsert_global_candidates_and_audit(db, moodboard_slug, run_id, &source_key, &candidates)
        .await?;
    increment_global_source_run_count(db, run_id, "candidate_count", candidates.len() as u32)
        .await?;

    enqueue_review_global_visual_candidates(env, moodboard_slug, run_id, 60).await?;
    enqueue_finalize_global_moodboard_library(env, moodboard_slug, run_id, "post_detail_fetched")
        .await
}

async fn review_global_visual_candidates(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    limit: u32,
) -> WorkerResult<()> {
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
        return Ok(());
    }

    let active_moodboards = load_active_global_moodboard_briefs(db).await?;
    let configured_limit = config_value_u32(db, "instagram_candidate_review_limit", 80).await?;
    let review_limit = if limit == 0 {
        configured_limit
    } else {
        limit.min(configured_limit)
    }
    .max(1);
    let review_retry_limit = config_value_u32(db, "visual_reference_review_retry_limit", 2)
        .await?
        .max(1);
    let now = now_iso_string();
    let candidates = db::all::<GlobalVisualCandidateReviewRow>(
        db,
        select_global_candidates_for_review_sql(),
        vec![
            json!(run_id),
            json!(moodboard_slug),
            json!(review_retry_limit),
            json!(now),
            json!(now),
            json!(review_retry_limit),
            json!(review_limit),
        ],
    )
    .await?;
    let ai = env.ai("AI")?;

    for candidate in candidates {
        let _observed_attempt_count = candidate.review_attempt_count;
        if !global_run_is_current(db, moodboard_slug, run_id).await? {
            return Ok(());
        }

        let claim_id = new_global_review_claim_id();
        let locked_until = retry_after_minutes(15);
        let claim_result = db::run(
            db,
            claim_global_candidate_for_review_sql(),
            vec![
                json!(run_id),
                json!(claim_id),
                json!(locked_until),
                json!(now_iso_string()),
                json!(candidate.id),
                json!(review_retry_limit),
                json!(now_iso_string()),
                json!(now_iso_string()),
                json!(review_retry_limit),
                json!(moodboard_slug),
                json!(run_id),
            ],
        )
        .await?;
        if changed_rows(&claim_result)? == 0 {
            continue;
        }
        if !global_run_is_current(db, moodboard_slug, run_id).await? {
            return Ok(());
        }

        let source_handle = candidate.source_handle.as_deref().unwrap_or("");
        let prompt = global_visual_reference_review_prompt(
            &active_moodboards,
            &candidate.platform,
            source_handle,
            candidate.source_caption.as_deref(),
            candidate.like_count,
            candidate.comment_count,
            candidate.source_published_at.as_deref(),
        );
        let review = match run_vision_json::<GlobalVisualReferenceReview>(
            &ai,
            &prompt,
            &candidate.image_url,
        )
        .await
        {
            Ok(review) => review,
            Err(error) => {
                let code = queue_error_code(&error.to_string());
                db::exec(
                    db,
                    mark_global_candidate_review_failed_sql(),
                    vec![
                        json!(review_retry_limit),
                        json!(code),
                        json!(compact_error_detail(&error.to_string())),
                        json!(review_retry_limit),
                        json!(retry_after_minutes(15)),
                        json!(now_iso_string()),
                        json!(candidate.id),
                        json!(run_id),
                        json!(claim_id),
                        json!(moodboard_slug),
                        json!(run_id),
                    ],
                )
                .await?;
                continue;
            }
        };

        let review_json = serde_json::to_string(&review).unwrap_or_else(|_| "{}".to_string());
        match accept_global_visual_review(&review, &active_moodboards) {
            Ok(accepted) => {
                let write_now = now_iso_string();
                let result = db::run(
                    db,
                    mark_global_candidate_review_approved_sql(),
                    vec![
                        json!(accepted.moodboard_slug),
                        json!(review_json),
                        json!(write_now),
                        json!(write_now),
                        json!(candidate.id),
                        json!(run_id),
                        json!(claim_id),
                        json!(moodboard_slug),
                        json!(run_id),
                    ],
                )
                .await?;
                if changed_rows(&result)? > 0 {
                    env.queue(REFERENCE_PIPELINE_QUEUE_BINDING)?
                        .send(ReferencePipelineMessage::CleanupGlobalMoodboardReference {
                            moodboard_slug: moodboard_slug.to_string(),
                            run_id: run_id.to_string(),
                            candidate_id: candidate.id,
                        })
                        .await?;
                }
            }
            Err(_) => {
                let write_now = now_iso_string();
                db::exec(
                    db,
                    mark_global_candidate_review_rejected_sql(),
                    vec![
                        json!(review_json),
                        json!(write_now),
                        json!(write_now),
                        json!(candidate.id),
                        json!(run_id),
                        json!(claim_id),
                        json!(moodboard_slug),
                        json!(run_id),
                    ],
                )
                .await?;
            }
        }
    }

    if !global_run_is_current(db, moodboard_slug, run_id).await? {
        return Ok(());
    }
    if global_review_has_eligible_rows(db, moodboard_slug, run_id, review_retry_limit).await? {
        enqueue_review_global_visual_candidates(env, moodboard_slug, run_id, review_limit).await
    } else {
        enqueue_finalize_global_moodboard_library(
            env,
            moodboard_slug,
            run_id,
            "visual_candidate_review_completed",
        )
        .await
    }
}

async fn cleanup_global_moodboard_reference(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    candidate_id: &str,
) -> WorkerResult<()> {
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
        return Ok(());
    }

    let retry_limit = config_value_u32(db, "visual_reference_cleanup_retry_limit", 3)
        .await?
        .max(1);
    let now = now_iso_string();
    let Some(candidate) = db::first::<GlobalVisualCandidateCleanupRow>(
        db,
        load_global_candidate_for_cleanup_sql(),
        vec![
            json!(candidate_id),
            json!(now),
            json!(now),
            json!(moodboard_slug),
            json!(run_id),
        ],
    )
    .await?
    else {
        resume_or_finalize_cleaned_global_candidate(
            db,
            env,
            moodboard_slug,
            run_id,
            candidate_id,
            retry_limit,
        )
        .await?;
        return Ok(());
    };

    let cleanup_lock_expires_at = retry_after_minutes(15);
    let claim_result = db::run(
        db,
        claim_global_candidate_for_cleanup_sql(),
        vec![
            json!(cleanup_lock_expires_at),
            json!(now_iso_string()),
            json!(candidate_id),
            json!(retry_limit),
            json!(now_iso_string()),
            json!(now_iso_string()),
            json!(moodboard_slug),
            json!(run_id),
        ],
    )
    .await?;
    if changed_rows(&claim_result)? == 0 {
        if !resume_or_finalize_cleaned_global_candidate(
            db,
            env,
            moodboard_slug,
            run_id,
            candidate_id,
            retry_limit,
        )
        .await?
        {
            enqueue_finalize_global_moodboard_library(
                env,
                moodboard_slug,
                run_id,
                "global_cleanup_claim_skipped",
            )
            .await?;
        }
        return Ok(());
    }
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
        return Ok(());
    }

    let review = match serde_json::from_str::<GlobalVisualReferenceReview>(&candidate.review_json) {
        Ok(review) => review,
        Err(error) => {
            record_global_candidate_cleanup_failure_and_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                candidate_id,
                retry_limit,
                &cleanup_lock_expires_at,
                "global_review_json_malformed",
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    };

    let cleanup_auth = match prepare_global_seedream_cleanup_auth(env).await {
        Ok(auth) => auth,
        Err(error) => {
            record_global_candidate_cleanup_failure_and_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                candidate_id,
                retry_limit,
                &cleanup_lock_expires_at,
                "global_seedream_cleanup_auth_failed",
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    };
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
        return Ok(());
    }

    let source_image = match fetch_global_seedream_cleanup_image(&candidate.image_url).await {
        Ok(image) => image,
        Err(error) => {
            record_global_candidate_cleanup_failure_and_finalize(
                db,
                env,
                moodboard_slug,
                run_id,
                candidate_id,
                retry_limit,
                &cleanup_lock_expires_at,
                "global_seedream_cleanup_fetch_failed",
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    };
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
        return Ok(());
    }

    let cleanup_request =
        match upload_global_seedream_cleanup_image(&cleanup_auth, candidate_id, source_image).await
        {
            Ok(request) => request,
            Err(error) => {
                record_global_candidate_cleanup_failure_and_finalize(
                    db,
                    env,
                    moodboard_slug,
                    run_id,
                    candidate_id,
                    retry_limit,
                    &cleanup_lock_expires_at,
                    "global_seedream_cleanup_upload_failed",
                    &error.to_string(),
                )
                .await?;
                return Ok(());
            }
        };
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
        return Ok(());
    }

    let (cleaned_url, provider_job_id, raw_cleanup_json) =
        match call_global_seedream_cleanup(candidate_id, &cleanup_request).await {
            Ok(result) => result,
            Err(error) => {
                record_global_candidate_cleanup_failure_and_finalize(
                    db,
                    env,
                    moodboard_slug,
                    run_id,
                    candidate_id,
                    retry_limit,
                    &cleanup_lock_expires_at,
                    "global_seedream_cleanup_failed",
                    &error.to_string(),
                )
                .await?;
                return Ok(());
            }
        };
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
        return Ok(());
    }

    let cleanup_json = serde_json::to_string(&json!({
        "provider": "higgsfield_mcp",
        "tool": cleanup_request.tool_name,
        "model": cleanup_request.model,
        "providerJobId": provider_job_id,
        "cleanedImageUrl": cleaned_url,
        "raw": raw_cleanup_json,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    let write_now = now_iso_string();
    let mark_result = db::run(
        db,
        mark_global_candidate_cleanup_succeeded_sql(),
        vec![
            json!(cleaned_url),
            json!(cleanup_json),
            json!(write_now),
            json!(write_now),
            json!(candidate_id),
            json!(cleanup_lock_expires_at),
            json!(moodboard_slug),
            json!(run_id),
        ],
    )
    .await?;
    if changed_rows(&mark_result)? == 0 {
        enqueue_finalize_global_moodboard_library(
            env,
            moodboard_slug,
            run_id,
            "global_cleanup_success_write_skipped",
        )
        .await?;
        return Ok(());
    }
    complete_cleaned_global_moodboard_reference(
        db,
        env,
        moodboard_slug,
        run_id,
        candidate_id,
        retry_limit,
        &cleanup_lock_expires_at,
        &GlobalVisualCandidateReferenceRow {
            cleaned_image_url: cleaned_url,
            image_width: candidate.image_width,
            image_height: candidate.image_height,
            discovery_moodboard_slug: candidate.discovery_moodboard_slug,
            assigned_moodboard_slug: candidate.assigned_moodboard_slug,
            source_handle: candidate.source_handle,
            review_json: candidate.review_json,
        },
        Some(review),
    )
    .await
}

async fn finalize_global_moodboard_library(
    db: &D1Database,
    _env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    reason: &str,
) -> WorkerResult<()> {
    let Some(source_run) =
        load_current_global_source_run_for_finalize(db, moodboard_slug, run_id).await?
    else {
        return Ok(());
    };

    let now = now_iso_string();
    let impacted_slugs = impacted_global_moodboard_slugs(db, moodboard_slug, run_id).await?;
    let target = config_value_u32(db, "global_refs_per_moodboard_target", 25)
        .await?
        .max(1);
    let review_retry_limit = config_value_u32(db, "visual_reference_review_retry_limit", 2)
        .await?
        .max(1);
    let cleanup_retry_limit = config_value_u32(db, "visual_reference_cleanup_retry_limit", 3)
        .await?
        .max(1);
    let retry_after_hours = config_value_u32(db, "global_insufficient_retry_after_hours", 6)
        .await?
        .max(1);
    let run_failed = source_run.error_code.is_some();
    let mut source_slug_work_exists = false;
    let mut source_slug_status = "refreshing";

    for impacted_slug in impacted_slugs {
        ensure_global_reference_state(db, &impacted_slug, &now).await?;
        let active_count = active_global_reference_count(db, &impacted_slug).await?;
        let retryable_candidate_work = retryable_global_candidate_work_count(
            db,
            moodboard_slug,
            run_id,
            &impacted_slug,
            review_retry_limit,
            cleanup_retry_limit,
            &now,
        )
        .await?;
        let in_flight_candidate_work = in_flight_global_candidate_work_count(
            db,
            moodboard_slug,
            run_id,
            &impacted_slug,
            &now,
        )
        .await?;
        let eligible_source_work =
            eligible_global_source_work_count(db, &impacted_slug, &now).await?;
        let work_exists = retryable_candidate_work > 0 || in_flight_candidate_work > 0
            || eligible_source_work > 0;
        let status = if run_failed && active_count == 0 && !work_exists {
            "discovery_failed"
        } else if active_count >= target {
            "library_ready"
        } else if active_count > 0 && work_exists {
            "underfilled"
        } else if active_count > 0 {
            "underfilled_exhausted"
        } else if active_count == 0 && work_exists {
            "refreshing"
        } else if work_exists {
            "refreshing"
        } else {
            "insufficient_refs"
        };
        if impacted_slug == moodboard_slug {
            source_slug_work_exists = work_exists;
            source_slug_status = status;
        }
        let underfilled = (active_count < target) as u8;
        let next_retry_at = match status {
            "underfilled_exhausted" | "insufficient_refs" | "discovery_failed" => {
                earliest_global_retry_at(db, moodboard_slug, run_id, &impacted_slug).await?
                    .or_else(|| Some(retry_after_iso(retry_after_hours)))
            }
            _ => None,
        };
        let has_successful_references = active_count > 0;

        update_global_reference_state_after_recount(
            db,
            moodboard_slug,
            run_id,
            &impacted_slug,
            active_count,
            status,
            underfilled,
            next_retry_at.as_deref(),
            has_successful_references,
            status == "library_ready",
            status == "underfilled" || status == "underfilled_exhausted",
            status == "insufficient_refs" || status == "discovery_failed",
            &now,
        )
        .await?;
    }

    if source_slug_work_exists {
        return Ok(());
    }
    let source_run_terminal_status = if source_slug_status == "discovery_failed" {
        "discovery_failed"
    } else {
        "completed"
    };

    complete_global_source_run_after_recount(
        db,
        moodboard_slug,
        run_id,
        source_run_terminal_status,
        reason,
        source_run.error_code.as_deref(),
        source_run.error_message.as_deref(),
        &now,
    )
    .await
}

fn clone_pool_messages_are_enabled_in_part_three() {}

async fn sync_default_global_moodboard_definitions(db: &D1Database) -> WorkerResult<()> {
    let now = now_iso_string();
    for (index, seed) in default_moodboards().into_iter().enumerate() {
        let search_queries_json =
            serde_json::to_string(&seed.search_queries).unwrap_or_else(|_| "[]".to_string());
        db::exec(
            db,
            r#"
            INSERT INTO global_moodboard_definitions (
              slug,
              title,
              vibe_summary,
              search_queries_json,
              sort_order,
              status,
              created_at,
              updated_at
            )
            VALUES (?, ?, ?, ?, ?, 'active', ?, ?)
            ON CONFLICT(slug) DO UPDATE SET
              title = excluded.title,
              vibe_summary = excluded.vibe_summary,
              search_queries_json = excluded.search_queries_json,
              sort_order = excluded.sort_order,
              updated_at = excluded.updated_at
            WHERE global_moodboard_definitions.status = 'active'
            "#,
            vec![
                json!(seed.slug),
                json!(seed.title),
                json!(seed.vibe_summary),
                json!(search_queries_json),
                json!(index),
                json!(now),
                json!(now),
            ],
        )
        .await?;
    }
    Ok(())
}

async fn load_active_global_moodboard_definition(
    db: &D1Database,
    moodboard_slug: &str,
) -> WorkerResult<Option<GlobalMoodboardDefinitionRow>> {
    db::first(
        db,
        r#"
        SELECT slug, title, vibe_summary, search_queries_json
        FROM global_moodboard_definitions
        WHERE slug = ?
          AND status = 'active'
        LIMIT 1
        "#,
        vec![json!(moodboard_slug)],
    )
    .await
}

async fn load_active_global_moodboard_briefs(db: &D1Database) -> WorkerResult<Vec<MoodboardBrief>> {
    let rows = db::all::<GlobalMoodboardDefinitionRow>(
        db,
        r#"
        SELECT slug, title, vibe_summary, search_queries_json
        FROM global_moodboard_definitions
        WHERE status = 'active'
        ORDER BY sort_order ASC, slug ASC
        "#,
        vec![],
    )
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| MoodboardBrief {
            id: format!("global_moodboard_{}", row.slug),
            slug: row.slug,
            title: row.title,
            vibe_summary: row.vibe_summary,
            search_queries: serde_json::from_str(&row.search_queries_json).unwrap_or_default(),
        })
        .collect())
}

async fn ensure_global_reference_state(
    db: &D1Database,
    moodboard_slug: &str,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        INSERT OR IGNORE INTO global_moodboard_reference_state (
          moodboard_slug,
          status,
          created_at,
          updated_at
        )
        VALUES (?, 'queued', ?, ?)
        "#,
        vec![json!(moodboard_slug), json!(now), json!(now)],
    )
    .await
}

async fn global_reference_state_retry_is_due(
    db: &D1Database,
    moodboard_slug: &str,
    now: &str,
) -> WorkerResult<bool> {
    let row = db::first::<GlobalReferenceStateRetryRow>(
        db,
        r#"
        SELECT next_retry_at
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
        LIMIT 1
        "#,
        vec![json!(moodboard_slug)],
    )
    .await?;
    Ok(row
        .and_then(|row| row.next_retry_at)
        .map(|next_retry_at| next_retry_at.as_str() <= now)
        .unwrap_or(true))
}

async fn load_reusable_current_global_source_run(
    db: &D1Database,
    moodboard_slug: &str,
) -> WorkerResult<Option<ReusableGlobalSourceRunRow>> {
    db::first(
        db,
        r#"
        SELECT run.id
        FROM global_moodboard_reference_state state
        INNER JOIN global_moodboard_source_runs run
          ON run.id = state.current_run_id
        WHERE state.moodboard_slug = ?
          AND run.moodboard_slug = ?
          AND run.status IN ('queued', 'refreshing')
        LIMIT 1
        "#,
        vec![json!(moodboard_slug), json!(moodboard_slug)],
    )
    .await
}

async fn create_global_source_run(
    db: &D1Database,
    run_id: &str,
    moodboard_slug: &str,
    reason: &str,
    selected_terms: &[String],
    date_windows: &[String],
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        INSERT INTO global_moodboard_source_runs (
          id,
          moodboard_slug,
          status,
          reason,
          selected_search_terms_json,
          selected_date_windows_json,
          created_at,
          updated_at,
          started_at
        )
        VALUES (?, ?, 'refreshing', ?, ?, ?, ?, ?, ?)
        "#,
        vec![
            json!(run_id),
            json!(moodboard_slug),
            json!(reason),
            json!(serde_json::to_string(selected_terms).unwrap_or_else(|_| "[]".to_string())),
            json!(serde_json::to_string(date_windows).unwrap_or_else(|_| "[]".to_string())),
            json!(now),
            json!(now),
            json!(now),
        ],
    )
    .await
}

async fn enqueue_global_source_work_for_run(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let search_limit = config_value_u32(db, "instagram_search_terms_per_moodboard", 2)
        .await?
        .max(1)
        * config_value_u32(db, "instagram_reels_pages_per_term", 1)
            .await?
            .max(1);
    for work in select_global_search_work(db, moodboard_slug, &now, search_limit).await? {
        env.queue(REFERENCE_PIPELINE_QUEUE_BINDING)?
            .send(ReferencePipelineMessage::DiscoverGlobalInstagramHandles {
                moodboard_slug: work.moodboard_slug,
                run_id: run_id.to_string(),
                search_term: work.search_term,
                date_window: work.date_window,
                page: work.page,
            })
            .await?;
    }

    let handle_limit = config_value_u32(db, "instagram_max_profiles_per_run", 20)
        .await?
        .max(1);
    for work in select_global_handle_work(db, moodboard_slug, &now, handle_limit).await? {
        env.queue(REFERENCE_PIPELINE_QUEUE_BINDING)?
            .send(ReferencePipelineMessage::FetchGlobalInstagramProfile {
                moodboard_slug: work.moodboard_slug,
                run_id: run_id.to_string(),
                handle: work.handle,
                discovered_via: work.discovered_via,
                related_depth: work.related_depth,
            })
            .await?;
    }

    enqueue_review_global_visual_candidates(env, moodboard_slug, run_id, 60).await?;
    enqueue_finalize_global_moodboard_library(env, moodboard_slug, run_id, "source_fetch_started")
        .await
}

async fn set_current_global_source_run(
    db: &D1Database,
    moodboard_slug: &str,
    run_id: &str,
    target: u32,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE global_moodboard_reference_state
        SET current_run_id = ?,
            status = 'refreshing',
            target_reference_count = ?,
            next_retry_at = NULL,
            updated_at = ?
        WHERE moodboard_slug = ?
        "#,
        vec![
            json!(run_id),
            json!(target),
            json!(now),
            json!(moodboard_slug),
        ],
    )
    .await
}

async fn global_run_is_current(
    db: &D1Database,
    moodboard_slug: &str,
    run_id: &str,
) -> WorkerResult<bool> {
    let row = db::first::<CurrentRunRow>(
        db,
        r#"
        SELECT current_run_id
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
        LIMIT 1
        "#,
        vec![json!(moodboard_slug)],
    )
    .await?;
    Ok(row
        .and_then(|row| row.current_run_id)
        .map(|current_run_id| current_run_id == run_id)
        .unwrap_or(false))
}

async fn ensure_current_global_run_after_provider_fetch(
    db: &D1Database,
    moodboard_slug: &str,
    run_id: &str,
) -> WorkerResult<bool> {
    global_run_is_current(db, moodboard_slug, run_id).await
}

async fn load_current_global_source_run_for_finalize(
    db: &D1Database,
    moodboard_slug: &str,
    run_id: &str,
) -> WorkerResult<Option<GlobalSourceRunFinalizeRow>> {
    db::first(
        db,
        r#"
        SELECT
          run.error_code,
          run.error_message
        FROM global_moodboard_source_runs run
        INNER JOIN global_moodboard_reference_state state
          ON state.moodboard_slug = run.moodboard_slug
        WHERE run.id = ?
          AND run.moodboard_slug = ?
          AND state.current_run_id = ?
          AND run.status IN ('queued', 'refreshing')
        LIMIT 1
        "#,
        vec![json!(run_id), json!(moodboard_slug), json!(run_id)],
    )
    .await
}

async fn select_global_search_work(
    db: &D1Database,
    moodboard_slug: &str,
    now: &str,
    limit: u32,
) -> WorkerResult<Vec<GlobalSearchWorkRow>> {
    db::all(
        db,
        select_global_search_work_sql(),
        vec![json!(moodboard_slug), json!(now), json!(limit)],
    )
    .await
}

async fn select_global_handle_work(
    db: &D1Database,
    moodboard_slug: &str,
    now: &str,
    limit: u32,
) -> WorkerResult<Vec<GlobalHandleWorkRow>> {
    db::all(
        db,
        select_global_handle_work_sql(),
        vec![json!(moodboard_slug), json!(now), json!(limit)],
    )
    .await
}

fn impacted_global_moodboard_slugs_sql() -> &'static str {
    r#"
    SELECT DISTINCT moodboard_slug
    FROM global_moodboard_references
    WHERE source_run_id = ?
    UNION
    SELECT DISTINCT discovery_moodboard_slug AS moodboard_slug
    FROM global_moodboard_references
    WHERE source_run_id = ?
      AND discovery_moodboard_slug = ?
    UNION
    SELECT ?
    "#
}

fn active_global_reference_count_sql() -> &'static str {
    r#"
    SELECT COUNT(*) AS count
    FROM global_moodboard_references
    WHERE moodboard_slug = ?
      AND status = 'active'
    "#
}

fn retryable_global_candidate_work_sql() -> &'static str {
    r#"
    SELECT COUNT(*) AS count
    FROM global_visual_candidate_discoveries gcd
    JOIN global_visual_reference_candidates gvc ON gvc.id = gcd.candidate_id
    WHERE (
        (gcd.moodboard_slug = ? AND gcd.run_id = ?)
        OR gvc.assigned_moodboard_slug = ?
      )
      AND gvc.candidate_status = 'active'
      AND (
        gvc.review_status = 'queued'
        OR (
          gvc.review_status = 'reviewing'
          AND gvc.review_locked_until IS NOT NULL
          AND gvc.review_locked_until <= ?
          AND gvc.review_attempt_count < ?
        )
        OR (
          gvc.review_status = 'failed'
          AND gvc.review_attempt_count < ?
          AND (gvc.review_next_retry_at IS NULL OR gvc.review_next_retry_at <= ?)
        )
        OR (
          gvc.review_status = 'approved'
          AND gvc.cleanup_status = 'queued'
        )
        OR (
          gvc.review_status = 'approved'
          AND gvc.cleanup_status = 'cleaning'
          AND (gvc.cleanup_next_retry_at IS NULL OR gvc.cleanup_next_retry_at <= ?)
        )
        OR (
          gvc.review_status = 'approved'
          AND gvc.cleanup_status = 'failed'
          AND gvc.cleanup_attempt_count < ?
          AND (gvc.cleanup_next_retry_at IS NULL OR gvc.cleanup_next_retry_at <= ?)
        )
      )
    "#
}

fn in_flight_global_candidate_work_sql() -> &'static str {
    r#"
    SELECT COUNT(DISTINCT gvc.id) AS count
    FROM global_visual_candidate_discoveries gcd
    JOIN global_visual_reference_candidates gvc ON gvc.id = gcd.candidate_id
    WHERE (
        (gcd.moodboard_slug = ? AND gcd.run_id = ?)
        OR gvc.assigned_moodboard_slug = ?
      )
      AND gvc.candidate_status = 'active'
      AND (
        (
          gvc.review_status = 'reviewing'
          AND gvc.review_locked_until IS NOT NULL
          AND gvc.review_locked_until > ?
        )
        OR (
          gvc.review_status = 'approved'
          AND gvc.cleanup_status = 'cleaning'
          AND gvc.cleanup_next_retry_at IS NOT NULL
          AND gvc.cleanup_next_retry_at > ?
        )
      )
    "#
}

fn eligible_global_source_work_sql() -> &'static str {
    r#"
    SELECT COUNT(*) AS count
    FROM (
      SELECT id
      FROM global_moodboard_search_state
      WHERE moodboard_slug = ?
        AND status IN ('active', 'cooldown')
        AND (next_eligible_at IS NULL OR next_eligible_at <= ?)
      UNION ALL
      SELECT id
      FROM global_moodboard_handles
      WHERE moodboard_slug = ?
        AND status IN ('active', 'cooldown')
        AND (cooldown_until IS NULL OR cooldown_until <= ?)
    )
    "#
}

fn earliest_global_retry_at_sql() -> &'static str {
    r#"
    SELECT MIN(next_retry_at) AS next_retry_at
    FROM (
      SELECT next_eligible_at AS next_retry_at
      FROM global_moodboard_search_state
      WHERE moodboard_slug = ?
        AND status IN ('active', 'cooldown')
        AND next_eligible_at IS NOT NULL
        AND next_eligible_at > ?
      UNION ALL
      SELECT cooldown_until AS next_retry_at
      FROM global_moodboard_handles
      WHERE moodboard_slug = ?
        AND status IN ('active', 'cooldown')
        AND cooldown_until IS NOT NULL
        AND cooldown_until > ?
      UNION ALL
      SELECT gvc.review_next_retry_at AS next_retry_at
      FROM global_visual_candidate_discoveries gcd
      JOIN global_visual_reference_candidates gvc ON gvc.id = gcd.candidate_id
      WHERE (
          (gcd.moodboard_slug = ? AND gcd.run_id = ?)
          OR gvc.assigned_moodboard_slug = ?
        )
        AND gvc.candidate_status = 'active'
        AND gvc.review_status = 'failed'
        AND gvc.review_next_retry_at IS NOT NULL
        AND gvc.review_next_retry_at > ?
      UNION ALL
      SELECT gvc.review_locked_until AS next_retry_at
      FROM global_visual_candidate_discoveries gcd
      JOIN global_visual_reference_candidates gvc ON gvc.id = gcd.candidate_id
      WHERE (
          (gcd.moodboard_slug = ? AND gcd.run_id = ?)
          OR gvc.assigned_moodboard_slug = ?
        )
        AND gvc.candidate_status = 'active'
        AND gvc.review_status = 'reviewing'
        AND gvc.review_locked_until IS NOT NULL
        AND gvc.review_locked_until > ?
      UNION ALL
      SELECT gvc.cleanup_next_retry_at AS next_retry_at
      FROM global_visual_candidate_discoveries gcd
      JOIN global_visual_reference_candidates gvc ON gvc.id = gcd.candidate_id
      WHERE (
          (gcd.moodboard_slug = ? AND gcd.run_id = ?)
          OR gvc.assigned_moodboard_slug = ?
        )
        AND gvc.candidate_status = 'active'
        AND gvc.review_status = 'approved'
        AND gvc.cleanup_status IN ('failed', 'cleaning')
        AND gvc.cleanup_next_retry_at IS NOT NULL
        AND gvc.cleanup_next_retry_at > ?
    )
    "#
}

fn update_global_reference_state_after_recount_sql() -> &'static str {
    r#"
    UPDATE global_moodboard_reference_state
    SET active_reference_count = ?,
        status = ?,
        underfilled = ?,
        next_retry_at = ?,
        last_successful_refresh_at = CASE WHEN ? THEN ? ELSE last_successful_refresh_at END,
        last_ready_at = CASE WHEN ? THEN ? ELSE last_ready_at END,
        last_underfilled_at = CASE WHEN ? THEN ? ELSE last_underfilled_at END,
        last_insufficient_at = CASE WHEN ? THEN ? ELSE last_insufficient_at END,
        current_run_id = CASE WHEN moodboard_slug = ? THEN ? ELSE current_run_id END,
        updated_at = ?
    WHERE moodboard_slug = ?
      AND (
        (moodboard_slug = ? AND current_run_id = ?)
        OR (
          moodboard_slug != ?
          AND (
            current_run_id IS NULL
            OR current_run_id = ?
            OR NOT EXISTS (
              SELECT 1
              FROM global_moodboard_source_runs active_run
              WHERE active_run.id = current_run_id
                AND active_run.status IN ('queued', 'refreshing')
            )
          )
        )
      )
    -- assigned slug recount side effect
    "#
}

fn select_global_candidates_for_review_sql() -> &'static str {
    r#"
    SELECT DISTINCT
      gvc.id,
      gvc.platform,
      gvc.source_handle,
      gvc.source_caption,
      gvc.source_published_at,
      gvc.like_count,
      gvc.comment_count,
      gvc.image_url,
      gvc.review_attempt_count
    FROM global_visual_candidate_discoveries gcd
    JOIN global_visual_reference_candidates gvc ON gvc.id = gcd.candidate_id
    WHERE gcd.run_id = ?
      AND gcd.moodboard_slug = ?
      AND gvc.candidate_status = 'active'
      AND (
        gvc.review_status = 'queued'
        OR (
          gvc.review_status = 'failed'
          AND gvc.review_attempt_count < ?
          AND (gvc.review_next_retry_at IS NULL OR gvc.review_next_retry_at <= ?)
        )
        OR (
          gvc.review_status = 'reviewing'
          AND gvc.review_locked_until IS NOT NULL
          AND gvc.review_locked_until <= ?
          AND gvc.review_attempt_count < ?
        )
      )
    ORDER BY
      gvc.review_attempt_count ASC,
      COALESCE(gvc.like_count, 0) DESC,
      COALESCE(gvc.comment_count, 0) DESC,
      gvc.created_at ASC
    LIMIT ?
    "#
}

fn claim_global_candidate_for_review_sql() -> &'static str {
    r#"
    UPDATE global_visual_reference_candidates
    SET review_status = 'reviewing',
        review_run_id = ?,
        review_claim_id = ?,
        review_locked_until = ?,
        review_attempt_count = review_attempt_count + 1,
        updated_at = ?
    WHERE id = ?
      AND candidate_status = 'active'
      AND (
        review_status = 'queued'
        OR (
          review_status = 'failed'
          AND review_attempt_count < ?
          AND (review_next_retry_at IS NULL OR review_next_retry_at <= ?)
        )
        OR (
          review_status = 'reviewing'
          AND review_locked_until IS NOT NULL
          AND review_locked_until <= ?
          AND review_attempt_count < ?
        )
      )
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn mark_global_candidate_review_approved_sql() -> &'static str {
    r#"
    UPDATE global_visual_reference_candidates
    SET review_status = 'approved',
        assigned_moodboard_slug = ?,
        cleanup_status = 'queued',
        review_json = ?,
        review_error_code = NULL,
        review_error_message = NULL,
        review_next_retry_at = NULL,
        review_claim_id = NULL,
        review_locked_until = NULL,
        reviewed_at = ?,
        updated_at = ?
    WHERE id = ?
      AND review_status = 'reviewing'
      AND review_run_id = ?
      AND review_claim_id = ?
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn mark_global_candidate_review_rejected_sql() -> &'static str {
    r#"
    UPDATE global_visual_reference_candidates
    SET review_status = 'rejected',
        cleanup_status = 'not_required',
        review_json = ?,
        review_next_retry_at = NULL,
        review_claim_id = NULL,
        review_locked_until = NULL,
        reviewed_at = ?,
        updated_at = ?
    WHERE id = ?
      AND review_status = 'reviewing'
      AND review_run_id = ?
      AND review_claim_id = ?
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn mark_global_candidate_review_failed_sql() -> &'static str {
    r#"
    UPDATE global_visual_reference_candidates
    SET review_status = 'failed',
        candidate_status = CASE WHEN review_attempt_count >= ? THEN 'review_failed' ELSE candidate_status END,
        review_error_code = ?,
        review_error_message = ?,
        review_next_retry_at = CASE WHEN review_attempt_count >= ? THEN NULL ELSE ? END,
        review_claim_id = NULL,
        review_locked_until = NULL,
        updated_at = ?
    WHERE id = ?
      AND review_status = 'reviewing'
      AND review_run_id = ?
      AND review_claim_id = ?
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn load_global_candidate_for_cleanup_sql() -> &'static str {
    r#"
    SELECT
      id,
      image_url,
      image_width,
      image_height,
      discovery_moodboard_slug,
      assigned_moodboard_slug,
      source_handle,
      source_post_id,
      source_post_code,
      source_url,
      source_published_at,
      review_json,
      cleanup_attempt_count
    FROM global_visual_reference_candidates
    WHERE id = ?
      AND candidate_status = 'active'
      AND review_status = 'approved'
      AND cleanup_status IN ('queued', 'failed', 'cleaning')
      AND assigned_moodboard_slug IS NOT NULL
      AND image_url IS NOT NULL
      AND (cleanup_status != 'cleaning' OR cleanup_next_retry_at <= ?)
      AND (cleanup_status = 'cleaning' OR cleanup_next_retry_at IS NULL OR cleanup_next_retry_at <= ?)
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn claim_global_candidate_for_cleanup_sql() -> &'static str {
    r#"
    UPDATE global_visual_reference_candidates
    SET cleanup_status = 'cleaning',
        cleanup_next_retry_at = ?,
        cleanup_attempt_count = cleanup_attempt_count + 1,
        updated_at = ?
    WHERE id = ?
      AND candidate_status = 'active'
      AND review_status = 'approved'
      AND cleanup_status IN ('queued', 'failed', 'cleaning')
      AND assigned_moodboard_slug IS NOT NULL
      AND cleanup_attempt_count < ?
      AND (cleanup_status != 'cleaning' OR cleanup_next_retry_at <= ?)
      AND (cleanup_status = 'cleaning' OR cleanup_next_retry_at IS NULL OR cleanup_next_retry_at <= ?)
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn load_cleaned_global_candidate_for_followup_sql() -> &'static str {
    r#"
    SELECT
      discovery_moodboard_slug,
      assigned_moodboard_slug,
      source_handle
    FROM global_visual_reference_candidates gvc
    WHERE gvc.id = ?
      AND gvc.candidate_status = 'active'
      AND gvc.review_status = 'approved'
      AND gvc.cleanup_status = 'cleaned'
      AND gvc.assigned_moodboard_slug IS NOT NULL
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_references gmr
        WHERE gmr.candidate_id = gvc.id
          AND gmr.status = 'active'
      )
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn load_cleaned_global_candidate_for_reference_resume_sql() -> &'static str {
    r#"
    SELECT
      cleaned_image_url,
      image_width,
      image_height,
      discovery_moodboard_slug,
      assigned_moodboard_slug,
      source_handle,
      source_post_id,
      source_post_code,
      source_url,
      source_published_at,
      review_json
    FROM global_visual_reference_candidates gvc
    WHERE gvc.id = ?
      AND gvc.candidate_status = 'active'
      AND gvc.review_status = 'approved'
      AND gvc.cleanup_status = 'cleaned'
      AND gvc.cleaned_image_url IS NOT NULL
      AND gvc.assigned_moodboard_slug IS NOT NULL
      AND NOT EXISTS (
        SELECT 1
        FROM global_moodboard_references gmr
        WHERE gmr.candidate_id = gvc.id
          AND gmr.status = 'active'
      )
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn claim_cleaned_global_candidate_for_reference_resume_sql() -> &'static str {
    r#"
    UPDATE global_visual_reference_candidates
    SET cleanup_next_retry_at = ?,
        updated_at = ?
    WHERE id = ?
      AND candidate_status = 'active'
      AND review_status = 'approved'
      AND cleanup_status = 'cleaned'
      AND cleaned_image_url IS NOT NULL
      AND assigned_moodboard_slug IS NOT NULL
      AND NOT EXISTS (
        SELECT 1
        FROM global_moodboard_references gmr
        WHERE gmr.candidate_id = global_visual_reference_candidates.id
          AND gmr.status = 'active'
      )
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn mark_global_candidate_cleanup_failed_sql() -> &'static str {
    r#"
    UPDATE global_visual_reference_candidates
    SET cleanup_status = 'failed',
        -- Exhausted cleanup attempts make candidate_status = 'cleanup_failed'.
        candidate_status = CASE WHEN cleanup_attempt_count >= ? THEN 'cleanup_failed' ELSE candidate_status END,
        cleanup_error_code = ?,
        cleanup_error_message = ?,
        cleanup_next_retry_at = CASE WHEN cleanup_attempt_count >= ? THEN NULL ELSE ? END,
        updated_at = ?
    WHERE id = ?
      AND review_status = 'approved'
      AND cleanup_status IN ('cleaning', 'cleaned')
      AND cleanup_next_retry_at = ?
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn mark_global_candidate_cleanup_succeeded_sql() -> &'static str {
    r#"
    UPDATE global_visual_reference_candidates
    SET cleanup_status = 'cleaned',
        cleanup_error_code = NULL,
        cleanup_error_message = NULL,
        cleaned_image_url = ?,
        cleanup_json = ?,
        cleaned_at = ?,
        updated_at = ?
    WHERE id = ?
      AND candidate_status = 'active'
      AND review_status = 'approved'
      AND cleanup_status = 'cleaning'
      AND cleanup_next_retry_at = ?
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

fn insert_global_moodboard_reference_sql() -> &'static str {
    r#"
    INSERT OR IGNORE INTO global_moodboard_references (
      id,
      candidate_id,
      media_asset_id,
      moodboard_slug,
      discovery_moodboard_slug,
      source_run_id,
      source_platform,
      source_image_key,
      source_handle,
      source_post_id,
      source_post_code,
      source_url,
      source_published_at,
      image_width,
      image_height,
      editorial_composition_score,
      real_pose_angle_score,
      fashion_culture_cue_score,
      lighting_color_direction_score,
      moodboard_fit_score,
      overall_reference_score,
      pose,
      scene,
      lighting,
      framing,
      camera_feel,
      styling_direction,
      color_palette_json,
      fashion_culture_cues_json,
      composition_notes,
      review_json,
      status,
      created_at,
      updated_at
    )
    SELECT
      ?,
      gvc.id,
      ?,
      gvc.assigned_moodboard_slug,
      gvc.discovery_moodboard_slug,
      ?,
      gvc.platform,
      gvc.source_image_key,
      gvc.source_handle,
      gvc.source_post_id,
      gvc.source_post_code,
      gvc.source_url,
      gvc.source_published_at,
      gvc.image_width,
      gvc.image_height,
      ?,
      ?,
      ?,
      ?,
      ?,
      ?,
      ?,
      ?,
      ?,
      ?,
      ?,
      ?,
      ?,
      ?,
      ?,
      gvc.review_json,
      'active',
      ?,
      ?
    FROM global_visual_reference_candidates gvc
    WHERE gvc.id = ?
      AND gvc.review_status = 'approved'
      AND gvc.cleanup_status = 'cleaned'
      AND gvc.assigned_moodboard_slug IS NOT NULL
      AND EXISTS (
        SELECT 1
        FROM global_moodboard_reference_state
        WHERE moodboard_slug = ?
          AND current_run_id = ?
      )
    "#
}

async fn upsert_global_handle(
    db: &D1Database,
    moodboard_slug: &str,
    handle: &str,
    discovered_via: &str,
    related_depth: u8,
    now: &str,
) -> WorkerResult<()> {
    let Some(handle) = normalize_instagram_handle(handle) else {
        return Ok(());
    };
    db::exec(
        db,
        upsert_global_handle_sql(),
        vec![
            json!(deterministic_id(
                "global_handle",
                &[moodboard_slug, &handle]
            )),
            json!(moodboard_slug),
            json!(handle),
            json!(discovered_via),
            json!(related_depth),
            json!(now),
            json!(now),
        ],
    )
    .await
}

async fn mark_global_search_state_seen(
    db: &D1Database,
    moodboard_slug: &str,
    search_term: &str,
    date_window: &str,
    page: u32,
    seen_count: u32,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE global_moodboard_search_state
        SET last_run_at = ?,
            next_eligible_at = NULL,
            seen_result_count = seen_result_count + ?,
            failure_count = 0,
            last_error_code = NULL,
            last_error_message = NULL,
            updated_at = ?
        WHERE moodboard_slug = ?
          AND search_term = ?
          AND date_window = ?
          AND page = ?
        "#,
        vec![
            json!(now),
            json!(seen_count),
            json!(now),
            json!(moodboard_slug),
            json!(search_term),
            json!(date_window),
            json!(page),
        ],
    )
    .await
}

async fn mark_global_handle_fetch_result(
    db: &D1Database,
    moodboard_slug: &str,
    handle: &str,
    next_cursor: Option<String>,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE global_moodboard_handles
        SET last_fetched_at = ?,
            next_cursor = ?,
            fetch_count = fetch_count + 1,
            failure_count = 0,
            cooldown_until = NULL,
            updated_at = ?
        WHERE moodboard_slug = ?
          AND handle = lower(?)
        "#,
        vec![
            json!(now),
            next_cursor.map(Value::String).unwrap_or(Value::Null),
            json!(now),
            json!(moodboard_slug),
            json!(handle),
        ],
    )
    .await
}

async fn record_scrapecreators_search_failure_and_enqueue_finalize(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    search_term: &str,
    date_window: &str,
    page: u32,
    error_code: &str,
    error_message: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let cooldown_until = retry_after_iso(1);
    let detail = compact_error_detail(error_message);
    record_global_search_fetch_failure(
        db,
        moodboard_slug,
        search_term,
        date_window,
        page,
        error_code,
        &detail,
        &cooldown_until,
        &now,
    )
    .await?;
    record_global_source_run_failure(db, run_id, error_code, &detail, &now).await?;
    enqueue_finalize_global_moodboard_library(env, moodboard_slug, run_id, "source_fetch_failed")
        .await
}

async fn record_scrapecreators_handle_failure_and_enqueue_finalize(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    handle: &str,
    error_code: &str,
    error_message: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let cooldown_until = retry_after_iso(1);
    let detail = compact_error_detail(error_message);
    record_global_handle_fetch_failure(db, moodboard_slug, handle, &cooldown_until, &now).await?;
    record_global_source_run_failure(db, run_id, error_code, &detail, &now).await?;
    enqueue_finalize_global_moodboard_library(env, moodboard_slug, run_id, "source_fetch_failed")
        .await
}

async fn record_global_search_fetch_failure(
    db: &D1Database,
    moodboard_slug: &str,
    search_term: &str,
    date_window: &str,
    page: u32,
    error_code: &str,
    error_message: &str,
    next_eligible_at: &str,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE global_moodboard_search_state
        SET status = 'cooldown',
            failure_count = failure_count + 1,
            last_error_code = ?,
            last_error_message = ?,
            next_eligible_at = ?,
            updated_at = ?
        WHERE moodboard_slug = ?
          AND search_term = ?
          AND date_window = ?
          AND page = ?
        "#,
        vec![
            json!(error_code),
            json!(error_message),
            json!(next_eligible_at),
            json!(now),
            json!(moodboard_slug),
            json!(search_term),
            json!(date_window),
            json!(page),
        ],
    )
    .await
}

async fn record_global_handle_fetch_failure(
    db: &D1Database,
    moodboard_slug: &str,
    handle: &str,
    cooldown_until: &str,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE global_moodboard_handles
        SET failure_count = failure_count + 1,
            cooldown_until = ?,
            updated_at = ?
        WHERE moodboard_slug = ?
          AND handle = lower(?)
        "#,
        vec![
            json!(cooldown_until),
            json!(now),
            json!(moodboard_slug),
            json!(handle),
        ],
    )
    .await
}

async fn record_global_source_run_failure(
    db: &D1Database,
    run_id: &str,
    error_code: &str,
    error_message: &str,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE global_moodboard_source_runs
        SET error_code = ?,
            error_message = ?,
            updated_at = ?
        WHERE id = ?
        "#,
        vec![
            json!(error_code),
            json!(error_message),
            json!(now),
            json!(run_id),
        ],
    )
    .await
}

async fn upsert_global_candidates_and_audit(
    db: &D1Database,
    moodboard_slug: &str,
    run_id: &str,
    source_key: &str,
    candidates: &[InstagramImageCandidate],
) -> WorkerResult<()> {
    let now = now_iso_string();
    for candidate in candidates {
        let source_image_key = instagram_source_image_key(candidate);
        let candidate_id = deterministic_id("global_candidate", &[&source_image_key]);
        db::exec(
            db,
            upsert_global_candidate_sql(),
            vec![
                json!(candidate_id),
                json!("instagram"),
                json!(source_image_key),
                json!(candidate.source_handle),
                candidate
                    .source_profile_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                json!(candidate.source_post_id),
                json!(candidate.source_post_code),
                candidate
                    .source_url
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                candidate
                    .source_published_at
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                Value::Null,
                json!(candidate.media_type),
                json!(candidate.image_url),
                candidate
                    .image_width
                    .map(|value| json!(value))
                    .unwrap_or(Value::Null),
                candidate
                    .image_height
                    .map(|value| json!(value))
                    .unwrap_or(Value::Null),
                candidate
                    .like_count
                    .map(|value| json!(value))
                    .unwrap_or(Value::Null),
                candidate
                    .comment_count
                    .map(|value| json!(value))
                    .unwrap_or(Value::Null),
                candidate
                    .play_count
                    .map(|value| json!(value))
                    .unwrap_or(Value::Null),
                json!(moodboard_slug),
                json!(candidate.discovered_via),
                json!(run_id),
                json!(run_id),
                json!({
                    "sourceImageIndex": candidate.source_image_index,
                }),
                json!(candidate.raw_json),
                json!(now),
                json!(now),
            ],
        )
        .await?;
        db::exec(
            db,
            audit_global_candidate_discovery_sql(),
            vec![
                json!(deterministic_id(
                    "global_discovery",
                    &[run_id, moodboard_slug, source_key, &candidate_id],
                )),
                json!(candidate_id),
                json!(run_id),
                json!(moodboard_slug),
                json!(source_key),
                candidate
                    .source_url
                    .clone()
                    .or_else(|| Some(candidate.source_post_id.clone()))
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                json!(candidate.discovered_via),
                json!(candidate.source_handle),
                json!(now),
            ],
        )
        .await?;
    }
    Ok(())
}

async fn increment_global_source_run_count(
    db: &D1Database,
    run_id: &str,
    column: &str,
    amount: u32,
) -> WorkerResult<()> {
    let column = match column {
        "candidate_count" => "candidate_count",
        "discovered_handle_count" => "discovered_handle_count",
        _ => {
            return Err(Error::RustError(
                "invalid_global_source_run_count".to_string(),
            ))
        }
    };
    let sql = format!(
        "UPDATE global_moodboard_source_runs SET {column} = {column} + ?, updated_at = ? WHERE id = ?"
    );
    db::exec(
        db,
        &sql,
        vec![json!(amount), json!(now_iso_string()), json!(run_id)],
    )
    .await
}

async fn active_global_reference_count(db: &D1Database, moodboard_slug: &str) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        active_global_reference_count_sql(),
        vec![json!(moodboard_slug)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn impacted_global_moodboard_slugs(
    db: &D1Database,
    moodboard_slug: &str,
    run_id: &str,
) -> WorkerResult<Vec<String>> {
    let rows = db::all::<MoodboardSlugRow>(
        db,
        impacted_global_moodboard_slugs_sql(),
        vec![
            json!(run_id),
            json!(run_id),
            json!(moodboard_slug),
            json!(moodboard_slug),
        ],
    )
    .await?;
    let mut seen = HashSet::new();
    let mut slugs = rows
        .into_iter()
        .filter_map(|row| {
            let slug = row.moodboard_slug.trim().to_string();
            (!slug.is_empty() && seen.insert(slug.clone())).then_some(slug)
        })
        .collect::<Vec<_>>();
    if !seen.contains(moodboard_slug) {
        slugs.push(moodboard_slug.to_string());
    }
    Ok(slugs)
}

async fn retryable_global_candidate_work_count(
    db: &D1Database,
    source_moodboard_slug: &str,
    run_id: &str,
    assigned_moodboard_slug: &str,
    review_retry_limit: u32,
    cleanup_retry_limit: u32,
    now: &str,
) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        retryable_global_candidate_work_sql(),
        vec![
            json!(source_moodboard_slug),
            json!(run_id),
            json!(assigned_moodboard_slug),
            json!(now),
            json!(review_retry_limit),
            json!(review_retry_limit),
            json!(now),
            json!(now),
            json!(cleanup_retry_limit),
            json!(now),
        ],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn in_flight_global_candidate_work_count(
    db: &D1Database,
    source_moodboard_slug: &str,
    run_id: &str,
    assigned_moodboard_slug: &str,
    now: &str,
) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        in_flight_global_candidate_work_sql(),
        vec![
            json!(source_moodboard_slug),
            json!(run_id),
            json!(assigned_moodboard_slug),
            json!(now),
            json!(now),
        ],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn eligible_global_source_work_count(
    db: &D1Database,
    moodboard_slug: &str,
    now: &str,
) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        eligible_global_source_work_sql(),
        vec![
            json!(moodboard_slug),
            json!(now),
            json!(moodboard_slug),
            json!(now),
        ],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn earliest_global_retry_at(
    db: &D1Database,
    source_moodboard_slug: &str,
    run_id: &str,
    assigned_moodboard_slug: &str,
) -> WorkerResult<Option<String>> {
    let now = now_iso_string();
    let row = db::first::<NextRetryAtRow>(
        db,
        earliest_global_retry_at_sql(),
        vec![
            json!(assigned_moodboard_slug),
            json!(now),
            json!(assigned_moodboard_slug),
            json!(now),
            json!(source_moodboard_slug),
            json!(run_id),
            json!(assigned_moodboard_slug),
            json!(now),
            json!(source_moodboard_slug),
            json!(run_id),
            json!(assigned_moodboard_slug),
            json!(now),
            json!(source_moodboard_slug),
            json!(run_id),
            json!(assigned_moodboard_slug),
            json!(now),
        ],
    )
    .await?;
    Ok(row.and_then(|row| row.next_retry_at))
}

#[allow(clippy::too_many_arguments)]
async fn update_global_reference_state_after_recount(
    db: &D1Database,
    source_moodboard_slug: &str,
    run_id: &str,
    moodboard_slug: &str,
    active_count: u32,
    status: &str,
    underfilled: u8,
    next_retry_at: Option<&str>,
    set_last_successful_refresh_at: bool,
    set_last_ready_at: bool,
    set_last_underfilled_at: bool,
    set_last_insufficient_at: bool,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        update_global_reference_state_after_recount_sql(),
        vec![
            json!(active_count),
            json!(status),
            json!(underfilled),
            next_retry_at
                .map(|value| Value::String(value.to_string()))
                .unwrap_or(Value::Null),
            json!(set_last_successful_refresh_at as u8),
            json!(now),
            json!(set_last_ready_at as u8),
            json!(now),
            json!(set_last_underfilled_at as u8),
            json!(now),
            json!(set_last_insufficient_at as u8),
            json!(now),
            json!(source_moodboard_slug),
            json!(run_id),
            json!(now),
            json!(moodboard_slug),
            json!(source_moodboard_slug),
            json!(run_id),
            json!(source_moodboard_slug),
            json!(run_id),
        ],
    )
    .await
}

async fn complete_global_source_run_after_recount(
    db: &D1Database,
    moodboard_slug: &str,
    run_id: &str,
    status: &str,
    reason: &str,
    error_code: Option<&str>,
    error_message: Option<&str>,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE global_moodboard_source_runs
        SET status = ?,
            reason = ?,
            error_code = ?,
            error_message = ?,
            completed_at = ?,
            updated_at = ?
        WHERE id = ?
          AND status IN ('queued', 'refreshing')
          AND EXISTS (
            SELECT 1
            FROM global_moodboard_reference_state
            WHERE moodboard_slug = ?
              AND current_run_id = ?
          )
        "#,
        vec![
            json!(status),
            json!(reason),
            error_code
                .map(|value| Value::String(value.to_string()))
                .unwrap_or(Value::Null),
            error_message
                .map(|value| Value::String(value.to_string()))
                .unwrap_or(Value::Null),
            json!(now),
            json!(now),
            json!(run_id),
            json!(moodboard_slug),
            json!(run_id),
        ],
    )
    .await
}

async fn global_review_has_eligible_rows(
    db: &D1Database,
    moodboard_slug: &str,
    run_id: &str,
    review_retry_limit: u32,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let rows = db::all::<GlobalVisualCandidateReviewRow>(
        db,
        select_global_candidates_for_review_sql(),
        vec![
            json!(run_id),
            json!(moodboard_slug),
            json!(review_retry_limit),
            json!(now),
            json!(now),
            json!(review_retry_limit),
            json!(1),
        ],
    )
    .await?;
    Ok(!rows.is_empty())
}

async fn config_value_u32(db: &D1Database, key: &str, default: u32) -> WorkerResult<u32> {
    let row = db::first::<ConfigValueRow>(
        db,
        "SELECT value FROM blitz_config WHERE key = ?",
        vec![json!(key)],
    )
    .await?;
    Ok(row
        .and_then(|row| row.value.trim().parse::<u32>().ok())
        .unwrap_or(default))
}

fn selected_search_terms(
    search_queries_json: &str,
    fallback_title: &str,
    limit: usize,
) -> Vec<String> {
    let mut terms = serde_json::from_str::<Vec<String>>(search_queries_json)
        .unwrap_or_default()
        .into_iter()
        .map(|term| term.trim().to_string())
        .filter(|term| !term.is_empty())
        .take(limit.max(1))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        terms.push(format!("{} creator aesthetic", fallback_title.trim()));
    }
    terms
}

async fn enqueue_review_global_visual_candidates(
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    limit: u32,
) -> WorkerResult<()> {
    env.queue(REFERENCE_PIPELINE_QUEUE_BINDING)?
        .send(ReferencePipelineMessage::ReviewGlobalVisualCandidates {
            moodboard_slug: moodboard_slug.to_string(),
            run_id: run_id.to_string(),
            limit,
        })
        .await
}

async fn enqueue_finalize_global_moodboard_library(
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    reason: &str,
) -> WorkerResult<()> {
    env.queue(REFERENCE_PIPELINE_QUEUE_BINDING)?
        .send(ReferencePipelineMessage::FinalizeGlobalMoodboardLibrary {
            moodboard_slug: moodboard_slug.to_string(),
            run_id: run_id.to_string(),
            reason: reason.to_string(),
        })
        .await
}

async fn record_global_candidate_cleanup_failure_and_finalize(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    candidate_id: &str,
    retry_limit: u32,
    cleanup_claim_expires_at: &str,
    error_code: &str,
    error_message: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let next_retry_at = retry_after_minutes(15);
    db::exec(
        db,
        mark_global_candidate_cleanup_failed_sql(),
        vec![
            json!(retry_limit),
            json!(error_code),
            json!(compact_error_detail(error_message)),
            json!(retry_limit),
            json!(next_retry_at),
            json!(now),
            json!(candidate_id),
            json!(cleanup_claim_expires_at),
            json!(moodboard_slug),
            json!(run_id),
        ],
    )
    .await?;
    enqueue_finalize_global_moodboard_library(env, moodboard_slug, run_id, "global_cleanup_failed")
        .await
}

async fn resume_or_finalize_cleaned_global_candidate(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    candidate_id: &str,
    retry_limit: u32,
) -> WorkerResult<bool> {
    if let Some(candidate) = db::first::<GlobalVisualCandidateReferenceRow>(
        db,
        load_cleaned_global_candidate_for_reference_resume_sql(),
        vec![json!(candidate_id), json!(moodboard_slug), json!(run_id)],
    )
    .await?
    {
        let cleanup_lock_expires_at = retry_after_minutes(15);
        let claim_result = db::run(
            db,
            claim_cleaned_global_candidate_for_reference_resume_sql(),
            vec![
                json!(cleanup_lock_expires_at),
                json!(now_iso_string()),
                json!(candidate_id),
                json!(moodboard_slug),
                json!(run_id),
            ],
        )
        .await?;
        if changed_rows(&claim_result)? > 0 {
            complete_cleaned_global_moodboard_reference(
                db,
                env,
                moodboard_slug,
                run_id,
                candidate_id,
                retry_limit,
                &cleanup_lock_expires_at,
                &candidate,
                None,
            )
            .await?;
            return Ok(true);
        }
    }

    if let Some(candidate) = db::first::<GlobalVisualCandidateFollowupRow>(
        db,
        load_cleaned_global_candidate_for_followup_sql(),
        vec![json!(candidate_id), json!(moodboard_slug), json!(run_id)],
    )
    .await?
    {
        ensure_global_cleanup_followups(db, env, moodboard_slug, run_id, &candidate).await?;
        return Ok(true);
    }

    Ok(false)
}

async fn complete_cleaned_global_moodboard_reference(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    candidate_id: &str,
    retry_limit: u32,
    cleanup_claim_expires_at: &str,
    candidate: &GlobalVisualCandidateReferenceRow,
    review: Option<GlobalVisualReferenceReview>,
) -> WorkerResult<()> {
    let review = match review {
        Some(review) => review,
        None => match serde_json::from_str::<GlobalVisualReferenceReview>(&candidate.review_json) {
            Ok(review) => review,
            Err(error) => {
                record_global_candidate_cleanup_failure_and_finalize(
                    db,
                    env,
                    moodboard_slug,
                    run_id,
                    candidate_id,
                    retry_limit,
                    cleanup_claim_expires_at,
                    "global_review_json_malformed",
                    &error.to_string(),
                )
                .await?;
                return Ok(());
            }
        },
    };

    if !global_run_is_current(db, moodboard_slug, run_id).await? {
        return Ok(());
    }
    let global_reference_id = deterministic_global_moodboard_reference_id(candidate_id);
    let cached =
        match crate::services::visual_reference_cache::cache_cleaned_global_moodboard_reference(
            db,
            env,
            &candidate.assigned_moodboard_slug,
            &global_reference_id,
            &candidate.cleaned_image_url,
            candidate.image_width,
            candidate.image_height,
        )
        .await
        {
            Ok(cached) => cached,
            Err(error) => {
                record_global_candidate_cleanup_failure_and_finalize(
                    db,
                    env,
                    moodboard_slug,
                    run_id,
                    candidate_id,
                    retry_limit,
                    cleanup_claim_expires_at,
                    "global_reference_cache_failed",
                    &error.to_string(),
                )
                .await?;
                return Ok(());
            }
        };
    if !global_run_is_current(db, moodboard_slug, run_id).await? {
        return Ok(());
    }

    let insert_result = db::run(
        db,
        insert_global_moodboard_reference_sql(),
        vec![
            json!(global_reference_id),
            json!(cached.media_asset_id),
            json!(run_id),
            json!(review.editorial_composition_score),
            json!(review.real_pose_angle_score),
            json!(review.fashion_culture_cue_score),
            json!(review.lighting_color_direction_score),
            json!(review.moodboard_fit_score),
            json!(review.overall_reference_score),
            json!(empty_string_as_null(&review.pose)),
            json!(empty_string_as_null(&review.scene)),
            json!(empty_string_as_null(&review.lighting)),
            json!(empty_string_as_null(&review.framing)),
            json!(empty_string_as_null(&review.camera_feel)),
            json!(empty_string_as_null(&review.styling_direction)),
            json!(
                serde_json::to_string(&review.color_palette).unwrap_or_else(|_| "[]".to_string())
            ),
            json!(serde_json::to_string(&review.fashion_culture_cues)
                .unwrap_or_else(|_| "[]".to_string())),
            json!(empty_string_as_null(&review.composition_notes)),
            json!(now_iso_string()),
            json!(now_iso_string()),
            json!(candidate_id),
            json!(moodboard_slug),
            json!(run_id),
        ],
    )
    .await;
    if let Err(error) = insert_result {
        record_global_candidate_cleanup_failure_and_finalize(
            db,
            env,
            moodboard_slug,
            run_id,
            candidate_id,
            retry_limit,
            cleanup_claim_expires_at,
            "global_reference_insert_failed",
            &error.to_string(),
        )
        .await?;
        return Ok(());
    }

    ensure_global_cleanup_followups(
        db,
        env,
        moodboard_slug,
        run_id,
        &GlobalVisualCandidateFollowupRow {
            discovery_moodboard_slug: candidate.discovery_moodboard_slug.clone(),
            assigned_moodboard_slug: candidate.assigned_moodboard_slug.clone(),
            source_handle: candidate.source_handle.clone(),
        },
    )
    .await
}

async fn ensure_global_cleanup_followups(
    db: &D1Database,
    env: &Env,
    moodboard_slug: &str,
    run_id: &str,
    candidate: &GlobalVisualCandidateFollowupRow,
) -> WorkerResult<()> {
    if candidate.assigned_moodboard_slug != candidate.discovery_moodboard_slug {
        if let Some(handle) = candidate.source_handle.as_deref() {
            if let Err(error) = upsert_global_handle(
                db,
                &candidate.assigned_moodboard_slug,
                handle,
                "cross_routed_acceptance",
                0,
                &now_iso_string(),
            )
            .await
            {
                web_sys::console::error_1(
                    &format!("global cleanup cross-routed handle upsert failed: {error}").into(),
                );
            }
        }
    }

    enqueue_finalize_global_moodboard_library(
        env,
        moodboard_slug,
        run_id,
        "global_cleanup_already_cleaned",
    )
    .await
}

async fn prepare_global_seedream_cleanup_auth(env: &Env) -> WorkerResult<SeedreamCleanupAuth> {
    let token = provider_account_access_token(
        env,
        HIGGSFIELD_PROVIDER_ACCOUNT_ID,
        HIGGSFIELD_REFRESH_SECRET_NAME,
    )
    .await
    .map_err(|error| Error::RustError(format!("seedream_cleanup_auth_failed:{error}")))?;
    let tool_name = env
        .var(HIGGSFIELD_CLEANUP_TOOL_VAR)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "generate_image".to_string());
    let model = env
        .var(HIGGSFIELD_CLEANUP_MODEL_VAR)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| SEEDREAM_CLEANUP_MODEL.to_string());
    Ok(SeedreamCleanupAuth {
        access_token: token.access_token,
        tool_name,
        model,
    })
}

async fn fetch_global_seedream_cleanup_image(
    image_url: &str,
) -> WorkerResult<SeedreamCleanupImage> {
    let (bytes, content_type) =
        crate::services::visual_reference_cache::fetch_visual_reference_image(image_url).await?;
    Ok(SeedreamCleanupImage {
        bytes,
        content_type,
    })
}

async fn upload_global_seedream_cleanup_image(
    auth: &SeedreamCleanupAuth,
    candidate_id: &str,
    image: SeedreamCleanupImage,
) -> WorkerResult<SeedreamCleanupRequest> {
    let uploaded = upload_media_files(
        &auth.access_token,
        &[HiggsfieldMcpMediaFile {
            filename: format!("{candidate_id}.{}", cleanup_extension(&image.content_type)),
            content_type: image.content_type,
            bytes: image.bytes,
        }],
    )
    .await
    .map_err(|error| Error::RustError(format!("seedream_cleanup_upload_failed:{error}")))?;
    let Some(reference) = uploaded.first() else {
        return Err(Error::RustError(
            "seedream_cleanup_upload_missing".to_string(),
        ));
    };
    Ok(SeedreamCleanupRequest {
        access_token: auth.access_token.clone(),
        reference_value: reference.reference_value.clone(),
        tool_name: auth.tool_name.clone(),
        model: auth.model.clone(),
    })
}

async fn call_global_seedream_cleanup(
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

fn deterministic_global_moodboard_reference_id(candidate_id: &str) -> String {
    deterministic_id("global_reference", &[candidate_id])
}

fn empty_string_as_null(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn env_var(env: &Env, key: &str, error_code: &str) -> WorkerResult<String> {
    match env.var(key) {
        Ok(value) if !value.to_string().trim().is_empty() => Ok(value.to_string()),
        _ => Err(Error::RustError(error_code.to_string())),
    }
}

fn retry_after_iso(hours: u32) -> String {
    let date = js_sys::Date::new_0();
    date.set_time(date.get_time() + (hours as f64 * 60.0 * 60.0 * 1000.0));
    date.to_iso_string().into()
}

fn retry_after_minutes(minutes: u32) -> String {
    let date = js_sys::Date::new_0();
    date.set_time(date.get_time() + (minutes as f64 * 60.0 * 1000.0));
    date.to_iso_string().into()
}

fn queue_error_code(error: &str) -> &'static str {
    if crate::ai::workers_ai::is_workers_ai_upstream_timeout(&error.to_ascii_lowercase()) {
        "ai_upstream_timeout"
    } else {
        "global_visual_review_failed"
    }
}

fn changed_rows(result: &worker::D1Result) -> WorkerResult<usize> {
    Ok(result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or_default())
}

fn compact_error_detail(error: &str) -> String {
    const MAX_DETAIL_LENGTH: usize = 240;
    error
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_DETAIL_LENGTH)
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

fn next_max_id_from(raw: &Value) -> Option<String> {
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

    let mut candidate_counts_by_url = HashMap::<String, usize>::new();
    for candidate in normalized_candidates {
        if let Some(source_url) = candidate.source_url.as_deref() {
            *candidate_counts_by_url
                .entry(source_url.to_ascii_lowercase())
                .or_default() += 1;
        }
    }

    let mut seen = HashSet::new();
    array_at(raw, &["items"])
        .or_else(|| array_at(raw, &["data", "items"]))
        .or_else(|| array_at(raw, &["data"]))
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let source_url = text_at_any(item, &[&["url"], &["permalink"]])?;
            if !source_url.contains("instagram.com/p/") {
                return None;
            }
            if !seen.insert(source_url.to_ascii_lowercase()) {
                return None;
            }
            let existing_count = candidate_counts_by_url
                .get(&source_url.to_ascii_lowercase())
                .copied()
                .unwrap_or_default();
            (existing_count < images_per_post).then_some(InstagramPostDetailTarget { source_url })
        })
        .take(limit)
        .collect()
}

fn array_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_array()
}

fn text_at_any(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| {
        let mut current = value;
        for segment in *path {
            current = current.get(*segment)?;
        }
        current
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn deterministic_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.len().to_string().as_bytes());
        hasher.update(b"\0");
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    let digest = hasher.finalize();
    format!("{prefix}_{}", hex::encode(&digest[..16]))
}

fn new_global_run_id() -> String {
    format!("global_run_{}", uuid::Uuid::new_v4().simple())
}

fn new_global_review_claim_id() -> String {
    format!("global_review_claim_{}", uuid::Uuid::new_v4().simple())
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

#[derive(Debug, Deserialize)]
struct GlobalMoodboardDefinitionRow {
    slug: String,
    title: String,
    vibe_summary: String,
    search_queries_json: String,
}

#[derive(Debug, Deserialize)]
struct GlobalReferenceStateRetryRow {
    next_retry_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReusableGlobalSourceRunRow {
    id: String,
}

#[derive(Debug, Deserialize)]
struct CurrentRunRow {
    current_run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GlobalSourceRunFinalizeRow {
    error_code: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MoodboardSlugRow {
    moodboard_slug: String,
}

#[derive(Debug, Deserialize)]
struct GlobalSearchWorkRow {
    moodboard_slug: String,
    search_term: String,
    date_window: String,
    page: u32,
}

#[derive(Debug, Deserialize)]
struct GlobalHandleWorkRow {
    moodboard_slug: String,
    handle: String,
    discovered_via: String,
    related_depth: u8,
}

#[derive(Debug, Deserialize)]
struct GlobalVisualCandidateReviewRow {
    id: String,
    platform: String,
    source_handle: Option<String>,
    source_caption: Option<String>,
    source_published_at: Option<String>,
    like_count: Option<u64>,
    comment_count: Option<u64>,
    image_url: String,
    review_attempt_count: u32,
}

#[derive(Debug, Deserialize)]
struct GlobalVisualCandidateCleanupRow {
    image_url: String,
    image_width: Option<u32>,
    image_height: Option<u32>,
    discovery_moodboard_slug: String,
    assigned_moodboard_slug: String,
    source_handle: Option<String>,
    review_json: String,
}

#[derive(Debug, Deserialize)]
struct GlobalVisualCandidateReferenceRow {
    cleaned_image_url: String,
    image_width: Option<u32>,
    image_height: Option<u32>,
    discovery_moodboard_slug: String,
    assigned_moodboard_slug: String,
    source_handle: Option<String>,
    review_json: String,
}

#[derive(Debug, Deserialize)]
struct GlobalVisualCandidateFollowupRow {
    discovery_moodboard_slug: String,
    assigned_moodboard_slug: String,
    source_handle: Option<String>,
}

#[derive(Debug)]
struct SeedreamCleanupAuth {
    access_token: String,
    tool_name: String,
    model: String,
}

#[derive(Debug)]
struct SeedreamCleanupImage {
    bytes: Vec<u8>,
    content_type: String,
}

#[derive(Debug)]
struct SeedreamCleanupRequest {
    access_token: String,
    reference_value: String,
    tool_name: String,
    model: String,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u32,
}

#[derive(Debug, Deserialize)]
struct NextRetryAtRow {
    next_retry_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConfigValueRow {
    value: String,
}
