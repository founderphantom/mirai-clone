use crate::db;
use crate::providers::higgsfield_auth::{refresh_access_token, validate_access_token};
use crate::providers::higgsfield_mcp::{call_tool, HiggsfieldMcpError};
use crate::queues::messages::GenerationMessage;
use crate::services::generation_usage::{current_utc_date, reserve_image_for_date};
use crate::services::media::media_storage_key;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use worker::{
    D1Database, Env, Error, Fetch, HttpMetadata, MessageBatch, MessageBuilder, MessageExt, Method,
    Request, Result as WorkerResult,
};

const HIGGSFIELD_REFRESH_SECRET_NAME: &str = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FUFU";
const HIGGSFIELD_GENERATION_TOOL_VAR: &str = "HIGGSFIELD_MCP_GENERATION_TOOL";
const GENERATION_POLL_DELAY_SECONDS: u32 = 10;

#[derive(Debug, Deserialize)]
struct ClonePlanRow {
    plan: String,
}

#[derive(Debug, Deserialize)]
struct ConfigRow {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct VisualReferenceRow {
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
    provider_job_ids_json: String,
    request_json: String,
    response_json: String,
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
    let (free_daily_limit, pro_daily_limit) = load_generation_limits(db).await?;

    for visual_reference_id in visual_reference_ids {
        if let Some(existing) =
            load_generation_job_by_batch_reference(db, batch_id, visual_reference_id).await?
        {
            repair_terminal_generation_job(db, &existing.id, &existing).await?;
            continue;
        }

        let Some(reference) =
            load_visual_reference(db, clone_id, user_id, visual_reference_id).await?
        else {
            continue;
        };
        let Some(materialized_reference_url) = reference
            .materialized_reference_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

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
                repair_terminal_generation_job(db, &job_id, &existing).await?;
            }
            continue;
        }

        if !reserve_image_for_date(
            db,
            user_id,
            &clone.plan,
            free_daily_limit,
            pro_daily_limit,
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
            fail_generation_job(
                db,
                &job_id,
                "provider_submission_failed",
                &error.to_string(),
            )
            .await?;
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
    materialized_reference_url: &str,
) -> WorkerResult<()> {
    let tool_name = generation_tool_name(env)?;
    let token = match refresh_access_token(env, HIGGSFIELD_REFRESH_SECRET_NAME).await {
        Ok(token) => token,
        Err(error) => {
            schedule_submission_retry(
                db,
                env,
                job_id,
                batch_id,
                "provider_submission_auth_retry",
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    };
    let validation = match validate_access_token(&token.access_token).await {
        Ok(validation) => validation,
        Err(error) => {
            schedule_submission_retry(
                db,
                env,
                job_id,
                batch_id,
                "provider_submission_validation_retry",
                &error.to_string(),
            )
            .await?;
            return Ok(());
        }
    };
    if !validation.valid {
        schedule_submission_retry(
            db,
            env,
            job_id,
            batch_id,
            "provider_submission_token_invalid",
            "Higgsfield provider access token is invalid.",
        )
        .await?;
        return Ok(());
    }

    mark_generation_job_submitting(db, job_id).await?;

    let request = db::first::<GenerationJobRow>(
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
          response_json
        FROM generation_jobs
        WHERE id = ?
        "#,
        vec![json!(job_id)],
    )
    .await?
    .and_then(|job| serde_json::from_str::<Value>(&job.request_json).ok())
    .unwrap_or_else(|| {
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
    let arguments = submission_arguments_from_request(job_id, &request)?;

    let result = match call_tool(&token.access_token, json!(job_id), &tool_name, arguments).await {
        Ok(result) => result,
        Err(error) => {
            let mapped = map_mcp_error(error);
            schedule_submission_retry(
                db,
                env,
                job_id,
                batch_id,
                "provider_submission_retry",
                &mapped.to_string(),
            )
            .await?;
            return Ok(());
        }
    };

    if let Some(final_url) = final_image_url(&result.raw_json) {
        complete_generation_job(db, env, job_id, &final_url, &result.raw_json).await?;
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
    let arguments = match submission_arguments_from_request(job_id, request_json) {
        Ok(arguments) => arguments,
        Err(error) => {
            return handle_poll_failure(
                db,
                env,
                job_id,
                batch_id,
                attempt,
                max_attempts,
                "generation_submission_request_invalid",
                &error.to_string(),
            )
            .await;
        }
    };

    let result = match call_tool(
        access_token,
        json!(format!("submit:{job_id}:{attempt}")),
        tool_name,
        arguments,
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
                &map_mcp_error(error).to_string(),
            )
            .await;
        }
    };

    if let Some(final_url) = final_image_url(&result.raw_json) {
        if let Err(error) =
            complete_generation_job(db, env, job_id, &final_url, &result.raw_json).await
        {
            return handle_poll_failure(
                db,
                env,
                job_id,
                batch_id,
                attempt,
                max_attempts,
                "generation_completion_failed",
                &error.to_string(),
            )
            .await;
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
    match poll_failure_action(attempt, max_attempts) {
        PollFailureAction::Fail => {
            fail_generation_job(
                db,
                job_id,
                "generation_poll_exhausted",
                "Provider submission succeeded on final attempt but polling is exhausted.",
            )
            .await
        }
        PollFailureAction::Retry(next_attempt) => {
            enqueue_poll(env, job_id, batch_id, next_attempt, max_attempts).await
        }
    }
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
    if matches!(job.status.as_str(), "completed" | "failed") {
        repair_terminal_generation_job(db, job_id, &job).await?;
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
    let token = match refresh_access_token(env, HIGGSFIELD_REFRESH_SECRET_NAME).await {
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
                &error.to_string(),
            )
            .await;
        }
    };
    let validation = match validate_access_token(&token.access_token).await {
        Ok(validation) => validation,
        Err(error) => {
            return handle_poll_failure(
                db,
                env,
                job_id,
                batch_id,
                attempt,
                max_attempts,
                "provider_poll_auth_failed",
                &error.to_string(),
            )
            .await
        }
    };
    if !validation.valid {
        return handle_poll_failure(
            db,
            env,
            job_id,
            batch_id,
            attempt,
            max_attempts,
            "provider_poll_token_invalid",
            "Higgsfield provider access token is invalid.",
        )
        .await;
    }

    let original_request =
        serde_json::from_str::<Value>(&job.request_json).unwrap_or_else(|_| json!({}));
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

    let result = match call_tool(
        &token.access_token,
        json!(format!("poll:{job_id}:{attempt}")),
        &tool_name,
        json!({
            "action": "poll",
            "jobId": job_id,
            "batchId": batch_id,
            "providerJobIds": provider_job_ids,
            "attempt": attempt,
            "request": original_request,
        }),
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
            return handle_poll_failure(
                db,
                env,
                job_id,
                batch_id,
                attempt,
                max_attempts,
                "generation_completion_failed",
                &error.to_string(),
            )
            .await;
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
        if mark_generation_job_completed(db, job_id, raw_response).await? {
            repair_completed_generation_side_effects(db, &job).await?;
        } else {
            repair_completed_generation_side_effects(db, &job).await?;
        }
        mark_batch_ready_if_complete(db, job.blitz_batch_id.as_deref().unwrap_or_default()).await?;
        return Ok(());
    }
    if job.status == "failed" {
        mark_batch_ready_if_complete(db, job.blitz_batch_id.as_deref().unwrap_or_default()).await?;
        return Ok(());
    }

    if !claim_generation_completion(db, job_id).await? {
        let Some(reloaded) = load_generation_job_by_id(db, job_id).await? else {
            return Ok(());
        };
        if reloaded.status == "completed" || generation_output_count(db, job_id).await? > 0 {
            repair_terminal_generation_job(db, job_id, &reloaded).await?;
        }
        return Ok(());
    }

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

    let completed = mark_generation_job_completed(db, job_id, raw_response).await?;
    if completed {
        repair_completed_generation_side_effects(db, &job).await?;
    } else {
        repair_completed_generation_side_effects(db, &job).await?;
    }

    if let Some(batch_id) = job.blitz_batch_id.as_deref() {
        mark_batch_ready_if_complete(db, batch_id).await?;
    }

    Ok(())
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

async fn load_generation_limits(db: &D1Database) -> WorkerResult<(u32, u32)> {
    let rows = db::all::<ConfigRow>(
        db,
        r#"
        SELECT key, value
        FROM blitz_config
        WHERE key IN ('free_daily_limit', 'pro_daily_limit')
        "#,
        vec![],
    )
    .await?;

    let mut free_daily_limit = 10;
    let mut pro_daily_limit = 50;
    for row in rows {
        let parsed = row.value.parse::<u32>().unwrap_or(0);
        match row.key.as_str() {
            "free_daily_limit" if parsed > 0 => free_daily_limit = parsed,
            "pro_daily_limit" if parsed > 0 => pro_daily_limit = parsed,
            _ => {}
        }
    }

    Ok((free_daily_limit, pro_daily_limit))
}

async fn load_visual_reference(
    db: &D1Database,
    clone_id: &str,
    user_id: &str,
    visual_reference_id: &str,
) -> WorkerResult<Option<VisualReferenceRow>> {
    db::first::<VisualReferenceRow>(
        db,
        r#"
        SELECT COALESCE(ma.remote_url, vr.source_url) AS materialized_reference_url
        FROM visual_references vr
        LEFT JOIN media_assets ma
          ON ma.id = vr.media_asset_id
         AND ma.deleted_at IS NULL
        WHERE vr.id = ?
          AND vr.clone_id = ?
          AND (vr.user_id IS NULL OR vr.user_id = ?)
          AND vr.status = 'active'
        "#,
        vec![json!(visual_reference_id), json!(clone_id), json!(user_id)],
    )
    .await
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

async fn mark_generation_job_submitting(db: &D1Database, job_id: &str) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE generation_jobs
        SET status = 'submitted',
            started_at = COALESCE(started_at, ?),
            updated_at = ?
        WHERE id = ?
          AND status = 'queued'
        "#,
        vec![json!(now), json!(now), json!(job_id)],
    )
    .await
}

async fn claim_generation_completion(db: &D1Database, job_id: &str) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        r#"
        UPDATE generation_jobs
        SET status = 'completing',
            updated_at = ?
        WHERE id = ?
          AND status IN ('queued', 'submitted')
          AND NOT EXISTS (
            SELECT 1
            FROM generation_outputs
            WHERE job_id = ?
          )
        "#,
        vec![json!(now), json!(job_id), json!(job_id)],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn record_provider_generation_response(
    db: &D1Database,
    job_id: &str,
    raw_json: &Value,
) -> WorkerResult<()> {
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
          AND status IN ('queued', 'submitted', 'completing')
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
        SET response_json = ?,
            updated_at = ?
        WHERE id = ?
          AND status IN ('queued', 'submitted', 'completing')
        "#,
        vec![
            json!(json!({
                "pollAttempt": attempt,
                "response": raw_json,
            })
            .to_string()),
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
    if response_has_usage_refund_marker(&job.response_json) {
        return Ok(());
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
          provider_job_ids_json,
          request_json,
          response_json
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
          provider_job_ids_json,
          request_json,
          response_json
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
          response_json
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
    let bytes = response.bytes().await?;
    if bytes.is_empty() {
        return Err(Error::RustError(
            "generation_image_download_empty".to_string(),
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
        "/image_url",
        "/url",
        "/result/imageUrl",
        "/imageUrl",
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

fn submission_arguments_from_request(job_id: &str, request_json: &Value) -> WorkerResult<Value> {
    Ok(json!({
        "jobId": json_string_at(request_json, "/jobId").unwrap_or_else(|| job_id.to_string()),
        "batchId": required_json_string(request_json, "/batchId")?,
        "cloneId": required_json_string(request_json, "/cloneId")?,
        "userId": required_json_string(request_json, "/userId")?,
        "idempotencyKey": required_json_string(request_json, "/idempotencyKey")?,
        "providerSoulId": required_json_string(request_json, "/providerSoulId")?,
        "inputImageUrl": required_json_string(request_json, "/inputImageUrl")?,
        "prompt": json_string_at(request_json, "/prompt").unwrap_or_default(),
    }))
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

fn response_has_usage_refund_marker(response_json: &str) -> bool {
    serde_json::from_str::<Value>(response_json)
        .ok()
        .and_then(|value| json_string_at(&value, "/usageRefundedAt"))
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

fn poll_failure_action(attempt: u8, max_attempts: u8) -> PollFailureAction {
    if attempt >= max_attempts {
        PollFailureAction::Fail
    } else {
        PollFailureAction::Retry(attempt.saturating_add(1))
    }
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

#[cfg(test)]
mod tests {
    use super::{
        deterministic_generation_job_id, final_image_url, generation_media_id,
        generation_output_id, poll_failure_action, provider_asset_id, provider_ids_are_empty,
        provider_job_ids, provider_status, response_has_usage_refund_marker,
        submission_arguments_from_request, usage_date_from_request_json, PollFailureAction,
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
    fn empty_provider_ids_trigger_submission_retry_decision() {
        assert!(provider_ids_are_empty(&json!([])));
        assert!(provider_ids_are_empty(&json!([""])));
        assert!(!provider_ids_are_empty(&json!(["hf_job_1"])));
        assert!(provider_ids_are_empty(&json!({})));
    }

    #[test]
    fn submission_arguments_are_rebuilt_from_request_json() {
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
            "prompt": ""
        });

        assert_eq!(
            submission_arguments_from_request("fallback", &request).unwrap(),
            json!({
                "jobId": "gen_1",
                "batchId": "batch_1",
                "cloneId": "clone_1",
                "userId": "user_1",
                "idempotencyKey": "blitz_gen:batch_1:vref_1",
                "providerSoulId": "soul_1",
                "inputImageUrl": "https://cdn.example/input.jpg",
                "prompt": ""
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
