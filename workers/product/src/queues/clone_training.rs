use crate::db;
use crate::providers::higgsfield_auth::{
    refresh_provider_account_access_token, validate_access_token, HiggsfieldAuthError,
};
use crate::providers::higgsfield_mcp::{
    call_tool, extract_provider_soul_id, extract_provider_status, upload_media_files,
    HiggsfieldMcpError, HiggsfieldMcpMediaFile,
};
use crate::queues::messages::CloneTrainingMessage;
use crate::services::provider_accounts::{choose_provider_account, ProviderAccountCandidate};
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;
use worker::{
    D1Database, Env, Error, MessageBatch, MessageBuilder, MessageExt, Result as WorkerResult,
};

const HIGGSFIELD_REFRESH_SECRET_NAME: &str = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER";
const HIGGSFIELD_TRAINING_TOOL_VAR: &str = "HIGGSFIELD_MCP_CLONE_TRAINING_TOOL";
const ACTIVE_LEASE_MINUTES: f64 = 30.0;
const SUBMITTED_LEASE_MINUTES: f64 = 360.0;
const CLONE_TRAINING_POLL_DELAY_SECONDS: u32 = 60;
const MAX_CLONE_TRAINING_POLL_ATTEMPTS: u8 = 90;

#[derive(Debug, Deserialize)]
struct TrainingJobRow {
    status: String,
    provider_account_id: Option<String>,
    provider_job_id: Option<String>,
    response_json: String,
    clone_display_name: String,
}

#[derive(Debug, Deserialize)]
struct CloneTrainingReferenceRow {
    media_asset_id: String,
    storage_key: Option<String>,
    content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProviderLeaseRow {
    provider_account_id: String,
    status: String,
    lease_expires_at: String,
}

#[derive(Debug, PartialEq, Eq)]
enum ProviderLeaseReservation {
    Acquired,
    InProgress,
    Submitted { provider_account_id: String },
}

#[derive(Debug, Error, PartialEq, Eq)]
enum CloneTrainingProviderError {
    #[error("Higgsfield provider refresh token secret is not configured.")]
    HiggsfieldSecretMissing,
    #[error("Higgsfield provider refresh token is invalid or expired.")]
    HiggsfieldRefreshTokenInvalid,
    #[error("Higgsfield MCP clone training tool is not configured.")]
    HiggsfieldMcpToolMissing,
    #[error("No healthy Higgsfield provider account is available.")]
    HiggsfieldProviderAccountUnavailable,
}

impl CloneTrainingProviderError {
    fn code(&self) -> &'static str {
        match self {
            Self::HiggsfieldSecretMissing => "higgsfield_secret_missing",
            Self::HiggsfieldRefreshTokenInvalid => "higgsfield_refresh_token_invalid",
            Self::HiggsfieldMcpToolMissing => "higgsfield_mcp_tool_missing",
            Self::HiggsfieldProviderAccountUnavailable => "higgsfield_provider_account_unavailable",
        }
    }
}

pub async fn handle_batch(batch: MessageBatch<Value>, env: Env) -> WorkerResult<()> {
    let db = env.d1("DB")?;

    for raw_message in batch.raw_iter() {
        let body = match serde_wasm_bindgen::from_value::<CloneTrainingMessage>(raw_message.body())
        {
            Ok(body) => body,
            Err(error) => {
                web_sys::console::error_1(
                    &format!("failed to deserialize clone training queue message: {error:?}")
                        .into(),
                );
                raw_message.ack();
                continue;
            }
        };

        let result = match body {
            CloneTrainingMessage::SubmitCloneTraining {
                job_id,
                clone_id,
                user_id,
                idempotency_key,
            } => {
                handle_clone_training_message(
                    &db,
                    &env,
                    &job_id,
                    &clone_id,
                    &user_id,
                    &idempotency_key,
                )
                .await
            }
            CloneTrainingMessage::PollCloneTraining {
                job_id,
                clone_id,
                user_id,
                idempotency_key,
                provider_soul_id,
                attempt,
                max_attempts,
            } => {
                poll_clone_training_message(
                    &db,
                    &env,
                    &job_id,
                    &clone_id,
                    &user_id,
                    &idempotency_key,
                    &provider_soul_id,
                    attempt,
                    max_attempts,
                )
                .await
            }
        };

        match result {
            Ok(()) => raw_message.ack(),
            Err(error) => {
                web_sys::console::error_1(
                    &format!("clone training queue message failed: {error:?}").into(),
                );
                raw_message.retry();
            }
        }
    }

    Ok(())
}

async fn handle_clone_training_message(
    db: &D1Database,
    env: &Env,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
) -> WorkerResult<()> {
    let job = db::first::<TrainingJobRow>(
        db,
        r#"
        SELECT
          stj.status,
          stj.provider_account_id,
          stj.provider_job_id,
          stj.response_json,
          cp.display_name AS clone_display_name
        FROM soul_training_jobs stj
        INNER JOIN clone_profiles cp
          ON cp.id = stj.clone_id
         AND cp.user_id = stj.user_id
        WHERE stj.id = ?
          AND stj.user_id = ?
          AND stj.clone_id = ?
          AND stj.idempotency_key = ?
        "#,
        vec![
            json!(job_id),
            json!(user_id),
            json!(clone_id),
            json!(idempotency_key),
        ],
    )
    .await?;

    let Some(job) = job else {
        return Ok(());
    };

    if !matches!(job.status.as_str(), "queued" | "training") {
        return Ok(());
    }
    if has_provider_submission(&job) {
        let provider_soul_id = job
            .provider_job_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                Error::RustError("clone_training_provider_soul_id_missing".to_string())
            })?;
        enqueue_clone_training_poll(
            env,
            job_id,
            clone_id,
            user_id,
            idempotency_key,
            provider_soul_id,
            1,
            MAX_CLONE_TRAINING_POLL_ATTEMPTS,
        )
        .await?;
        return Ok(());
    }

    if job.status == "queued"
        && !claim_training_job(db, job_id, clone_id, user_id, idempotency_key).await?
    {
        return Ok(());
    }
    ensure_clone_training_status(db, clone_id, user_id).await?;

    let tool_name = match env.var(HIGGSFIELD_TRAINING_TOOL_VAR) {
        Ok(tool_name) if !tool_name.to_string().trim().is_empty() => tool_name.to_string(),
        _ => {
            mark_provider_action_required(
                db,
                job_id,
                clone_id,
                user_id,
                idempotency_key,
                &CloneTrainingProviderError::HiggsfieldMcpToolMissing,
            )
            .await?;
            return Ok(());
        }
    };

    let candidates = load_provider_candidates(db).await?;
    let provider_account = choose_provider_account(&candidates).cloned();
    let Some(provider_account) = provider_account else {
        mark_provider_action_required(
            db,
            job_id,
            clone_id,
            user_id,
            idempotency_key,
            &CloneTrainingProviderError::HiggsfieldProviderAccountUnavailable,
        )
        .await?;
        return Ok(());
    };

    let token = match refresh_provider_account_access_token(
        db,
        env,
        &provider_account.id,
        HIGGSFIELD_REFRESH_SECRET_NAME,
    )
    .await
    {
        Ok(token) => token,
        Err(error) => {
            let Some(provider_error) = higgsfield_auth_provider_action_error(&error) else {
                return Err(Error::RustError(error.to_string()));
            };
            mark_provider_action_required(
                db,
                job_id,
                clone_id,
                user_id,
                idempotency_key,
                &provider_error,
            )
            .await?;
            return Ok(());
        }
    };

    validate_access_token(&token.access_token)
        .await
        .map_err(|error| Error::RustError(error.to_string()))?;

    match reserve_provider_lease(db, &provider_account.id, job_id).await? {
        ProviderLeaseReservation::Acquired => {}
        ProviderLeaseReservation::Submitted {
            provider_account_id,
        } => {
            record_provider_submission_marker(
                db,
                job_id,
                clone_id,
                user_id,
                idempotency_key,
                &provider_account_id,
            )
            .await?;
            return Ok(());
        }
        ProviderLeaseReservation::InProgress => {
            return Err(Error::RustError(
                "clone_training_provider_submission_in_progress".to_string(),
            ));
        }
    }

    let uploaded_images = match load_and_upload_training_references(
        env,
        &token.access_token,
        clone_id,
        user_id,
    )
    .await
    {
        Ok(images) => images,
        Err(error) => {
            release_provider_lease(db, job_id).await?;
            return Err(error);
        }
    };
    let result = match call_tool(
        &token.access_token,
        json!(job_id),
        &tool_name,
        clone_training_submission_arguments(&job.clone_display_name, uploaded_images),
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            release_provider_lease(db, job_id).await?;
            return Err(map_mcp_error(error));
        }
    };

    let provider_soul_id = match extract_provider_soul_id(&result.raw_json) {
        Some(provider_soul_id) => provider_soul_id,
        None => {
            release_provider_lease(db, job_id).await?;
            return Err(Error::RustError(
                "clone_training_provider_soul_id_missing".to_string(),
            ));
        }
    };

    if let Err(error) = record_provider_submission(
        db,
        job_id,
        clone_id,
        user_id,
        idempotency_key,
        &provider_account.id,
        &provider_soul_id,
        &result.raw_json,
    )
    .await
    {
        mark_provider_lease_submitted(db, job_id).await?;
        return Err(error);
    }

    mark_provider_lease_submitted(db, job_id).await?;
    enqueue_clone_training_poll(
        env,
        job_id,
        clone_id,
        user_id,
        idempotency_key,
        &provider_soul_id,
        1,
        MAX_CLONE_TRAINING_POLL_ATTEMPTS,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn poll_clone_training_message(
    db: &D1Database,
    env: &Env,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    provider_soul_id: &str,
    attempt: u8,
    max_attempts: u8,
) -> WorkerResult<()> {
    let Some(job) =
        load_training_job_for_poll(db, job_id, clone_id, user_id, idempotency_key).await?
    else {
        return Ok(());
    };
    if job.status == "completed" {
        ensure_clone_training_ready(db, clone_id, user_id, provider_soul_id, &job.response_json)
            .await?;
        return Ok(());
    }
    if !matches!(job.status.as_str(), "training" | "queued") {
        return Ok(());
    }

    if attempt > max_attempts {
        fail_clone_training_job(
            db,
            job_id,
            clone_id,
            user_id,
            idempotency_key,
            "clone_training_poll_exhausted",
            "Clone training polling exhausted before the provider reported ready.",
            &json!({ "providerSoulId": provider_soul_id }),
        )
        .await?;
        release_provider_lease(db, job_id).await?;
        return Ok(());
    }

    let tool_name = match env.var(HIGGSFIELD_TRAINING_TOOL_VAR) {
        Ok(tool_name) if !tool_name.to_string().trim().is_empty() => tool_name.to_string(),
        _ => {
            fail_clone_training_job(
                db,
                job_id,
                clone_id,
                user_id,
                idempotency_key,
                "higgsfield_mcp_tool_missing",
                "Higgsfield MCP clone training tool is not configured.",
                &json!({ "providerSoulId": provider_soul_id }),
            )
            .await?;
            release_provider_lease(db, job_id).await?;
            return Ok(());
        }
    };

    let Some(provider_account_id) = job
        .provider_account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return retry_clone_training_poll_after_error(
            db,
            env,
            job_id,
            clone_id,
            user_id,
            idempotency_key,
            provider_soul_id,
            attempt,
            max_attempts,
            "clone_training_poll_provider_account_missing",
            "Clone training poll is missing provider account state.",
        )
        .await;
    };
    let token = match refresh_provider_account_access_token(
        db,
        env,
        provider_account_id,
        HIGGSFIELD_REFRESH_SECRET_NAME,
    )
    .await
    {
        Ok(token) => token,
        Err(error) => {
            return retry_clone_training_poll_after_error(
                db,
                env,
                job_id,
                clone_id,
                user_id,
                idempotency_key,
                provider_soul_id,
                attempt,
                max_attempts,
                "clone_training_poll_auth_failed",
                &error.to_string(),
            )
            .await;
        }
    };
    if let Err(error) = validate_access_token(&token.access_token).await {
        return retry_clone_training_poll_after_error(
            db,
            env,
            job_id,
            clone_id,
            user_id,
            idempotency_key,
            provider_soul_id,
            attempt,
            max_attempts,
            "clone_training_poll_auth_failed",
            &error.to_string(),
        )
        .await;
    }

    let result = match call_tool(
        &token.access_token,
        json!(format!("status:{job_id}:{attempt}")),
        &tool_name,
        clone_training_status_arguments(provider_soul_id),
    )
    .await
    {
        Ok(result) => result,
        Err(error) => {
            return retry_clone_training_poll_after_error(
                db,
                env,
                job_id,
                clone_id,
                user_id,
                idempotency_key,
                provider_soul_id,
                attempt,
                max_attempts,
                "clone_training_poll_failed",
                &map_mcp_error(error).to_string(),
            )
            .await;
        }
    };

    if provider_training_ready(&result.raw_json) {
        let ready_soul_id = extract_provider_soul_id(&result.raw_json)
            .unwrap_or_else(|| provider_soul_id.to_string());
        complete_clone_training_job(
            db,
            job_id,
            clone_id,
            user_id,
            idempotency_key,
            &ready_soul_id,
            &result.raw_json,
        )
        .await?;
        release_provider_lease(db, job_id).await?;
        return Ok(());
    }

    if provider_training_failed(&result.raw_json) {
        fail_clone_training_job(
            db,
            job_id,
            clone_id,
            user_id,
            idempotency_key,
            "clone_training_failed",
            "Clone training provider returned a terminal failure.",
            &result.raw_json,
        )
        .await?;
        release_provider_lease(db, job_id).await?;
        return Ok(());
    }

    record_training_poll_attempt(db, job_id, attempt, &result.raw_json).await?;
    enqueue_clone_training_poll(
        env,
        job_id,
        clone_id,
        user_id,
        idempotency_key,
        provider_soul_id,
        attempt.saturating_add(1),
        max_attempts,
    )
    .await
}

fn has_provider_submission(job: &TrainingJobRow) -> bool {
    job.provider_job_id
        .as_deref()
        .is_some_and(|provider_job_id| !provider_job_id.trim().is_empty())
        || job.response_json.trim() != "{}"
}

async fn claim_training_job(
    db: &D1Database,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
) -> WorkerResult<bool> {
    let now = now_iso_string();
    let result = db::run(
        db,
        r#"
        UPDATE soul_training_jobs
        SET status = 'training',
            started_at = COALESCE(started_at, ?),
            error_code = NULL,
            error_message = NULL,
            updated_at = ?
        WHERE id = ?
          AND user_id = ?
          AND clone_id = ?
          AND idempotency_key = ?
          AND status = 'queued'
        "#,
        vec![
            json!(now),
            json!(now),
            json!(job_id),
            json!(user_id),
            json!(clone_id),
            json!(idempotency_key),
        ],
    )
    .await?;

    Ok(changed_rows(&result)? > 0)
}

async fn ensure_clone_training_status(
    db: &D1Database,
    clone_id: &str,
    user_id: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE clone_profiles
        SET soul_status = 'training',
            updated_at = ?
        WHERE id = ?
          AND user_id = ?
          AND soul_status IN ('queued', 'training')
        "#,
        vec![json!(now), json!(clone_id), json!(user_id)],
    )
    .await
}

async fn load_provider_candidates(db: &D1Database) -> WorkerResult<Vec<ProviderAccountCandidate>> {
    let now = now_iso_string();
    db::all::<ProviderAccountCandidate>(
        db,
        r#"
        SELECT
          pa.id AS id,
          pa.health_state AS health_state,
          COUNT(pal.id) AS active_leases,
          COALESCE(CAST(json_extract(pa.capacity_json, '$.maxLeases') AS INTEGER), 1) AS max_leases
        FROM provider_accounts pa
        LEFT JOIN provider_account_leases pal
          ON pal.provider_account_id = pa.id
         AND pal.status IN ('active', 'submitted')
         AND pal.released_at IS NULL
         AND pal.lease_expires_at > ?
        WHERE pa.provider = 'higgsfield'
          AND pa.disabled_at IS NULL
        GROUP BY pa.id, pa.health_state, pa.capacity_json
        "#,
        vec![json!(now)],
    )
    .await
}

async fn reserve_provider_lease(
    db: &D1Database,
    provider_account_id: &str,
    job_id: &str,
) -> WorkerResult<ProviderLeaseReservation> {
    let now = now_iso_string();
    let lease_expires_at = lease_expires_at_iso(ACTIVE_LEASE_MINUTES);
    let result = db::run(
        db,
        r#"
        INSERT OR IGNORE INTO provider_account_leases (
          id,
          provider_account_id,
          job_type,
          job_id,
          status,
          lease_expires_at,
          created_at
        )
        VALUES (?, ?, 'clone_training', ?, 'active', ?, ?)
        "#,
        vec![
            json!(format!("lease_{}", uuid::Uuid::new_v4().simple())),
            json!(provider_account_id),
            json!(job_id),
            json!(lease_expires_at),
            json!(now),
        ],
    )
    .await?;

    if changed_rows(&result)? > 0 {
        return Ok(ProviderLeaseReservation::Acquired);
    }

    let lease = db::first::<ProviderLeaseRow>(
        db,
        r#"
        SELECT provider_account_id, status, lease_expires_at
        FROM provider_account_leases
        WHERE job_type = 'clone_training'
          AND job_id = ?
        "#,
        vec![json!(job_id)],
    )
    .await?;

    let Some(lease) = lease else {
        return Ok(ProviderLeaseReservation::InProgress);
    };
    if lease.status == "submitted" {
        return Ok(ProviderLeaseReservation::Submitted {
            provider_account_id: lease.provider_account_id,
        });
    }
    if lease.status == "active" && !lease_is_expired(&lease.lease_expires_at, &now) {
        return Ok(ProviderLeaseReservation::InProgress);
    }

    let result = db::run(
        db,
        r#"
        UPDATE provider_account_leases
        SET provider_account_id = ?,
            status = 'active',
            lease_expires_at = ?,
            released_at = NULL
        WHERE job_type = 'clone_training'
          AND job_id = ?
          AND (
            status = 'released'
            OR lease_expires_at <= ?
          )
        "#,
        vec![
            json!(provider_account_id),
            json!(lease_expires_at),
            json!(job_id),
            json!(now),
        ],
    )
    .await?;

    if changed_rows(&result)? > 0 {
        Ok(ProviderLeaseReservation::Acquired)
    } else {
        Ok(ProviderLeaseReservation::InProgress)
    }
}

async fn record_provider_submission(
    db: &D1Database,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    provider_account_id: &str,
    provider_soul_id: &str,
    raw_json: &Value,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let response_json = raw_json.to_string();
    let result = db::run(
        db,
        r#"
        UPDATE soul_training_jobs
        SET provider_account_id = ?,
            provider_job_id = ?,
            response_json = ?,
            updated_at = ?
        WHERE id = ?
          AND user_id = ?
          AND clone_id = ?
          AND idempotency_key = ?
          AND status = 'training'
        "#,
        vec![
            json!(provider_account_id),
            json!(provider_soul_id),
            json!(response_json),
            json!(now),
            json!(job_id),
            json!(user_id),
            json!(clone_id),
            json!(idempotency_key),
        ],
    )
    .await?;

    if changed_rows(&result)? == 0 {
        return Err(Error::RustError(
            "clone_training_submission_record_stale".to_string(),
        ));
    }

    Ok(())
}

async fn record_provider_submission_marker(
    db: &D1Database,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    provider_account_id: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE soul_training_jobs
        SET provider_account_id = COALESCE(provider_account_id, ?),
            response_json = CASE
              WHEN response_json = '{}' THEN ?
              ELSE response_json
            END,
            updated_at = ?
        WHERE id = ?
          AND user_id = ?
          AND clone_id = ?
          AND idempotency_key = ?
          AND status = 'training'
        "#,
        vec![
            json!(provider_account_id),
            json!(json!({
                "providerSubmission": "submitted",
                "source": "provider_lease",
            })
            .to_string()),
            json!(now),
            json!(job_id),
            json!(user_id),
            json!(clone_id),
            json!(idempotency_key),
        ],
    )
    .await
}

async fn release_provider_lease(db: &D1Database, job_id: &str) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE provider_account_leases
        SET status = 'released',
            released_at = ?
        WHERE job_type = 'clone_training'
          AND job_id = ?
          AND status IN ('active', 'submitted')
        "#,
        vec![json!(now), json!(job_id)],
    )
    .await
}

async fn mark_provider_lease_submitted(db: &D1Database, job_id: &str) -> WorkerResult<()> {
    let lease_expires_at = lease_expires_at_iso(SUBMITTED_LEASE_MINUTES);
    db::exec(
        db,
        r#"
        UPDATE provider_account_leases
        SET status = 'submitted',
            lease_expires_at = ?
        WHERE job_type = 'clone_training'
          AND job_id = ?
          AND status = 'active'
        "#,
        vec![json!(lease_expires_at), json!(job_id)],
    )
    .await
}

async fn load_training_job_for_poll(
    db: &D1Database,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
) -> WorkerResult<Option<TrainingJobRow>> {
    db::first::<TrainingJobRow>(
        db,
        r#"
        SELECT
          stj.status,
          stj.provider_account_id,
          stj.provider_job_id,
          stj.response_json,
          cp.display_name AS clone_display_name
        FROM soul_training_jobs stj
        INNER JOIN clone_profiles cp
          ON cp.id = stj.clone_id
         AND cp.user_id = stj.user_id
        WHERE stj.id = ?
          AND stj.user_id = ?
          AND stj.clone_id = ?
          AND stj.idempotency_key = ?
        "#,
        vec![
            json!(job_id),
            json!(user_id),
            json!(clone_id),
            json!(idempotency_key),
        ],
    )
    .await
}

async fn load_and_upload_training_references(
    env: &Env,
    access_token: &str,
    clone_id: &str,
    user_id: &str,
) -> WorkerResult<Vec<String>> {
    let references = load_training_references(&env.d1("DB")?, clone_id, user_id).await?;
    if references.is_empty() {
        return Err(Error::RustError(
            "clone_training_references_missing".to_string(),
        ));
    }

    let mut files = Vec::with_capacity(references.len());
    for reference in references {
        let storage_key = reference
            .storage_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                Error::RustError("clone_training_reference_storage_missing".to_string())
            })?;
        let content_type = reference
            .content_type
            .as_deref()
            .map(normalize_training_content_type)
            .unwrap_or("image/jpeg")
            .to_string();
        let bytes = read_media_object_bytes(env, storage_key).await?;
        files.push(HiggsfieldMcpMediaFile {
            filename: training_upload_filename(&reference.media_asset_id, &content_type),
            content_type,
            bytes,
        });
    }

    let uploaded = upload_media_files(access_token, &files)
        .await
        .map_err(map_mcp_error)?;
    Ok(uploaded.into_iter().map(|media| media.url).collect())
}

async fn load_training_references(
    db: &D1Database,
    clone_id: &str,
    user_id: &str,
) -> WorkerResult<Vec<CloneTrainingReferenceRow>> {
    db::all::<CloneTrainingReferenceRow>(
        db,
        r#"
        SELECT
          ma.id AS media_asset_id,
          ma.storage_key,
          ma.content_type
        FROM clone_reference_assets cra
        INNER JOIN media_assets ma
          ON ma.id = cra.media_asset_id
         AND ma.deleted_at IS NULL
        WHERE cra.clone_id = ?
          AND cra.user_id = ?
          AND cra.training_selected = 1
          AND cra.eligibility_status = 'accepted'
        ORDER BY cra.sort_order ASC
        "#,
        vec![json!(clone_id), json!(user_id)],
    )
    .await
}

async fn read_media_object_bytes(env: &Env, storage_key: &str) -> WorkerResult<Vec<u8>> {
    let object = env
        .bucket("MEDIA")?
        .get(storage_key.to_string())
        .execute()
        .await?
        .ok_or_else(|| Error::RustError("clone_training_media_missing".to_string()))?;
    let body = object
        .body()
        .ok_or_else(|| Error::RustError("clone_training_media_body_missing".to_string()))?;
    body.bytes().await
}

async fn complete_clone_training_job(
    db: &D1Database,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    provider_soul_id: &str,
    raw_json: &Value,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let response_json = raw_json.to_string();
    db::batch(
        db,
        vec![
            (
                r#"
                UPDATE soul_training_jobs
                SET status = 'completed',
                    provider_job_id = ?,
                    response_json = ?,
                    completed_at = COALESCE(completed_at, ?),
                    updated_at = ?
                WHERE id = ?
                  AND user_id = ?
                  AND clone_id = ?
                  AND idempotency_key = ?
                  AND status IN ('queued', 'training')
                "#,
                vec![
                    json!(provider_soul_id),
                    json!(response_json),
                    json!(now),
                    json!(now),
                    json!(job_id),
                    json!(user_id),
                    json!(clone_id),
                    json!(idempotency_key),
                ],
            ),
            (
                r#"
                UPDATE clone_profiles
                SET soul_status = 'ready',
                    provider_soul_id = ?,
                    updated_at = ?
                WHERE id = ?
                  AND user_id = ?
                  AND soul_status IN ('queued', 'training', 'provider_action_required')
                "#,
                vec![
                    json!(provider_soul_id),
                    json!(now),
                    json!(clone_id),
                    json!(user_id),
                ],
            ),
        ],
    )
    .await?;
    Ok(())
}

async fn ensure_clone_training_ready(
    db: &D1Database,
    clone_id: &str,
    user_id: &str,
    provider_soul_id: &str,
    response_json: &str,
) -> WorkerResult<()> {
    let raw_json = serde_json::from_str::<Value>(response_json).unwrap_or_else(|_| json!({}));
    let soul_id =
        extract_provider_soul_id(&raw_json).unwrap_or_else(|| provider_soul_id.to_string());
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE clone_profiles
        SET soul_status = 'ready',
            provider_soul_id = ?,
            updated_at = ?
        WHERE id = ?
          AND user_id = ?
          AND soul_status != 'ready'
        "#,
        vec![json!(soul_id), json!(now), json!(clone_id), json!(user_id)],
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn fail_clone_training_job(
    db: &D1Database,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    error_code: &str,
    error_message: &str,
    raw_json: &Value,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let response_json = raw_json.to_string();
    db::batch(
        db,
        vec![
            (
                r#"
                UPDATE soul_training_jobs
                SET status = 'failed',
                    response_json = ?,
                    error_code = ?,
                    error_message = ?,
                    completed_at = COALESCE(completed_at, ?),
                    updated_at = ?
                WHERE id = ?
                  AND user_id = ?
                  AND clone_id = ?
                  AND idempotency_key = ?
                  AND status IN ('queued', 'training')
                "#,
                vec![
                    json!(response_json),
                    json!(error_code),
                    json!(error_message),
                    json!(now),
                    json!(now),
                    json!(job_id),
                    json!(user_id),
                    json!(clone_id),
                    json!(idempotency_key),
                ],
            ),
            (
                r#"
                UPDATE clone_profiles
                SET soul_status = 'failed',
                    updated_at = ?
                WHERE id = ?
                  AND user_id = ?
                  AND soul_status IN ('queued', 'training', 'provider_action_required')
                "#,
                vec![json!(now), json!(clone_id), json!(user_id)],
            ),
        ],
    )
    .await?;
    Ok(())
}

async fn record_training_poll_attempt(
    db: &D1Database,
    job_id: &str,
    attempt: u8,
    raw_json: &Value,
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE soul_training_jobs
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
          AND status = 'training'
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

#[allow(clippy::too_many_arguments)]
async fn retry_clone_training_poll_after_error(
    db: &D1Database,
    env: &Env,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    provider_soul_id: &str,
    attempt: u8,
    max_attempts: u8,
    error_code: &str,
    error_message: &str,
) -> WorkerResult<()> {
    let raw_json = json!({
        "errorCode": error_code,
        "errorMessage": error_message,
        "providerSoulId": provider_soul_id,
    });
    if attempt >= max_attempts {
        fail_clone_training_job(
            db,
            job_id,
            clone_id,
            user_id,
            idempotency_key,
            error_code,
            error_message,
            &raw_json,
        )
        .await?;
        release_provider_lease(db, job_id).await?;
        return Ok(());
    }

    record_training_poll_attempt(db, job_id, attempt, &raw_json).await?;
    enqueue_clone_training_poll(
        env,
        job_id,
        clone_id,
        user_id,
        idempotency_key,
        provider_soul_id,
        attempt.saturating_add(1),
        max_attempts,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn enqueue_clone_training_poll(
    env: &Env,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    provider_soul_id: &str,
    attempt: u8,
    max_attempts: u8,
) -> WorkerResult<()> {
    env.queue("CLONE_TRAINING_QUEUE")?
        .send(
            MessageBuilder::new(CloneTrainingMessage::PollCloneTraining {
                job_id: job_id.to_string(),
                clone_id: clone_id.to_string(),
                user_id: user_id.to_string(),
                idempotency_key: idempotency_key.to_string(),
                provider_soul_id: provider_soul_id.to_string(),
                attempt,
                max_attempts,
            })
            .delay_seconds(CLONE_TRAINING_POLL_DELAY_SECONDS)
            .build(),
        )
        .await
}

fn clone_training_submission_arguments(name: &str, images: Vec<String>) -> Value {
    json!({
        "action": "train",
        "name": name,
        "images": images,
    })
}

fn clone_training_status_arguments(provider_soul_id: &str) -> Value {
    json!({
        "action": "status",
        "soul_id": provider_soul_id,
    })
}

fn provider_training_ready(raw_json: &Value) -> bool {
    extract_provider_status(raw_json).is_some_and(|status| {
        matches!(
            status.as_str(),
            "ready" | "completed" | "complete" | "succeeded" | "success"
        )
    })
}

fn provider_training_failed(raw_json: &Value) -> bool {
    extract_provider_status(raw_json).is_some_and(|status| {
        matches!(
            status.as_str(),
            "failed" | "failure" | "error" | "errored" | "canceled" | "cancelled"
        )
    })
}

fn normalize_training_content_type(content_type: &str) -> &'static str {
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

fn training_upload_filename(media_asset_id: &str, content_type: &str) -> String {
    let extension = match normalize_training_content_type(content_type) {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/heic" => "heic",
        _ => "jpg",
    };
    format!("{media_asset_id}.{extension}")
}

async fn mark_provider_action_required(
    db: &D1Database,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    error: &CloneTrainingProviderError,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let message = error.to_string();

    let job_result = db::run(
        db,
        r#"
        UPDATE soul_training_jobs
        SET status = 'provider_action_required',
            error_code = ?,
            error_message = ?,
            updated_at = ?
        WHERE id = ?
          AND user_id = ?
          AND clone_id = ?
          AND idempotency_key = ?
          AND status IN ('queued', 'training')
        "#,
        vec![
            json!(error.code()),
            json!(message),
            json!(now),
            json!(job_id),
            json!(user_id),
            json!(clone_id),
            json!(idempotency_key),
        ],
    )
    .await?;

    if changed_rows(&job_result)? == 0 {
        return Ok(());
    }

    db::exec(
        db,
        r#"
        UPDATE clone_profiles
        SET soul_status = 'provider_action_required',
            updated_at = ?
        WHERE id = ?
          AND user_id = ?
          AND soul_status IN ('queued', 'training', 'provider_action_required')
        "#,
        vec![json!(now), json!(clone_id), json!(user_id)],
    )
    .await
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

fn higgsfield_auth_provider_action_error(
    error: &HiggsfieldAuthError,
) -> Option<CloneTrainingProviderError> {
    match error {
        HiggsfieldAuthError::MissingSecret { .. } => {
            Some(CloneTrainingProviderError::HiggsfieldSecretMissing)
        }
        HiggsfieldAuthError::HttpStatus { status } if matches!(*status, 401 | 422) => {
            Some(CloneTrainingProviderError::HiggsfieldRefreshTokenInvalid)
        }
        _ => None,
    }
}

fn lease_is_expired(lease_expires_at: &str, now: &str) -> bool {
    lease_expires_at <= now
}

fn lease_expires_at_iso(minutes_from_now: f64) -> String {
    let now = js_sys::Date::new_0().get_time();
    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(
        now + minutes_from_now * 60_000.0,
    ))
    .to_iso_string()
    .into()
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

#[cfg(test)]
mod tests {
    use super::{
        clone_training_status_arguments, clone_training_submission_arguments,
        has_provider_submission, higgsfield_auth_provider_action_error, lease_is_expired,
        CloneTrainingProviderError, TrainingJobRow,
    };
    use crate::providers::higgsfield_auth::HiggsfieldAuthError;
    use serde_json::json;

    fn job(provider_job_id: Option<&str>, response_json: &str) -> TrainingJobRow {
        TrainingJobRow {
            status: "training".to_string(),
            provider_account_id: Some("pa_higgsfield_founder".to_string()),
            provider_job_id: provider_job_id.map(ToString::to_string),
            response_json: response_json.to_string(),
            clone_display_name: "Maya".to_string(),
        }
    }

    #[test]
    fn provider_submission_is_present_when_provider_job_id_exists() {
        assert!(has_provider_submission(&job(Some("hf_job_1"), "{}")));
        assert!(!has_provider_submission(&job(Some("   "), "{}")));
    }

    #[test]
    fn provider_submission_is_present_when_response_json_is_recorded() {
        assert!(has_provider_submission(&job(
            None,
            r#"{"rawText":"accepted"}"#
        )));
        assert!(!has_provider_submission(&job(None, "{}")));
    }

    #[test]
    fn lease_expiry_uses_iso_timestamp_ordering() {
        assert!(lease_is_expired(
            "2026-05-08T12:00:00.000Z",
            "2026-05-08T12:00:00.000Z"
        ));
        assert!(lease_is_expired(
            "2026-05-08T11:59:59.999Z",
            "2026-05-08T12:00:00.000Z"
        ));
        assert!(!lease_is_expired(
            "2026-05-08T12:00:00.001Z",
            "2026-05-08T12:00:00.000Z"
        ));
    }

    #[test]
    fn clone_training_submission_arguments_match_show_characters_train_payload() {
        assert_eq!(
            clone_training_submission_arguments(
                "Maya",
                vec![
                    "https://higgsfield.example/maya-01.png".to_string(),
                    "https://higgsfield.example/maya-02.png".to_string(),
                ],
            ),
            json!({
                "action": "train",
                "name": "Maya",
                "images": [
                    "https://higgsfield.example/maya-01.png",
                    "https://higgsfield.example/maya-02.png"
                ]
            })
        );
    }

    #[test]
    fn clone_training_status_arguments_match_show_characters_status_payload() {
        assert_eq!(
            clone_training_status_arguments("soul_1"),
            json!({
                "action": "status",
                "soul_id": "soul_1"
            })
        );
    }

    #[test]
    fn refresh_token_rejection_requires_provider_action_instead_of_queue_retry() {
        assert_eq!(
            higgsfield_auth_provider_action_error(&HiggsfieldAuthError::HttpStatus { status: 401 }),
            Some(CloneTrainingProviderError::HiggsfieldRefreshTokenInvalid)
        );
        assert_eq!(
            higgsfield_auth_provider_action_error(&HiggsfieldAuthError::HttpStatus { status: 422 }),
            Some(CloneTrainingProviderError::HiggsfieldRefreshTokenInvalid)
        );
        assert_eq!(
            higgsfield_auth_provider_action_error(&HiggsfieldAuthError::HttpStatus { status: 500 }),
            None
        );
    }
}
