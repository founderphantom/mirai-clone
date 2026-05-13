use crate::db;
use crate::providers::higgsfield_auth::provider_account_access_token;
use crate::providers::higgsfield_mcp::{
    call_tool, upload_media_files, HiggsfieldMcpError, HiggsfieldMcpMediaFile,
};
use crate::queues::messages::GenerationMessage;
use crate::services::generation_usage::{
    current_utc_date, load_generation_limits, refund_image_for_date, reserve_image_for_date,
};
use crate::services::media::media_storage_key;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use worker::{
    D1Database, Env, Error, Fetch, HttpMetadata, MessageBatch, MessageBuilder, MessageExt, Method,
    Request, Result as WorkerResult,
};

const HIGGSFIELD_REFRESH_SECRET_NAME: &str = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER";
const HIGGSFIELD_PROVIDER_ACCOUNT_ID: &str = "pa_higgsfield_founder";
const HIGGSFIELD_GENERATION_TOOL_VAR: &str = "HIGGSFIELD_MCP_GENERATION_TOOL";
const HIGGSFIELD_JOB_STATUS_TOOL: &str = "job_status";
const GENERATION_POLL_DELAY_SECONDS: u32 = 10;
const MAX_GENERATED_IMAGE_BYTES: usize = 15 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct ClonePlanRow {
    plan: String,
}

#[derive(Debug, Deserialize)]
struct VisualReferenceRow {
    media_asset_id: Option<String>,
    storage_key: Option<String>,
    content_type: Option<String>,
    materialized_reference_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GenerationJobRow {
    id: String,
    user_id: String,
    clone_id: String,
    blitz_batch_id: Option<String>,
    input_visual_reference_id: Option<String>,
    status: String,
    provider_account_id: Option<String>,
    provider_job_ids_json: String,
    request_json: String,
    response_json: String,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u32,
}

#[derive(Debug, Deserialize)]
struct BatchCompletionRow {
    batch_size: u32,
    generation_count: u32,
}

#[derive(Debug, Deserialize)]
struct TerminalJobsRow {
    total: u32,
    terminal: u32,
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
                web_sys::console::error_1(
                    &format!("generation queue message failed: {error:?}").into(),
                );
                raw_message.retry();
            }
        }
    }

    Ok(())
}

async fn generate_blitz_batch(
    db: &D1Database,
    env: &Env,
    batch_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    visual_reference_ids: &[String],
    provider_soul_id: &str,
) -> WorkerResult<()> {
    let Some(clone) = load_ready_clone_plan(db, clone_id, user_id).await? else {
        mark_batch_failed_without_jobs(db, batch_id).await?;
        return Ok(());
    };
    let generation_limits = load_generation_limits(db).await?;

    for visual_reference_id in visual_reference_ids {
        if let Some(existing) =
            load_generation_job_by_batch_reference(db, batch_id, visual_reference_id).await?
        {
            resume_existing_generation_job(db, env, &existing).await?;
            continue;
        }

        let Some(reference) =
            load_visual_reference(db, clone_id, user_id, visual_reference_id).await?
        else {
            continue;
        };
        let materialized_reference_url = reference
            .materialized_reference_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let storage_key = reference
            .storage_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if materialized_reference_url.is_none() && storage_key.is_none() {
            continue;
        }

        let job_id = deterministic_generation_job_id(batch_id, visual_reference_id);
        let usage_date = current_utc_date();
        let request_json = json!({
            "jobId": job_id,
            "batchId": batch_id,
            "cloneId": clone_id,
            "userId": user_id,
            "idempotencyKey": format!("{idempotency_key}:{visual_reference_id}"),
            "providerSoulId": provider_soul_id,
            "inputImageUrl": materialized_reference_url,
            "inputMediaAssetId": reference.media_asset_id.clone(),
            "inputStorageKey": reference.storage_key.clone(),
            "inputContentType": reference.content_type.clone(),
            "visualReferenceId": visual_reference_id,
            "usageDate": usage_date,
            "prompt": "",
        });
        if !insert_generation_job(
            db,
            &job_id,
            user_id,
            clone_id,
            batch_id,
            visual_reference_id,
            &request_json,
        )
        .await?
        {
            if let Some(existing) = load_generation_job_by_id(db, &job_id).await? {
                resume_existing_generation_job(db, env, &existing).await?;
            }
            continue;
        }

        if !reserve_image_for_date(
            db,
            user_id,
            &clone.plan,
            generation_limits.free_daily_limit,
            generation_limits.pro_daily_limit,
            &usage_date,
        )
        .await?
        {
            fail_generation_job_without_refund(
                db,
                &job_id,
                "daily_generation_limit_reached",
                "Daily generation limit was reached before provider submission.",
            )
            .await?;
            continue;
        }
        persist_generation_usage_marker_or_refund(db, &job_id, user_id, &usage_date).await?;

        if let Err(error) = submit_generation_job(
            db,
            env,
            &job_id,
            batch_id,
            clone_id,
            user_id,
            idempotency_key,
            visual_reference_id,
            provider_soul_id,
            materialized_reference_url,
        )
        .await
        {
            if is_permanent_submission_error(&error) {
                fail_generation_job(
                    db,
                    &job_id,
                    "provider_submission_failed",
                    &error.to_string(),
                )
                .await?;
            } else {
                return Err(error);
            }
        }
    }

    if !batch_has_generation_jobs(db, batch_id).await? {
        mark_batch_failed_without_jobs(db, batch_id).await?;
        return Ok(());
    }

    mark_batch_ready_if_complete(db, batch_id).await
}

#[allow(clippy::too_many_arguments)]
async fn submit_generation_job(
    db: &D1Database,
    env: &Env,
    job_id: &str,
    batch_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    visual_reference_id: &str,
    provider_soul_id: &str,
    materialized_reference_url: Option<&str>,
) -> WorkerResult<()> {
    let Some(job) = load_generation_job_by_id(db, job_id).await? else {
        return Ok(());
    };
    if !ensure_generation_usage_reserved(db, &job).await? {
        return Ok(());
    }

    let tool_name = generation_tool_name(env)?;
    let token = match provider_account_access_token(
        env,
        HIGGSFIELD_PROVIDER_ACCOUNT_ID,
        HIGGSFIELD_REFRESH_SECRET_NAME,
    )
    .await
    {
        Ok(token) => token,
        Err(error) => {
            schedule_submission_retry(
                db,
                env,
                job_id,
                batch_id,
                "provider_submission_auth_retry",
                &error.sanitized_message(),
            )
            .await?;
            return Ok(());
        }
    };

    if !mark_generation_job_submitting(db, job_id, HIGGSFIELD_PROVIDER_ACCOUNT_ID).await? {
        return Ok(());
    }

    let request = serde_json::from_str::<Value>(&job.request_json).unwrap_or_else(|_| {
        json!({
            "jobId": job_id,
            "batchId": batch_id,
            "cloneId": clone_id,
            "userId": user_id,
            "idempotencyKey": format!("{idempotency_key}:{visual_reference_id}"),
            "providerSoulId": provider_soul_id,
            "inputImageUrl": materialized_reference_url,
            "visualReferenceId": visual_reference_id,
            "prompt": "",
        })
    });
    let result = match submit_generation_to_provider(
        env,
        &token.access_token,
        json!(job_id),
        &tool_name,
        job_id,
        &request,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            schedule_submission_retry(
                db,
                env,
                job_id,
                batch_id,
                "provider_submission_retry",
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    };

    if let Some(final_url) = final_image_url(&result.raw_json) {
        if let Err(error) =
            complete_generation_job(db, env, job_id, &final_url, &result.raw_json).await
        {
            web_sys::console::error_1(
                &format!("generation completion scheduled for retry: {error:?}").into(),
            );
            enqueue_completion_retry(env, job_id, batch_id, 1, 30).await?;
        }
        return Ok(());
    }

    record_provider_generation_response(db, job_id, &result.raw_json).await?;
    enqueue_poll(env, job_id, batch_id, 1, 30).await
}

#[allow(clippy::too_many_arguments)]
async fn retry_provider_submission_from_poll(
    db: &D1Database,
    env: &Env,
    job_id: &str,
    batch_id: &str,
    attempt: u8,
    max_attempts: u8,
    tool_name: &str,
    access_token: &str,
    request_json: &Value,
) -> WorkerResult<()> {
    if !claim_generation_retry_submission(db, job_id, attempt).await? {
        return Ok(());
    }

    let result = match submit_generation_to_provider(
        env,
        access_token,
        json!(format!("submit:{job_id}:{attempt}")),
        tool_name,
        job_id,
        request_json,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            return handle_poll_failure(
                db,
                env,
                job_id,
                batch_id,
                attempt,
                max_attempts,
                "provider_submission_retry_failed",
                &error.to_string(),
            )
            .await;
        }
    };

    if let Some(final_url) = final_image_url(&result.raw_json) {
        if let Err(error) =
            complete_generation_job(db, env, job_id, &final_url, &result.raw_json).await
        {
            web_sys::console::error_1(
                &format!("generation completion scheduled for retry: {error:?}").into(),
            );
            enqueue_completion_retry(env, job_id, batch_id, attempt, max_attempts).await?;
        }
        return Ok(());
    }

    if provider_ids_are_empty(&provider_job_ids(&result.raw_json)) {
        return handle_poll_failure(
            db,
            env,
            job_id,
            batch_id,
            attempt,
            max_attempts,
            "provider_submission_missing_job_id",
            "Provider submission did not return a job id.",
        )
        .await;
    }

    record_provider_generation_response(db, job_id, &result.raw_json).await?;
    let next_attempt = match poll_failure_action(attempt, max_attempts) {
        PollFailureAction::Retry(next_attempt) => next_attempt,
        PollFailureAction::Fail => max_attempts.max(1),
    };
    enqueue_poll(env, job_id, batch_id, next_attempt, max_attempts).await
}

async fn schedule_submission_retry(
    db: &D1Database,
    env: &Env,
    job_id: &str,
    batch_id: &str,
    error_code: &str,
    error_message: &str,
) -> WorkerResult<()> {
    // Submission has no attempt counter. Keep the reserved job non-terminal and
    // hand it to the poll path, which owns delayed retry and final failure/refund.
    record_provider_generation_response(
        db,
        job_id,
        &json!({
            "submissionRetry": true,
            "errorCode": error_code,
            "errorMessage": error_message,
        }),
    )
    .await?;
    enqueue_poll(env, job_id, batch_id, 1, 30).await
}

async fn resume_existing_generation_job(
    db: &D1Database,
    env: &Env,
    job: &GenerationJobRow,
) -> WorkerResult<()> {
    let output_count = generation_output_count(db, &job.id).await?;
    if job.status == "completed" || output_count > 0 {
        let raw_response =
            serde_json::from_str::<Value>(&job.response_json).unwrap_or_else(|_| json!({}));
        repair_completed_generation_job(db, &job.id, job, &raw_response).await?;
        return Ok(());
    }
    if job.status == "failed" {
        repair_terminal_generation_job(db, &job.id, job).await?;
        return Ok(());
    }

    if let Some(batch_id) = job
        .blitz_batch_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        enqueue_poll(env, &job.id, batch_id, 1, 30).await?;
    }
    Ok(())
}

async fn ensure_generation_usage_reserved(
    db: &D1Database,
    job: &GenerationJobRow,
) -> WorkerResult<bool> {
    if request_has_usage_reservation_marker(&job.request_json) {
        return Ok(true);
    }

    let Some(clone) = load_ready_clone_plan(db, &job.clone_id, &job.user_id).await? else {
        fail_generation_job_without_refund(
            db,
            &job.id,
            "generation_clone_unavailable",
            "Clone was unavailable before generation usage could be reserved.",
        )
        .await?;
        if let Some(batch_id) = job.blitz_batch_id.as_deref() {
            mark_batch_ready_if_complete(db, batch_id).await?;
        }
        return Ok(false);
    };

    let generation_limits = load_generation_limits(db).await?;
    let usage_date =
        usage_date_from_request_json(&job.request_json).unwrap_or_else(current_utc_date);
    if !reserve_image_for_date(
        db,
        &job.user_id,
        &clone.plan,
        generation_limits.free_daily_limit,
        generation_limits.pro_daily_limit,
        &usage_date,
    )
    .await?
    {
        fail_generation_job_without_refund(
            db,
            &job.id,
            "daily_generation_limit_reached",
            "Daily generation limit was reached before provider submission.",
        )
        .await?;
        if let Some(batch_id) = job.blitz_batch_id.as_deref() {
            mark_batch_ready_if_complete(db, batch_id).await?;
        }
        return Ok(false);
    }

    persist_generation_usage_marker_or_refund(db, &job.id, &job.user_id, &usage_date).await?;
    Ok(true)
}

async fn poll_generation(
    db: &D1Database,
    env: &Env,
    job_id: &str,
    batch_id: &str,
    attempt: u8,
    max_attempts: u8,
) -> WorkerResult<()> {
    let Some(job) = load_generation_job(db, job_id, batch_id).await? else {
        return Ok(());
    };
    let output_count = generation_output_count(db, job_id).await?;
    if job.status == "completed" || output_count > 0 {
        let raw_response =
            serde_json::from_str::<Value>(&job.response_json).unwrap_or_else(|_| json!({}));
        repair_completed_generation_job(db, job_id, &job, &raw_response).await?;
        return Ok(());
    }
    if job.status == "failed" {
        repair_terminal_generation_job(db, job_id, &job).await?;
        return Ok(());
    }

    let original_request =
        serde_json::from_str::<Value>(&job.request_json).unwrap_or_else(|_| json!({}));
    let stored_response =
        serde_json::from_str::<Value>(&job.response_json).unwrap_or_else(|_| json!({}));
    if !ensure_generation_usage_reserved(db, &job).await? {
        return Ok(());
    }

    if let Some(final_url) = final_image_url(&stored_response) {
        if let Err(error) =
            complete_generation_job(db, env, job_id, &final_url, &stored_response).await
        {
            web_sys::console::error_1(
                &format!("generation completion scheduled for retry: {error:?}").into(),
            );
            enqueue_completion_retry(env, job_id, batch_id, attempt, max_attempts).await?;
        }
        return Ok(());
    }

    if attempt > max_attempts {
        fail_generation_job(
            db,
            job_id,
            "generation_poll_exhausted",
            "Generation polling exhausted before a terminal provider response.",
        )
        .await?;
        return Ok(());
    }

    let tool_name = match generation_tool_name(env) {
        Ok(tool_name) => tool_name,
        Err(error) => {
            fail_generation_job(db, job_id, "provider_poll_unavailable", &error.to_string())
                .await?;
            return Ok(());
        }
    };
    let provider_account_id = job
        .provider_account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(HIGGSFIELD_PROVIDER_ACCOUNT_ID);
    let token = match provider_account_access_token(
        env,
        provider_account_id,
        HIGGSFIELD_REFRESH_SECRET_NAME,
    )
    .await
    {
        Ok(token) => token,
        Err(error) => {
            return handle_poll_failure(
                db,
                env,
                job_id,
                batch_id,
                attempt,
                max_attempts,
                "provider_poll_auth_failed",
                &error.sanitized_message(),
            )
            .await;
        }
    };

    let provider_job_ids =
        serde_json::from_str::<Value>(&job.provider_job_ids_json).unwrap_or_else(|_| json!([]));

    if provider_ids_are_empty(&provider_job_ids) {
        return retry_provider_submission_from_poll(
            db,
            env,
            job_id,
            batch_id,
            attempt,
            max_attempts,
            &tool_name,
            &token.access_token,
            &original_request,
        )
        .await;
    }

    let Some(provider_job_id) = first_provider_job_id(&provider_job_ids) else {
        return handle_poll_failure(
            db,
            env,
            job_id,
            batch_id,
            attempt,
            max_attempts,
            "provider_poll_job_id_missing",
            "Provider job id was missing from stored generation response.",
        )
        .await;
    };

    let result = match call_tool(
        &token.access_token,
        json!(format!("poll:{job_id}:{attempt}")),
        HIGGSFIELD_JOB_STATUS_TOOL,
        generation_poll_arguments(&provider_job_id),
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            return handle_poll_failure(
                db,
                env,
                job_id,
                batch_id,
                attempt,
                max_attempts,
                "generation_poll_exhausted",
                &error.to_string(),
            )
            .await;
        }
    };

    if let Some(final_url) = final_image_url(&result.raw_json) {
        if let Err(error) =
            complete_generation_job(db, env, job_id, &final_url, &result.raw_json).await
        {
            web_sys::console::error_1(
                &format!("generation completion scheduled for retry: {error:?}").into(),
            );
            enqueue_completion_retry(env, job_id, batch_id, attempt, max_attempts).await?;
        }
        return Ok(());
    }

    let action = poll_failure_action(attempt, max_attempts);
    if provider_status(&result.raw_json).is_some_and(is_failed_provider_status)
        || action == PollFailureAction::Fail
    {
        if let Err(error) = record_poll_attempt(db, job_id, attempt, &result.raw_json).await {
            web_sys::console::error_1(
                &format!("failed to record terminal generation poll attempt: {error:?}").into(),
            );
        }
        fail_generation_job(
            db,
            job_id,
            "generation_failed",
            "Generation provider returned a terminal failure or polling exhausted.",
        )
        .await?;
    } else if let PollFailureAction::Retry(next_attempt) = action {
        record_poll_attempt(db, job_id, attempt, &result.raw_json).await?;
        enqueue_poll(env, job_id, batch_id, next_attempt, max_attempts).await?;
    }

    Ok(())
}

async fn handle_poll_failure(
    db: &D1Database,
    env: &Env,
    job_id: &str,
    batch_id: &str,
    attempt: u8,
    max_attempts: u8,
    error_code: &str,
    error_message: &str,
) -> WorkerResult<()> {
    let attempt_json = json!({
        "errorCode": error_code,
        "errorMessage": error_message,
    });

    match poll_failure_action(attempt, max_attempts) {
        PollFailureAction::Fail => {
            if let Err(error) = record_poll_attempt(db, job_id, attempt, &attempt_json).await {
                web_sys::console::error_1(
                    &format!("failed to record final generation poll attempt: {error:?}").into(),
                );
            }
            fail_generation_job(db, job_id, error_code, error_message).await
        }
        PollFailureAction::Retry(next_attempt) => {
            record_poll_attempt(db, job_id, attempt, &attempt_json).await?;
            enqueue_poll(env, job_id, batch_id, next_attempt, max_attempts).await
        }
    }
}

async fn complete_generation_job(
    db: &D1Database,
    env: &Env,
    job_id: &str,
    provider_url: &str,
    raw_response: &Value,
) -> WorkerResult<()> {
    let Some(job) = load_generation_job_by_id(db, job_id).await? else {
        return Ok(());
    };
    if job.status == "completed" || generation_output_count(db, job_id).await? > 0 {
        repair_completed_generation_job(db, job_id, &job, raw_response).await?;
        return Ok(());
    }
    if job.status == "failed" {
        repair_terminal_generation_job(db, job_id, &job).await?;
        return Ok(());
    }

    if !claim_generation_completion(db, job_id).await? {
        let Some(reloaded) = load_generation_job_by_id(db, job_id).await? else {
            return Ok(());
        };
        let output_exists = generation_output_count(db, job_id).await? > 0;
        match completion_claim_failure_action(&reloaded.status, output_exists) {
            CompletionClaimFailureAction::RepairCompleted => {
                repair_completed_generation_job(db, job_id, &reloaded, raw_response).await?;
            }
            CompletionClaimFailureAction::RepairFailed => {
                repair_terminal_generation_job(db, job_id, &reloaded).await?;
            }
            CompletionClaimFailureAction::RetryLater => {
                return Err(Error::RustError(
                    "generation_completion_in_progress".to_string(),
                ));
            }
            CompletionClaimFailureAction::Ignore => {}
        }
        return Ok(());
    }

    record_completion_response(db, job_id, raw_response).await?;

    let (bytes, content_type) = download_generated_image(provider_url).await?;
    let media_id = generation_media_id(job_id);
    let output_id = generation_output_id(job_id);
    let storage_key = media_storage_key(&job.user_id, &job.clone_id, &media_id, &content_type);
    let now = now_iso_string();

    env.bucket("MEDIA")?
        .put(storage_key.clone(), bytes.clone())
        .http_metadata(HttpMetadata {
            content_type: Some(content_type.clone()),
            content_language: None,
            content_disposition: None,
            content_encoding: None,
            cache_control: None,
            cache_expiry: None,
        })
        .execute()
        .await?;

    db::exec(
        db,
        r#"
        INSERT OR IGNORE INTO media_assets (
          id,
          user_id,
          clone_id,
          kind,
          source,
          storage_key,
          content_type,
          bytes,
          remote_url,
          metadata_json,
          created_at
        )
        VALUES (?, ?, ?, 'generation', 'higgsfield', ?, ?, ?, ?, ?, ?)
        "#,
        vec![
            json!(media_id),
            json!(job.user_id),
            json!(job.clone_id),
            json!(storage_key),
            json!(content_type),
            json!(bytes.len()),
            json!(provider_url),
            json!(json!({
                "jobId": job_id,
                "rawResponse": raw_response,
            })
            .to_string()),
            json!(now),
        ],
    )
    .await?;

    db::exec(
        db,
        r#"
        INSERT OR IGNORE INTO generation_outputs (
          id,
          job_id,
          user_id,
          clone_id,
          media_asset_id,
          provider_asset_id,
          raw_url,
          output_index,
          created_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?)
        "#,
        vec![
            json!(output_id),
            json!(job_id),
            json!(job.user_id),
            json!(job.clone_id),
            json!(media_id),
            json!(provider_asset_id(raw_response)),
            json!(provider_url),
            json!(now),
        ],
    )
    .await?;

    repair_completed_generation_job(db, job_id, &job, raw_response).await
}

async fn fail_generation_job(
    db: &D1Database,
    job_id: &str,
    error_code: &str,
    error_message: &str,
) -> WorkerResult<()> {
    let Some(job) = load_generation_job_by_id(db, job_id).await? else {
        return Ok(());
    };
    if job.status == "completed" {
        if let Some(batch_id) = job.blitz_batch_id.as_deref() {
            mark_batch_ready_if_complete(db, batch_id).await?;
        }
        return Ok(());
    }
    if job.status == "failed" {
        repair_failed_generation_refund(db, job_id, &job).await?;
        if let Some(batch_id) = job.blitz_batch_id.as_deref() {
            mark_batch_ready_if_complete(db, batch_id).await?;
        }
        return Ok(());
    }
    if job.status == "completing" {
        let output_exists = generation_output_count(db, job_id).await? > 0;
        let has_final_url = response_json_has_final_url(&job.response_json);
        if output_exists {
            let raw_response =
                serde_json::from_str::<Value>(&job.response_json).unwrap_or_else(|_| json!({}));
            repair_completed_generation_job(db, job_id, &job, &raw_response).await?;
            return Ok(());
        }
        let is_stale = completion_updated_at_is_stale(job.updated_at.as_deref());
        if !terminal_failure_allowed_for_job_state(
            &job.status,
            has_final_url,
            output_exists,
            is_stale,
        ) {
            return Err(Error::RustError(
                "generation_completion_in_progress".to_string(),
            ));
        }
    }

    let now = now_iso_string();
    let result = db::run(
        db,
        r#"
        UPDATE generation_jobs
        SET status = 'failed',
            error_code = ?,
            error_message = ?,
            completed_at = ?,
            updated_at = ?
        WHERE id = ?
          AND status NOT IN ('completed', 'failed')
        "#,
        vec![
            json!(error_code),
            json!(error_message),
            json!(now),
            json!(now),
            json!(job_id),
        ],
    )
    .await?;

    if changed_rows(&result)? > 0 {
        repair_failed_generation_refund(db, job_id, &job).await?;
    }
    if let Some(batch_id) = job.blitz_batch_id.as_deref() {
        mark_batch_ready_if_complete(db, batch_id).await?;
    }

    Ok(())
}

async fn fail_generation_job_without_refund(
    db: &D1Database,
    job_id: &str,
    error_code: &str,
    error_message: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE generation_jobs
        SET status = 'failed',
            error_code = ?,
            error_message = ?,
            response_json = json_set(
              CASE WHEN json_valid(response_json) THEN response_json ELSE '{}' END,
              '$.usageRefundedAt',
              ?,
              '$.usageRefundedDate',
              json_extract(
                CASE WHEN json_valid(request_json) THEN request_json ELSE '{}' END,
                '$.usageDate'
              ),
              '$.usageRefundSkipped',
              1
            ),
            completed_at = ?,
            updated_at = ?
        WHERE id = ?
          AND status NOT IN ('completed', 'failed')
        "#,
        vec![
            json!(error_code),
            json!(error_message),
            json!(now),
            json!(now),
            json!(now),
            json!(job_id),
        ],
    )
    .await
}

async fn repair_terminal_generation_job(
    db: &D1Database,
    job_id: &str,
    job: &GenerationJobRow,
) -> WorkerResult<()> {
    match job.status.as_str() {
        "completed" => {
            repair_completed_generation_side_effects(db, job).await?;
            if let Some(batch_id) = job.blitz_batch_id.as_deref() {
                mark_batch_ready_if_complete(db, batch_id).await?;
            }
        }
        "failed" => {
            repair_failed_generation_refund(db, job_id, job).await?;
            if let Some(batch_id) = job.blitz_batch_id.as_deref() {
                mark_batch_ready_if_complete(db, batch_id).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn repair_completed_generation_job(
    db: &D1Database,
    job_id: &str,
    job: &GenerationJobRow,
    raw_response: &Value,
) -> WorkerResult<()> {
    mark_generation_job_completed(db, job_id, raw_response).await?;
    repair_completed_generation_side_effects(db, job).await?;
    if let Some(batch_id) = job.blitz_batch_id.as_deref() {
        mark_batch_ready_if_complete(db, batch_id).await?;
    }
    Ok(())
}

async fn mark_batch_ready_if_complete(db: &D1Database, batch_id: &str) -> WorkerResult<()> {
    if batch_id.is_empty() {
        return Ok(());
    }

    let Some(batch) = db::first::<BatchCompletionRow>(
        db,
        r#"
        SELECT batch_size, generation_count
        FROM blitz_batches
        WHERE id = ?
        "#,
        vec![json!(batch_id)],
    )
    .await?
    else {
        return Ok(());
    };

    let terminal = db::first::<TerminalJobsRow>(
        db,
        r#"
        SELECT
          COUNT(*) AS total,
          COALESCE(SUM(CASE WHEN status IN ('completed', 'failed') THEN 1 ELSE 0 END), 0) AS terminal
        FROM generation_jobs
        WHERE blitz_batch_id = ?
        "#,
        vec![json!(batch_id)],
    )
    .await?
    .unwrap_or(TerminalJobsRow {
        total: 0,
        terminal: 0,
    });

    let now = now_iso_string();
    if batch.generation_count >= batch.batch_size
        || (terminal.total > 0 && terminal.total == terminal.terminal && batch.generation_count > 0)
    {
        db::exec(
            db,
            r#"
            UPDATE blitz_batches
            SET status = 'ready',
                ready_at = COALESCE(ready_at, ?),
                error_code = NULL,
                error_message = NULL
            WHERE id = ?
              AND status NOT IN ('ready', 'served', 'completed')
            "#,
            vec![json!(now), json!(batch_id)],
        )
        .await?;
    } else if terminal.total > 0
        && terminal.total == terminal.terminal
        && batch.generation_count == 0
    {
        db::exec(
            db,
            r#"
            UPDATE blitz_batches
            SET status = 'failed',
                error_code = 'generation_failed',
                error_message = 'All generation jobs failed.'
            WHERE id = ?
              AND status NOT IN ('ready', 'served', 'completed', 'failed')
            "#,
            vec![json!(batch_id)],
        )
        .await?;
    }

    Ok(())
}

async fn mark_batch_failed_without_jobs(db: &D1Database, batch_id: &str) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE blitz_batches
        SET status = 'failed',
            error_code = 'generation_jobs_unavailable',
            error_message = 'No generation jobs could be created for the selected visual references.'
        WHERE id = ?
          AND status NOT IN ('ready', 'served', 'completed', 'failed')
          AND NOT EXISTS (
            SELECT 1
            FROM generation_jobs
            WHERE blitz_batch_id = ?
          )
        "#,
        vec![json!(batch_id), json!(batch_id)],
    )
    .await
}

async fn load_ready_clone_plan(
    db: &D1Database,
    clone_id: &str,
    user_id: &str,
) -> WorkerResult<Option<ClonePlanRow>> {
    db::first::<ClonePlanRow>(
        db,
        r#"
        SELECT COALESCE(a.plan, 'free') AS plan
        FROM clone_profiles cp
        LEFT JOIN accounts a
          ON a.user_id = cp.user_id
         AND a.deleted_at IS NULL
        WHERE cp.id = ?
          AND cp.user_id = ?
          AND cp.deleted_at IS NULL
          AND cp.soul_status = 'ready'
        "#,
        vec![json!(clone_id), json!(user_id)],
    )
    .await
}

async fn load_visual_reference(
    db: &D1Database,
    clone_id: &str,
    user_id: &str,
    visual_reference_id: &str,
) -> WorkerResult<Option<VisualReferenceRow>> {
    let sql = visual_reference_guidance_query();
    db::first::<VisualReferenceRow>(
        db,
        &sql,
        vec![
            json!(user_id),
            json!(visual_reference_id),
            json!(clone_id),
            json!(user_id),
        ],
    )
    .await
}

fn visual_reference_guidance_query() -> String {
    format!(
        r#"
        SELECT
          ma.id AS media_asset_id,
          ma.storage_key AS storage_key,
          ma.content_type AS content_type,
          {} AS materialized_reference_url
        FROM visual_references vr
        LEFT JOIN media_assets ma
          ON ma.id = vr.media_asset_id
         AND ma.deleted_at IS NULL
        LEFT JOIN visual_reference_candidates vrc
          ON vrc.id = vr.candidate_id
         AND vrc.clone_id = vr.clone_id
         AND (vrc.user_id IS NULL OR vrc.user_id = ?)
        LEFT JOIN discovery_items di
          ON di.id = vrc.discovery_item_id
        WHERE vr.id = ?
          AND vr.clone_id = ?
          AND (vr.user_id IS NULL OR vr.user_id = ?)
          AND vr.status = 'active'
        "#,
        visual_reference_guidance_url_expr()
    )
}

fn visual_reference_guidance_url_expr() -> &'static str {
    "COALESCE(ma.remote_url, vrc.image_url, di.image_url, di.thumbnail_url, vr.source_url)"
}

async fn batch_has_generation_jobs(db: &D1Database, batch_id: &str) -> WorkerResult<bool> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM generation_jobs
        WHERE blitz_batch_id = ?
        "#,
        vec![json!(batch_id)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0) > 0)
}

async fn insert_generation_job(
    db: &D1Database,
    job_id: &str,
    user_id: &str,
    clone_id: &str,
    batch_id: &str,
    visual_reference_id: &str,
    request_json: &Value,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        r#"
        INSERT OR IGNORE INTO generation_jobs (
          id,
          user_id,
          clone_id,
          blitz_batch_id,
          input_visual_reference_id,
          status,
          request_json,
          queued_at,
          updated_at
        )
        VALUES (?, ?, ?, ?, ?, 'queued', ?, ?, ?)
        "#,
        vec![
            json!(job_id),
            json!(user_id),
            json!(clone_id),
            json!(batch_id),
            json!(visual_reference_id),
            json!(request_json.to_string()),
            json!(now),
            json!(now),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn mark_generation_job_submitting(
    db: &D1Database,
    job_id: &str,
    provider_account_id: &str,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        r#"
        UPDATE generation_jobs
        SET status = 'submitted',
            provider_account_id = COALESCE(provider_account_id, ?),
            started_at = COALESCE(started_at, ?),
            updated_at = ?
        WHERE id = ?
          AND status = 'queued'
        "#,
        vec![
            json!(provider_account_id),
            json!(now),
            json!(now),
            json!(job_id),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn claim_generation_retry_submission(
    db: &D1Database,
    job_id: &str,
    attempt: u8,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let claim_json = json!({
        "attempt": attempt,
        "claimedAt": now,
    });
    let result = db::run(
        db,
        retry_submission_claim_sql(),
        vec![
            json!(now),
            json!(claim_json.to_string()),
            json!(now),
            json!(job_id),
            json!(attempt),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

fn retry_submission_claim_sql() -> &'static str {
    r#"
        UPDATE generation_jobs
        SET status = CASE WHEN status = 'queued' THEN 'submitted' ELSE status END,
            started_at = COALESCE(started_at, ?),
            response_json = json_set(
              CASE WHEN json_valid(response_json) THEN response_json ELSE '{}' END,
              '$.submissionRetryClaim',
              json(?)
            ),
            updated_at = ?
        WHERE id = ?
          AND status IN ('queued', 'submitted')
          AND json_array_length(
            CASE WHEN json_valid(provider_job_ids_json) THEN provider_job_ids_json ELSE '[]' END
          ) = 0
          AND COALESCE(
            json_extract(
              CASE WHEN json_valid(response_json) THEN response_json ELSE '{}' END,
              '$.submissionRetryClaim.attempt'
            ),
            -1
          ) != ?
        "#
}

async fn persist_generation_usage_marker_or_refund(
    db: &D1Database,
    job_id: &str,
    user_id: &str,
    usage_date: &str,
) -> WorkerResult<()> {
    match mark_generation_usage_reserved(db, job_id, usage_date).await {
        Ok(true) => Ok(()),
        Ok(false) => {
            refund_image_for_date(db, user_id, usage_date).await?;
            let Some(job) = load_generation_job_by_id(db, job_id).await? else {
                return Err(Error::RustError(
                    "generation_usage_marker_missing_after_reservation".to_string(),
                ));
            };
            if request_has_usage_reservation_marker(&job.request_json) {
                Ok(())
            } else {
                Err(Error::RustError(
                    "generation_usage_marker_missing_after_reservation".to_string(),
                ))
            }
        }
        Err(error) => {
            refund_image_for_date(db, user_id, usage_date).await?;
            Err(error)
        }
    }
}

async fn mark_generation_usage_reserved(
    db: &D1Database,
    job_id: &str,
    usage_date: &str,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        r#"
        UPDATE generation_jobs
        SET request_json = json_set(
              CASE WHEN json_valid(request_json) THEN request_json ELSE '{}' END,
              '$.usageReserved',
              json('true'),
              '$.usageReservedAt',
              ?,
              '$.usageReservedDate',
              ?
            ),
            updated_at = ?
        WHERE id = ?
          AND json_extract(
            CASE WHEN json_valid(request_json) THEN request_json ELSE '{}' END,
            '$.usageReservedAt'
          ) IS NULL
          AND COALESCE(
            json_extract(
              CASE WHEN json_valid(request_json) THEN request_json ELSE '{}' END,
              '$.usageReserved'
            ),
            0
          ) != 1
        "#,
        vec![json!(now), json!(usage_date), json!(now), json!(job_id)],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn claim_generation_completion(db: &D1Database, job_id: &str) -> WorkerResult<bool> {
    let now = now_iso_string();
    let reclaim_cutoff = completion_reclaim_cutoff_iso();
    let result = db::run(
        db,
        r#"
        UPDATE generation_jobs
        SET status = 'completing',
            updated_at = ?
        WHERE id = ?
          AND (
            status IN ('queued', 'submitted')
            OR (status = 'completing' AND updated_at <= ?)
          )
          AND NOT EXISTS (
            SELECT 1
            FROM generation_outputs
            WHERE job_id = ?
          )
        "#,
        vec![
            json!(now),
            json!(job_id),
            json!(reclaim_cutoff),
            json!(job_id),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn record_provider_generation_response(
    db: &D1Database,
    job_id: &str,
    raw_json: &Value,
) -> WorkerResult<()> {
    if final_image_url(raw_json).is_some() {
        return Err(Error::RustError(
            "final_generation_response_must_complete".to_string(),
        ));
    }

    let provider_job_ids = provider_job_ids(raw_json);
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE generation_jobs
        SET provider_job_ids_json = ?,
            response_json = ?,
            updated_at = ?
        WHERE id = ?
          AND status IN ('queued', 'submitted')
        "#,
        vec![
            json!(provider_job_ids.to_string()),
            json!(raw_json.to_string()),
            json!(now),
            json!(job_id),
        ],
    )
    .await
}

async fn record_completion_response(
    db: &D1Database,
    job_id: &str,
    raw_json: &Value,
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE generation_jobs
        SET response_json = ?,
            updated_at = ?
        WHERE id = ?
          AND status = 'completing'
        "#,
        vec![json!(raw_json.to_string()), json!(now), json!(job_id)],
    )
    .await
}

async fn record_poll_attempt(
    db: &D1Database,
    job_id: &str,
    attempt: u8,
    raw_json: &Value,
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE generation_jobs
        SET response_json = json_set(
              CASE
                WHEN json_valid(response_json) AND json_type(response_json) = 'object'
                  THEN response_json
                ELSE '{}'
              END,
              '$.pollAttempt',
              ?,
              '$.response',
              json(?)
            ),
            updated_at = ?
        WHERE id = ?
          AND status IN ('queued', 'submitted')
        "#,
        vec![
            json!(attempt),
            json!(raw_json.to_string()),
            json!(now),
            json!(job_id),
        ],
    )
    .await
}

async fn mark_generation_job_completed(
    db: &D1Database,
    job_id: &str,
    raw_json: &Value,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        r#"
        UPDATE generation_jobs
        SET status = 'completed',
            response_json = ?,
            completed_at = ?,
            updated_at = ?
        WHERE id = ?
          AND status NOT IN ('completed', 'failed')
        "#,
        vec![
            json!(raw_json.to_string()),
            json!(now),
            json!(now),
            json!(job_id),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn repair_completed_generation_side_effects(
    db: &D1Database,
    job: &GenerationJobRow,
) -> WorkerResult<()> {
    if let Some(visual_reference_id) = job.input_visual_reference_id.as_deref() {
        db::exec(
            db,
            r#"
            UPDATE visual_references
            SET generation_use_count = (
                    SELECT COUNT(DISTINCT gj.id)
                    FROM generation_jobs gj
                    INNER JOIN generation_outputs go
                      ON go.job_id = gj.id
                    WHERE gj.input_visual_reference_id = ?
                      AND gj.status = 'completed'
                ),
                last_used_batch_id = ?
            WHERE id = ?
            "#,
            vec![
                json!(visual_reference_id),
                json!(job.blitz_batch_id),
                json!(visual_reference_id),
            ],
        )
        .await?;
    }

    if let Some(batch_id) = job.blitz_batch_id.as_deref() {
        db::exec(
            db,
            r#"
            UPDATE blitz_batches
            SET generation_count = (
                SELECT COUNT(DISTINCT gj.id)
                FROM generation_jobs gj
                INNER JOIN generation_outputs go
                  ON go.job_id = gj.id
                WHERE gj.blitz_batch_id = ?
                  AND gj.status = 'completed'
            )
            WHERE id = ?
            "#,
            vec![json!(batch_id), json!(batch_id)],
        )
        .await?;
    }

    Ok(())
}

async fn repair_failed_generation_refund(
    db: &D1Database,
    job_id: &str,
    job: &GenerationJobRow,
) -> WorkerResult<()> {
    match failed_generation_refund_action(&job.request_json, &job.response_json) {
        FailedGenerationRefundAction::AlreadyHandled => return Ok(()),
        FailedGenerationRefundAction::MarkSkipped => {
            mark_generation_refund_skipped(db, job_id, "usage_reservation_missing").await?;
            return Ok(());
        }
        FailedGenerationRefundAction::Refund => {}
    }

    let usage_date =
        usage_date_from_request_json(&job.request_json).unwrap_or_else(current_utc_date);
    let now = now_iso_string();
    db::batch(
        db,
        vec![
            (
                r#"
                UPDATE generation_daily_usage
                SET images_generated = CASE
                      WHEN images_generated > 0 THEN images_generated - 1
                      ELSE 0
                    END,
                    updated_at = ?
                WHERE user_id = ?
                  AND usage_date = ?
                  AND EXISTS (
                    SELECT 1
                    FROM generation_jobs
                    WHERE id = ?
                      AND json_extract(
                        CASE WHEN json_valid(response_json) THEN response_json ELSE '{}' END,
                        '$.usageRefundedAt'
                      ) IS NULL
                  )
                "#,
                vec![
                    json!(now),
                    json!(job.user_id),
                    json!(usage_date),
                    json!(job_id),
                ],
            ),
            (
                r#"
                UPDATE generation_jobs
                SET response_json = json_set(
                      CASE WHEN json_valid(response_json) THEN response_json ELSE '{}' END,
                      '$.usageRefundedAt',
                      ?,
                      '$.usageRefundedDate',
                      ?
                    ),
                    updated_at = ?
                WHERE id = ?
                  AND json_extract(
                    CASE WHEN json_valid(response_json) THEN response_json ELSE '{}' END,
                    '$.usageRefundedAt'
                  ) IS NULL
                "#,
                vec![json!(now), json!(usage_date), json!(now), json!(job_id)],
            ),
        ],
    )
    .await?;

    Ok(())
}

async fn mark_generation_refund_skipped(
    db: &D1Database,
    job_id: &str,
    reason: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE generation_jobs
        SET response_json = json_set(
              CASE WHEN json_valid(response_json) THEN response_json ELSE '{}' END,
              '$.usageRefundSkipped',
              1,
              '$.usageRefundSkippedAt',
              ?,
              '$.usageRefundSkippedReason',
              ?
            ),
            updated_at = ?
        WHERE id = ?
          AND json_extract(
            CASE WHEN json_valid(response_json) THEN response_json ELSE '{}' END,
            '$.usageRefundSkippedAt'
          ) IS NULL
          AND json_extract(
            CASE WHEN json_valid(response_json) THEN response_json ELSE '{}' END,
            '$.usageRefundedAt'
          ) IS NULL
        "#,
        vec![json!(now), json!(reason), json!(now), json!(job_id)],
    )
    .await
}

async fn load_generation_job(
    db: &D1Database,
    job_id: &str,
    batch_id: &str,
) -> WorkerResult<Option<GenerationJobRow>> {
    db::first::<GenerationJobRow>(
        db,
        r#"
        SELECT
          id,
          user_id,
          clone_id,
          blitz_batch_id,
          input_visual_reference_id,
          status,
          provider_account_id,
          provider_job_ids_json,
          request_json,
          response_json,
          updated_at
        FROM generation_jobs
        WHERE id = ?
          AND blitz_batch_id = ?
        "#,
        vec![json!(job_id), json!(batch_id)],
    )
    .await
}

async fn load_generation_job_by_id(
    db: &D1Database,
    job_id: &str,
) -> WorkerResult<Option<GenerationJobRow>> {
    db::first::<GenerationJobRow>(
        db,
        r#"
        SELECT
          id,
          user_id,
          clone_id,
          blitz_batch_id,
          input_visual_reference_id,
          status,
          provider_account_id,
          provider_job_ids_json,
          request_json,
          response_json,
          updated_at
        FROM generation_jobs
        WHERE id = ?
        "#,
        vec![json!(job_id)],
    )
    .await
}

async fn load_generation_job_by_batch_reference(
    db: &D1Database,
    batch_id: &str,
    visual_reference_id: &str,
) -> WorkerResult<Option<GenerationJobRow>> {
    db::first::<GenerationJobRow>(
        db,
        r#"
        SELECT
          id,
          user_id,
          clone_id,
          blitz_batch_id,
          input_visual_reference_id,
          status,
          provider_job_ids_json,
          request_json,
          response_json,
          updated_at
        FROM generation_jobs
        WHERE blitz_batch_id = ?
          AND input_visual_reference_id = ?
        ORDER BY queued_at ASC
        LIMIT 1
        "#,
        vec![json!(batch_id), json!(visual_reference_id)],
    )
    .await
}

async fn generation_output_count(db: &D1Database, job_id: &str) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM generation_outputs
        WHERE job_id = ?
        "#,
        vec![json!(job_id)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn enqueue_poll(
    env: &Env,
    job_id: &str,
    batch_id: &str,
    attempt: u8,
    max_attempts: u8,
) -> WorkerResult<()> {
    env.queue("GENERATION_QUEUE")?
        .send(
            MessageBuilder::new(GenerationMessage::PollGeneration {
                job_id: job_id.to_string(),
                batch_id: batch_id.to_string(),
                attempt,
                max_attempts,
            })
            .delay_seconds(GENERATION_POLL_DELAY_SECONDS)
            .build(),
        )
        .await
}

async fn enqueue_completion_retry(
    env: &Env,
    job_id: &str,
    batch_id: &str,
    attempt: u8,
    max_attempts: u8,
) -> WorkerResult<()> {
    enqueue_poll(
        env,
        job_id,
        batch_id,
        retryable_completion_attempt(attempt, max_attempts),
        max_attempts,
    )
    .await
}

async fn download_generated_image(provider_url: &str) -> WorkerResult<(Vec<u8>, String)> {
    let request = Request::new(provider_url, Method::Get)?;
    let mut response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    if status >= 400 {
        return Err(Error::RustError(format!(
            "generation_image_download_failed:{status}"
        )));
    }

    let content_type = normalize_generated_content_type(
        response
            .headers()
            .get("content-type")?
            .as_deref()
            .unwrap_or("image/jpeg"),
    );
    if content_length_exceeds_generated_image_limit(
        response.headers().get("content-length")?.as_deref(),
    ) {
        return Err(Error::RustError(
            "generation_image_download_too_large".to_string(),
        ));
    }
    let bytes = response.bytes().await?;
    if bytes.is_empty() {
        return Err(Error::RustError(
            "generation_image_download_empty".to_string(),
        ));
    }
    if generated_image_size_too_large(bytes.len()) {
        return Err(Error::RustError(
            "generation_image_download_too_large".to_string(),
        ));
    }

    Ok((bytes, content_type.to_string()))
}

fn generation_tool_name(env: &Env) -> WorkerResult<String> {
    match env.var(HIGGSFIELD_GENERATION_TOOL_VAR) {
        Ok(tool_name) if !tool_name.to_string().trim().is_empty() => Ok(tool_name.to_string()),
        _ => Err(Error::RustError(
            "higgsfield_generation_tool_missing".to_string(),
        )),
    }
}

async fn submit_generation_to_provider(
    env: &Env,
    access_token: &str,
    request_id: Value,
    tool_name: &str,
    job_id: &str,
    request_json: &Value,
) -> WorkerResult<crate::providers::higgsfield_mcp::HiggsfieldMcpResponse> {
    let upload_file = load_generation_reference_upload(env, job_id, request_json).await?;
    let uploaded = upload_media_files(access_token, &[upload_file])
        .await
        .map_err(map_mcp_error)?;
    let Some(uploaded_reference) = uploaded.first() else {
        return Err(Error::RustError(
            "generation_reference_upload_missing".to_string(),
        ));
    };

    let mut provider_request = request_json.clone();
    if let Some(object) = provider_request.as_object_mut() {
        object.insert(
            "uploadedReferenceUrl".to_string(),
            json!(uploaded_reference.url.clone()),
        );
    }
    let arguments = submission_arguments_from_request(job_id, &provider_request)?;
    call_tool(access_token, request_id, tool_name, arguments)
        .await
        .map_err(map_mcp_error)
}

async fn load_generation_reference_upload(
    env: &Env,
    job_id: &str,
    request_json: &Value,
) -> WorkerResult<HiggsfieldMcpMediaFile> {
    let content_type = json_string_at(request_json, "/inputContentType")
        .map(|value| normalize_generated_content_type(&value).to_string())
        .unwrap_or_else(|| "image/jpeg".to_string());

    if let Some(storage_key) = json_string_at(request_json, "/inputStorageKey") {
        let bytes = read_media_object_bytes(env, &storage_key).await?;
        return Ok(HiggsfieldMcpMediaFile {
            filename: provider_upload_filename(job_id, &content_type),
            content_type,
            bytes,
        });
    }

    let input_url = required_json_string(request_json, "/inputImageUrl")?;
    let (bytes, fetched_content_type) = fetch_reference_image(&input_url).await?;
    Ok(HiggsfieldMcpMediaFile {
        filename: provider_upload_filename(job_id, &fetched_content_type),
        content_type: fetched_content_type,
        bytes,
    })
}

async fn read_media_object_bytes(env: &Env, storage_key: &str) -> WorkerResult<Vec<u8>> {
    let object = env
        .bucket("MEDIA")?
        .get(storage_key.to_string())
        .execute()
        .await?
        .ok_or_else(|| Error::RustError("generation_reference_media_missing".to_string()))?;
    let body = object
        .body()
        .ok_or_else(|| Error::RustError("generation_reference_media_body_missing".to_string()))?;
    body.bytes().await
}

async fn fetch_reference_image(reference_url: &str) -> WorkerResult<(Vec<u8>, String)> {
    let request = Request::new(reference_url, Method::Get)?;
    let mut response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    if status >= 400 {
        return Err(Error::RustError(format!(
            "generation_reference_download_failed:{status}"
        )));
    }

    let content_type = normalize_generated_content_type(
        response
            .headers()
            .get("content-type")?
            .as_deref()
            .unwrap_or("image/jpeg"),
    );
    if content_length_exceeds_generated_image_limit(
        response.headers().get("content-length")?.as_deref(),
    ) {
        return Err(Error::RustError(
            "generation_reference_download_too_large".to_string(),
        ));
    }
    let bytes = response.bytes().await?;
    if bytes.is_empty() {
        return Err(Error::RustError(
            "generation_reference_download_empty".to_string(),
        ));
    }
    if generated_image_size_too_large(bytes.len()) {
        return Err(Error::RustError(
            "generation_reference_download_too_large".to_string(),
        ));
    }

    Ok((bytes, content_type.to_string()))
}

fn provider_upload_filename(job_id: &str, content_type: &str) -> String {
    let extension = match normalize_generated_content_type(content_type) {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/heic" => "heic",
        _ => "jpg",
    };
    format!("{}.{}", generation_id_suffix(job_id), extension)
}

fn final_image_url(raw_json: &Value) -> Option<String> {
    provider_payloads(raw_json)
        .iter()
        .find_map(|payload| direct_final_image_url(payload))
}

fn provider_job_ids(raw_json: &Value) -> Value {
    let mut ids = Vec::new();
    for payload in provider_payloads(raw_json) {
        for path in [
            "/result/id",
            "/result/job_id",
            "/result/jobId",
            "/id",
            "/job_id",
            "/jobId",
        ] {
            if let Some(id) = json_string_at(&payload, path) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    json!(ids)
}

fn provider_asset_id(raw_json: &Value) -> Option<String> {
    provider_payloads(raw_json).iter().find_map(|payload| {
        [
            "/result/asset_id",
            "/result/assetId",
            "/asset_id",
            "/assetId",
        ]
        .into_iter()
        .find_map(|path| json_string_at(payload, path))
    })
}

fn provider_status(raw_json: &Value) -> Option<String> {
    provider_payloads(raw_json).iter().find_map(|payload| {
        ["/result/status", "/result/state", "/status", "/state"]
            .into_iter()
            .find_map(|path| json_string_at(payload, path))
            .map(|value| value.trim().to_ascii_lowercase())
    })
}

fn direct_final_image_url(value: &Value) -> Option<String> {
    [
        "/result/image_url",
        "/result/url",
        "/result/output_url",
        "/image_url",
        "/url",
        "/output_url",
        "/result/imageUrl",
        "/imageUrl",
        "/result/assets/0/url",
        "/result/images/0/url",
        "/result/outputs/0/url",
        "/result/generations/0/url",
        "/assets/0/url",
        "/images/0/url",
        "/outputs/0/url",
        "/generations/0/url",
    ]
    .into_iter()
    .find_map(|path| json_string_at(value, path))
}

fn provider_payloads(raw_json: &Value) -> Vec<Value> {
    let mut payloads = vec![raw_json.clone()];
    collect_mcp_content_payloads(raw_json, &mut payloads, 0);
    payloads
}

fn collect_mcp_content_payloads(value: &Value, payloads: &mut Vec<Value>, depth: u8) {
    if depth >= 3 {
        return;
    }

    for path in ["/result/content", "/content"] {
        let Some(content) = value.pointer(path).and_then(Value::as_array) else {
            continue;
        };
        for item in content {
            let Some(text) = item
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            payloads.push(parsed.clone());
            collect_mcp_content_payloads(&parsed, payloads, depth + 1);
        }
    }
}

fn usage_date_from_request_json(request_json: &str) -> Option<String> {
    serde_json::from_str::<Value>(request_json)
        .ok()
        .and_then(|value| json_string_at(&value, "/usageDate"))
}

fn request_has_usage_reservation_marker(request_json: &str) -> bool {
    serde_json::from_str::<Value>(request_json)
        .ok()
        .is_some_and(|value| {
            value
                .get("usageReserved")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || json_string_at(&value, "/usageReservedAt").is_some()
        })
}

fn submission_arguments_from_request(_job_id: &str, request_json: &Value) -> WorkerResult<Value> {
    Ok(json!({
        "params": {
            "model": "soul_2",
            "prompt": "",
            "soul_id": required_json_string(request_json, "/providerSoulId")?,
            "medias": [{
                "value": required_json_string(request_json, "/uploadedReferenceUrl")?,
                "role": "image",
            }],
            "count": 1,
        }
    }))
}

fn generation_poll_arguments(provider_job_id: &str) -> Value {
    json!({
        "jobId": provider_job_id,
        "sync": true,
    })
}

fn required_json_string(value: &Value, path: &str) -> WorkerResult<String> {
    json_string_at(value, path)
        .ok_or_else(|| Error::RustError(format!("missing_generation_request_field:{path}")))
}

fn provider_ids_are_empty(provider_job_ids: &Value) -> bool {
    provider_job_ids
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .all(|value| value.trim().is_empty())
        })
        .unwrap_or(true)
}

fn first_provider_job_id(provider_job_ids: &Value) -> Option<String> {
    provider_job_ids
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn response_has_usage_refund_marker(response_json: &str) -> bool {
    serde_json::from_str::<Value>(response_json)
        .ok()
        .and_then(|value| json_string_at(&value, "/usageRefundedAt"))
        .is_some()
}

fn response_has_usage_refund_skip_marker(response_json: &str) -> bool {
    serde_json::from_str::<Value>(response_json)
        .ok()
        .and_then(|value| json_string_at(&value, "/usageRefundSkippedAt"))
        .is_some()
}

fn failed_generation_refund_action(
    request_json: &str,
    response_json: &str,
) -> FailedGenerationRefundAction {
    if response_has_usage_refund_marker(response_json)
        || response_has_usage_refund_skip_marker(response_json)
    {
        FailedGenerationRefundAction::AlreadyHandled
    } else if request_has_usage_reservation_marker(request_json) {
        FailedGenerationRefundAction::Refund
    } else {
        FailedGenerationRefundAction::MarkSkipped
    }
}

fn response_json_has_final_url(response_json: &str) -> bool {
    serde_json::from_str::<Value>(response_json)
        .ok()
        .and_then(|value| final_image_url(&value))
        .is_some()
}

fn deterministic_generation_job_id(batch_id: &str, visual_reference_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(batch_id.as_bytes());
    hasher.update(b":");
    hasher.update(visual_reference_id.as_bytes());
    let digest = hasher.finalize();
    format!("gen_{}", &hex::encode(digest)[..24])
}

fn generation_media_id(job_id: &str) -> String {
    format!("media_{}", generation_id_suffix(job_id))
}

fn generation_output_id(job_id: &str) -> String {
    format!("gout_{}", generation_id_suffix(job_id))
}

fn generation_id_suffix(job_id: &str) -> String {
    let raw = job_id.strip_prefix("gen_").unwrap_or(job_id);
    let normalized = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '-'
            }
        })
        .take(96)
        .collect::<String>();
    if normalized.is_empty() {
        "job".to_string()
    } else {
        normalized
    }
}

fn is_failed_provider_status(status: String) -> bool {
    matches!(
        status.as_str(),
        "failed" | "failure" | "error" | "errored" | "canceled" | "cancelled"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PollFailureAction {
    Retry(u8),
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionClaimFailureAction {
    RepairCompleted,
    RepairFailed,
    RetryLater,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailedGenerationRefundAction {
    AlreadyHandled,
    Refund,
    MarkSkipped,
}

fn poll_failure_action(attempt: u8, max_attempts: u8) -> PollFailureAction {
    if attempt >= max_attempts {
        PollFailureAction::Fail
    } else {
        PollFailureAction::Retry(attempt.saturating_add(1))
    }
}

fn retryable_completion_attempt(attempt: u8, max_attempts: u8) -> u8 {
    if max_attempts == 0 {
        1
    } else {
        attempt.max(1).min(max_attempts)
    }
}

fn completion_claim_failure_action(
    status: &str,
    output_exists: bool,
) -> CompletionClaimFailureAction {
    if output_exists || status == "completed" {
        CompletionClaimFailureAction::RepairCompleted
    } else if status == "failed" {
        CompletionClaimFailureAction::RepairFailed
    } else if matches!(status, "queued" | "submitted" | "completing") {
        CompletionClaimFailureAction::RetryLater
    } else {
        CompletionClaimFailureAction::Ignore
    }
}

fn terminal_failure_allowed_for_job_state(
    status: &str,
    has_final_url: bool,
    output_exists: bool,
    is_stale: bool,
) -> bool {
    status != "completing" || (is_stale && !has_final_url && !output_exists)
}

fn is_permanent_submission_error(error: &Error) -> bool {
    let message = error.to_string();
    message.contains("higgsfield_generation_tool_missing")
        || message.contains("missing_generation_request_field")
}

fn json_string_at(value: &Value, path: &str) -> Option<String> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn normalize_generated_content_type(content_type: &str) -> &'static str {
    match content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => "image/png",
        "image/webp" => "image/webp",
        "image/heic" | "image/heif" => "image/heic",
        _ => "image/jpeg",
    }
}

fn content_length_exceeds_generated_image_limit(content_length: Option<&str>) -> bool {
    content_length
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(generated_image_size_too_large)
        .unwrap_or(false)
}

fn generated_image_size_too_large(byte_count: usize) -> bool {
    byte_count > MAX_GENERATED_IMAGE_BYTES
}

#[cfg(test)]
fn poll_attempt_response_json(
    existing_response_json: &str,
    attempt: u8,
    raw_json: &Value,
) -> String {
    let mut value =
        serde_json::from_str::<Value>(existing_response_json).unwrap_or_else(|_| json!({}));
    if !value.is_object() {
        value = json!({});
    }
    if let Some(object) = value.as_object_mut() {
        object.insert("pollAttempt".to_string(), json!(attempt));
        object.insert("response".to_string(), raw_json.clone());
    }
    value.to_string()
}

fn changed_rows(result: &worker::D1Result) -> WorkerResult<usize> {
    Ok(result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or_default())
}

fn map_mcp_error(error: HiggsfieldMcpError) -> Error {
    match error {
        HiggsfieldMcpError::Worker(worker_error) => worker_error,
        other => Error::RustError(other.to_string()),
    }
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

fn completion_reclaim_cutoff_iso() -> String {
    let cutoff_ms = js_sys::Date::now() - 120_000.0;
    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(cutoff_ms))
        .to_iso_string()
        .into()
}

fn completion_updated_at_is_stale(updated_at: Option<&str>) -> bool {
    let Some(updated_at) = updated_at else {
        return false;
    };
    let updated_ms = js_sys::Date::parse(updated_at);
    updated_ms.is_finite() && js_sys::Date::now() - updated_ms >= 120_000.0
}

#[cfg(test)]
mod tests {
    use super::{
        completion_claim_failure_action, content_length_exceeds_generated_image_limit,
        deterministic_generation_job_id, failed_generation_refund_action, final_image_url,
        generated_image_size_too_large, generation_media_id, generation_output_id,
        generation_poll_arguments, poll_attempt_response_json, poll_failure_action,
        provider_asset_id, provider_ids_are_empty, provider_job_ids, provider_status,
        request_has_usage_reservation_marker, response_has_usage_refund_marker,
        retry_submission_claim_sql, retryable_completion_attempt,
        submission_arguments_from_request, terminal_failure_allowed_for_job_state,
        usage_date_from_request_json, visual_reference_guidance_query,
        visual_reference_guidance_url_expr, CompletionClaimFailureAction,
        FailedGenerationRefundAction, PollFailureAction, MAX_GENERATED_IMAGE_BYTES,
    };
    use serde_json::json;

    #[test]
    fn poll_failure_action_retries_before_final_attempt() {
        assert_eq!(poll_failure_action(1, 3), PollFailureAction::Retry(2));
        assert_eq!(poll_failure_action(2, 3), PollFailureAction::Retry(3));
    }

    #[test]
    fn poll_failure_action_fails_on_final_or_exhausted_attempt() {
        assert_eq!(poll_failure_action(3, 3), PollFailureAction::Fail);
        assert_eq!(poll_failure_action(4, 3), PollFailureAction::Fail);
    }

    #[test]
    fn retryable_completion_attempt_stays_within_poll_bounds() {
        assert_eq!(retryable_completion_attempt(0, 30), 1);
        assert_eq!(retryable_completion_attempt(7, 30), 7);
        assert_eq!(retryable_completion_attempt(31, 30), 30);
        assert_eq!(retryable_completion_attempt(1, 0), 1);
    }

    #[test]
    fn retry_submission_claim_guards_provider_call_by_attempt() {
        let sql = retry_submission_claim_sql();

        assert!(sql.contains("status IN ('queued', 'submitted')"));
        assert!(sql.contains("json_array_length"));
        assert!(sql.contains("provider_job_ids_json"));
        assert!(sql.contains("$.submissionRetryClaim.attempt"));
        assert!(sql.contains("!= ?"));
    }

    #[test]
    fn poll_attempt_response_merge_preserves_retry_claim() {
        let existing = json!({
            "submissionRetry": true,
            "submissionRetryClaim": {
                "attempt": 2,
                "claimedAt": "2026-05-12T00:00:00.000Z"
            }
        })
        .to_string();
        let merged = poll_attempt_response_json(
            &existing,
            2,
            &json!({
                "errorCode": "provider_submission_retry_failed",
                "errorMessage": "network ambiguity"
            }),
        );
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();

        assert_eq!(value["submissionRetryClaim"]["attempt"], 2);
        assert_eq!(value["pollAttempt"], 2);
        assert_eq!(
            value["response"]["errorCode"],
            "provider_submission_retry_failed"
        );

        let later_attempt = 3;
        assert_ne!(
            value["submissionRetryClaim"]["attempt"].as_u64().unwrap(),
            later_attempt
        );
    }

    #[test]
    fn completion_claim_failure_action_retries_reclaimable_states() {
        assert_eq!(
            completion_claim_failure_action("completing", false),
            CompletionClaimFailureAction::RetryLater
        );
        assert_eq!(
            completion_claim_failure_action("submitted", false),
            CompletionClaimFailureAction::RetryLater
        );
        assert_eq!(
            completion_claim_failure_action("completing", true),
            CompletionClaimFailureAction::RepairCompleted
        );
        assert_eq!(
            completion_claim_failure_action("failed", false),
            CompletionClaimFailureAction::RepairFailed
        );
    }

    #[test]
    fn generation_output_ids_are_deterministic_from_job_id() {
        assert_eq!(generation_media_id("gen_abc123"), "media_abc123");
        assert_eq!(generation_output_id("gen_abc123"), "gout_abc123");
    }

    #[test]
    fn deterministic_generation_job_id_is_stable_and_safe() {
        let id = deterministic_generation_job_id("batch/one", "vref:two");
        assert_eq!(id, deterministic_generation_job_id("batch/one", "vref:two"));
        assert!(id.starts_with("gen_"));
        assert_eq!(id.len(), 28);
        assert!(id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
    }

    #[test]
    fn visual_reference_guidance_query_prefers_real_image_urls_before_source_posts() {
        assert_eq!(
            visual_reference_guidance_url_expr(),
            "COALESCE(ma.remote_url, vrc.image_url, di.image_url, di.thumbnail_url, vr.source_url)"
        );

        let query = visual_reference_guidance_query();

        assert!(query.contains("LEFT JOIN visual_reference_candidates vrc"));
        assert!(query.contains("vrc.id = vr.candidate_id"));
        assert!(query.contains("LEFT JOIN discovery_items di"));
        assert!(query.contains("di.id = vrc.discovery_item_id"));
    }

    #[test]
    fn generated_image_size_limit_rejects_large_downloads() {
        assert!(!generated_image_size_too_large(MAX_GENERATED_IMAGE_BYTES));
        assert!(generated_image_size_too_large(
            MAX_GENERATED_IMAGE_BYTES + 1
        ));
        assert!(content_length_exceeds_generated_image_limit(Some(
            "15728641"
        )));
        assert!(!content_length_exceeds_generated_image_limit(Some(
            "15728640"
        )));
        assert!(!content_length_exceeds_generated_image_limit(Some("bad")));
        assert!(!content_length_exceeds_generated_image_limit(None));
    }

    #[test]
    fn provider_helpers_parse_mcp_content_text_wrappers() {
        let wrapped = json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "{\"id\":\"hf_job_1\",\"status\":\"completed\",\"assetId\":\"asset_1\",\"image_url\":\"https://cdn.example/out.jpg\"}"
                }]
            }
        });

        assert_eq!(
            final_image_url(&wrapped),
            Some("https://cdn.example/out.jpg".to_string())
        );
        assert_eq!(provider_job_ids(&wrapped), json!(["hf_job_1"]));
        assert_eq!(provider_asset_id(&wrapped), Some("asset_1".to_string()));
        assert_eq!(provider_status(&wrapped), Some("completed".to_string()));
    }

    #[test]
    fn usage_date_is_read_from_generation_request_json() {
        assert_eq!(
            usage_date_from_request_json(r#"{"usageDate":"2026-05-11"}"#),
            Some("2026-05-11".to_string())
        );
        assert_eq!(usage_date_from_request_json("{}"), None);
    }

    #[test]
    fn usage_reservation_marker_accepts_boolean_or_timestamp() {
        assert!(request_has_usage_reservation_marker(
            r#"{"usageReserved":true}"#
        ));
        assert!(request_has_usage_reservation_marker(
            r#"{"usageReservedAt":"2026-05-11T01:02:03.000Z"}"#
        ));
        assert!(!request_has_usage_reservation_marker(
            r#"{"usageReserved":false}"#
        ));
        assert!(!request_has_usage_reservation_marker("{}"));
        assert!(!request_has_usage_reservation_marker("not json"));
    }

    #[test]
    fn failed_generation_refund_action_requires_usage_reservation_marker() {
        assert_eq!(
            failed_generation_refund_action(r#"{"usageReserved":true}"#, r#"{}"#),
            FailedGenerationRefundAction::Refund
        );
        assert_eq!(
            failed_generation_refund_action(r#"{}"#, r#"{}"#),
            FailedGenerationRefundAction::MarkSkipped
        );
        assert_eq!(
            failed_generation_refund_action(
                r#"{"usageReserved":true}"#,
                r#"{"usageRefundedAt":"2026-05-11T01:02:03.000Z"}"#
            ),
            FailedGenerationRefundAction::AlreadyHandled
        );
        assert_eq!(
            failed_generation_refund_action(
                r#"{}"#,
                r#"{"usageRefundSkippedAt":"2026-05-11T01:02:03.000Z"}"#
            ),
            FailedGenerationRefundAction::AlreadyHandled
        );
    }

    #[test]
    fn terminal_failure_decision_protects_active_completing_jobs() {
        assert!(!terminal_failure_allowed_for_job_state(
            "completing",
            true,
            false,
            true
        ));
        assert!(!terminal_failure_allowed_for_job_state(
            "completing",
            false,
            true,
            true
        ));
        assert!(!terminal_failure_allowed_for_job_state(
            "completing",
            false,
            false,
            false
        ));
        assert!(terminal_failure_allowed_for_job_state(
            "completing",
            false,
            false,
            true
        ));
        assert!(terminal_failure_allowed_for_job_state(
            "submitted",
            false,
            false,
            false
        ));
    }

    #[test]
    fn empty_provider_ids_trigger_submission_retry_decision() {
        assert!(provider_ids_are_empty(&json!([])));
        assert!(provider_ids_are_empty(&json!([""])));
        assert!(!provider_ids_are_empty(&json!(["hf_job_1"])));
        assert!(provider_ids_are_empty(&json!({})));
    }

    #[test]
    fn submission_arguments_match_validated_generate_image_payload() {
        let request = json!({
            "jobId": "gen_1",
            "batchId": "batch_1",
            "cloneId": "clone_1",
            "userId": "user_1",
            "idempotencyKey": "blitz_gen:batch_1:vref_1",
            "providerSoulId": "soul_1",
            "inputImageUrl": "https://cdn.example/input.jpg",
            "visualReferenceId": "vref_1",
            "usageDate": "2026-05-11",
            "prompt": "",
            "uploadedReferenceUrl": "https://higgsfield.example/uploaded.jpg"
        });

        assert_eq!(
            submission_arguments_from_request("fallback", &request).unwrap(),
            json!({
                "params": {
                    "model": "soul_2",
                    "prompt": "",
                    "soul_id": "soul_1",
                    "medias": [{
                        "value": "https://higgsfield.example/uploaded.jpg",
                        "role": "image"
                    }],
                    "count": 1
                }
            })
        );
    }

    #[test]
    fn generation_poll_arguments_use_job_status_payload() {
        assert_eq!(
            generation_poll_arguments("hf_job_1"),
            json!({
                "jobId": "hf_job_1",
                "sync": true
            })
        );
    }

    #[test]
    fn usage_refund_marker_is_read_from_response_json() {
        assert!(response_has_usage_refund_marker(
            r#"{"usageRefundedAt":"2026-05-11T01:02:03.000Z"}"#
        ));
        assert!(!response_has_usage_refund_marker("{}"));
        assert!(!response_has_usage_refund_marker("not json"));
    }
}
