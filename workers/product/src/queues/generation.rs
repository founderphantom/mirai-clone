use crate::db;
use crate::providers::higgsfield_auth::{refresh_access_token, validate_access_token};
use crate::providers::higgsfield_mcp::{call_tool, HiggsfieldMcpError};
use crate::queues::messages::GenerationMessage;
use crate::services::generation_usage::{refund_image, reserve_image};
use crate::services::media::media_storage_key;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;
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
    user_id: String,
    clone_id: String,
    blitz_batch_id: Option<String>,
    input_visual_reference_id: Option<String>,
    status: String,
    provider_job_ids_json: String,
    request_json: String,
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
        return Ok(());
    };
    let (free_daily_limit, pro_daily_limit) = load_generation_limits(db).await?;

    for visual_reference_id in visual_reference_ids {
        if existing_batch_reference_job(db, batch_id, visual_reference_id).await? {
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

        if !reserve_image(db, user_id, &clone.plan, free_daily_limit, pro_daily_limit).await? {
            continue;
        }

        let job_id = format!("gen_{}", Uuid::new_v4().simple());
        let request_json = json!({
            "idempotencyKey": idempotency_key,
            "providerSoulId": provider_soul_id,
            "visualReferenceId": visual_reference_id,
        });
        insert_generation_job(
            db,
            &job_id,
            user_id,
            clone_id,
            batch_id,
            visual_reference_id,
            &request_json,
        )
        .await?;

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
    let token = refresh_access_token(env, HIGGSFIELD_REFRESH_SECRET_NAME)
        .await
        .map_err(|error| Error::RustError(error.to_string()))?;
    let validation = validate_access_token(&token.access_token)
        .await
        .map_err(|error| Error::RustError(error.to_string()))?;
    if !validation.valid {
        return Err(Error::RustError(
            "higgsfield_generation_token_invalid".to_string(),
        ));
    }

    mark_generation_job_submitting(db, job_id).await?;

    let result = call_tool(
        &token.access_token,
        json!(job_id),
        &tool_name,
        json!({
            "jobId": job_id,
            "batchId": batch_id,
            "cloneId": clone_id,
            "userId": user_id,
            "idempotencyKey": format!("{idempotency_key}:{visual_reference_id}"),
            "providerSoulId": provider_soul_id,
            "inputImageUrl": materialized_reference_url,
            "prompt": "",
        }),
    )
    .await
    .map_err(map_mcp_error)?;

    if let Some(final_url) = final_image_url(&result.raw_json) {
        complete_generation_job(db, env, job_id, &final_url, &result.raw_json).await?;
        return Ok(());
    }

    record_provider_generation_response(db, job_id, &result.raw_json).await?;
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
            fail_generation_job(db, job_id, "provider_poll_auth_failed", &error.to_string())
                .await?;
            return Ok(());
        }
    };
    let validation = validate_access_token(&token.access_token)
        .await
        .map_err(|error| Error::RustError(error.to_string()))?;
    if !validation.valid {
        fail_generation_job(
            db,
            job_id,
            "provider_poll_token_invalid",
            "Higgsfield provider access token is invalid.",
        )
        .await?;
        return Ok(());
    }

    let provider_job_ids =
        serde_json::from_str::<Value>(&job.provider_job_ids_json).unwrap_or_else(|_| json!([]));
    let original_request =
        serde_json::from_str::<Value>(&job.request_json).unwrap_or_else(|_| json!({}));

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
            record_poll_attempt(db, job_id, attempt, &json!({ "error": error.to_string() }))
                .await?;
            if attempt >= max_attempts {
                fail_generation_job(db, job_id, "generation_poll_exhausted", &error.to_string())
                    .await?;
            } else {
                enqueue_poll(
                    env,
                    job_id,
                    batch_id,
                    attempt.saturating_add(1),
                    max_attempts,
                )
                .await?;
            }
            return Ok(());
        }
    };

    if let Some(final_url) = final_image_url(&result.raw_json) {
        complete_generation_job(db, env, job_id, &final_url, &result.raw_json).await?;
        return Ok(());
    }

    record_poll_attempt(db, job_id, attempt, &result.raw_json).await?;
    if provider_status(&result.raw_json).is_some_and(is_failed_provider_status)
        || attempt >= max_attempts
    {
        fail_generation_job(
            db,
            job_id,
            "generation_failed",
            "Generation provider returned a terminal failure or polling exhausted.",
        )
        .await?;
    } else {
        enqueue_poll(
            env,
            job_id,
            batch_id,
            attempt.saturating_add(1),
            max_attempts,
        )
        .await?;
    }

    Ok(())
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
        mark_batch_ready_if_complete(db, job.blitz_batch_id.as_deref().unwrap_or_default()).await?;
        return Ok(());
    }

    let (bytes, content_type) = download_generated_image(provider_url).await?;
    let media_id = format!("media_{}", Uuid::new_v4().simple());
    let output_id = format!("gout_{}", Uuid::new_v4().simple());
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
        INSERT INTO media_assets (
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
        INSERT INTO generation_outputs (
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
        if let Some(visual_reference_id) = job.input_visual_reference_id.as_deref() {
            db::exec(
                db,
                r#"
                UPDATE visual_references
                SET generation_use_count = generation_use_count + 1,
                    last_used_batch_id = ?
                WHERE id = ?
                "#,
                vec![json!(job.blitz_batch_id), json!(visual_reference_id)],
            )
            .await?;
        }

        if let Some(batch_id) = job.blitz_batch_id.as_deref() {
            db::exec(
                db,
                r#"
                UPDATE blitz_batches
                SET generation_count = generation_count + 1
                WHERE id = ?
                "#,
                vec![json!(batch_id)],
            )
            .await?;
        }
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
    if matches!(job.status.as_str(), "completed" | "failed") {
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
        refund_image(db, &job.user_id).await?;
    }
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

async fn existing_batch_reference_job(
    db: &D1Database,
    batch_id: &str,
    visual_reference_id: &str,
) -> WorkerResult<bool> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM generation_jobs
        WHERE blitz_batch_id = ?
          AND input_visual_reference_id = ?
        "#,
        vec![json!(batch_id), json!(visual_reference_id)],
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
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        INSERT INTO generation_jobs (
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
    .await
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
          AND status IN ('queued', 'submitted')
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

async fn load_generation_job(
    db: &D1Database,
    job_id: &str,
    batch_id: &str,
) -> WorkerResult<Option<GenerationJobRow>> {
    db::first::<GenerationJobRow>(
        db,
        r#"
        SELECT
          user_id,
          clone_id,
          blitz_batch_id,
          input_visual_reference_id,
          status,
          provider_job_ids_json,
          request_json
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
          user_id,
          clone_id,
          blitz_batch_id,
          input_visual_reference_id,
          status,
          provider_job_ids_json,
          request_json
        FROM generation_jobs
        WHERE id = ?
        "#,
        vec![json!(job_id)],
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
    [
        "/result/image_url",
        "/result/url",
        "/image_url",
        "/url",
        "/result/imageUrl",
        "/imageUrl",
    ]
    .into_iter()
    .find_map(|path| json_string_at(raw_json, path))
    .or_else(|| {
        json_string_at(raw_json, "/result/content/0/text")
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| final_image_url(&value))
    })
}

fn provider_job_ids(raw_json: &Value) -> Value {
    let ids = [
        "/result/id",
        "/result/job_id",
        "/result/jobId",
        "/id",
        "/job_id",
        "/jobId",
    ]
    .into_iter()
    .filter_map(|path| json_string_at(raw_json, path))
    .collect::<Vec<_>>();
    json!(ids)
}

fn provider_asset_id(raw_json: &Value) -> Option<String> {
    [
        "/result/asset_id",
        "/result/assetId",
        "/asset_id",
        "/assetId",
    ]
    .into_iter()
    .find_map(|path| json_string_at(raw_json, path))
}

fn provider_status(raw_json: &Value) -> Option<String> {
    ["/result/status", "/result/state", "/status", "/state"]
        .into_iter()
        .find_map(|path| json_string_at(raw_json, path))
        .map(|value| value.trim().to_ascii_lowercase())
}

fn is_failed_provider_status(status: String) -> bool {
    matches!(
        status.as_str(),
        "failed" | "failure" | "error" | "errored" | "canceled" | "cancelled"
    )
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
