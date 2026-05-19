use crate::db;
use crate::ai::workers_ai::{global_visual_reference_review_prompt, run_vision_json};
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
        ReferencePipelineMessage::CleanupGlobalMoodboardReference { .. }
        | ReferencePipelineMessage::FinalizeGlobalMoodboardLibrary { .. } => {
            global_review_cleanup_and_finalize_are_enabled_in_tasks_11_to_13();
            Ok(())
        }
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
    let review_retry_limit =
        config_value_u32(db, "visual_reference_review_retry_limit", 2)
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

fn global_review_cleanup_and_finalize_are_enabled_in_tasks_11_to_13() {}

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

async fn load_active_global_moodboard_briefs(
    db: &D1Database,
) -> WorkerResult<Vec<MoodboardBrief>> {
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
        r#"
        SELECT COUNT(*) AS count
        FROM global_moodboard_references
        WHERE moodboard_slug = ?
          AND status = 'active'
        "#,
        vec![json!(moodboard_slug)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
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
struct CountRow {
    count: u32,
}

#[derive(Debug, Deserialize)]
struct ConfigValueRow {
    value: String,
}
