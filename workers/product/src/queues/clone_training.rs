use crate::db;
use crate::providers::higgsfield_auth::{
    refresh_access_token, validate_access_token, HiggsfieldAuthError,
};
use crate::providers::higgsfield_mcp::{call_tool, HiggsfieldMcpError};
use crate::queues::messages::CloneTrainingMessage;
use crate::services::provider_accounts::{choose_provider_account, ProviderAccountCandidate};
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;
use worker::{D1Database, Env, Error, MessageBatch, MessageExt, Result as WorkerResult};

const HIGGSFIELD_REFRESH_SECRET_NAME: &str = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FUFU";
const HIGGSFIELD_TRAINING_TOOL_VAR: &str = "HIGGSFIELD_MCP_CLONE_TRAINING_TOOL";

#[derive(Debug, Deserialize)]
struct TrainingJobRow {
    status: String,
    request_json: String,
}

#[derive(Debug, Error)]
enum CloneTrainingProviderError {
    #[error("Higgsfield provider refresh token secret is not configured.")]
    HiggsfieldSecretMissing,
    #[error("Higgsfield MCP clone training tool is not configured.")]
    HiggsfieldMcpToolMissing,
    #[error("No healthy Higgsfield provider account is available.")]
    HiggsfieldProviderAccountUnavailable,
}

impl CloneTrainingProviderError {
    fn code(&self) -> &'static str {
        match self {
            Self::HiggsfieldSecretMissing => "higgsfield_secret_missing",
            Self::HiggsfieldMcpToolMissing => "higgsfield_mcp_tool_missing",
            Self::HiggsfieldProviderAccountUnavailable => "higgsfield_provider_account_unavailable",
        }
    }
}

pub async fn handle_batch(batch: MessageBatch<CloneTrainingMessage>, env: Env) -> WorkerResult<()> {
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
        SELECT status, request_json
        FROM soul_training_jobs
        WHERE id = ?
          AND user_id = ?
          AND clone_id = ?
          AND idempotency_key = ?
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

    if job.status == "queued"
        && !claim_training_job(db, job_id, clone_id, user_id, idempotency_key).await?
    {
        return Ok(());
    }
    ensure_clone_training_status(db, clone_id, user_id).await?;

    if env.secret(HIGGSFIELD_REFRESH_SECRET_NAME).is_err() {
        mark_provider_action_required(
            db,
            job_id,
            clone_id,
            user_id,
            idempotency_key,
            &CloneTrainingProviderError::HiggsfieldSecretMissing,
        )
        .await?;
        return Ok(());
    }

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

    let token = match refresh_access_token(env, HIGGSFIELD_REFRESH_SECRET_NAME).await {
        Ok(token) => token,
        Err(HiggsfieldAuthError::MissingSecret { .. }) => {
            mark_provider_action_required(
                db,
                job_id,
                clone_id,
                user_id,
                idempotency_key,
                &CloneTrainingProviderError::HiggsfieldSecretMissing,
            )
            .await?;
            return Ok(());
        }
        Err(error) => return Err(Error::RustError(error.to_string())),
    };

    validate_access_token(&token.access_token)
        .await
        .map_err(|error| Error::RustError(error.to_string()))?;
    reserve_provider_lease(db, &provider_account.id, job_id).await?;

    let request_json =
        serde_json::from_str::<Value>(&job.request_json).unwrap_or_else(|_| json!({}));
    let result = call_tool(
        &token.access_token,
        json!(job_id),
        &tool_name,
        json!({
            "jobId": job_id,
            "cloneId": clone_id,
            "userId": user_id,
            "providerAccountId": provider_account.id,
            "request": request_json,
        }),
    )
    .await
    .map_err(|error| match error {
        HiggsfieldMcpError::Worker(worker_error) => worker_error,
        other => Error::RustError(other.to_string()),
    })?;

    record_provider_submission(
        db,
        job_id,
        clone_id,
        user_id,
        idempotency_key,
        &provider_account.id,
        &result.raw_json,
    )
    .await
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
         AND pal.status = 'active'
         AND pal.released_at IS NULL
        WHERE pa.provider = 'higgsfield'
          AND pa.disabled_at IS NULL
        GROUP BY pa.id, pa.health_state, pa.capacity_json
        "#,
        vec![],
    )
    .await
}

async fn reserve_provider_lease(
    db: &D1Database,
    provider_account_id: &str,
    job_id: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let lease_expires_at = lease_expires_at_iso(30.0);
    db::exec(
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
    .await
}

async fn record_provider_submission(
    db: &D1Database,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    idempotency_key: &str,
    provider_account_id: &str,
    raw_json: &Value,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let response_json = raw_json.to_string();
    db::exec(
        db,
        r#"
        UPDATE soul_training_jobs
        SET provider_account_id = ?,
            provider_job_id = COALESCE(json_extract(?, '$.result.id'), json_extract(?, '$.id'), provider_job_id),
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
            json!(response_json),
            json!(response_json),
            json!(response_json),
            json!(now),
            json!(job_id),
            json!(user_id),
            json!(clone_id),
            json!(idempotency_key),
        ],
    )
    .await
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
    .await?;

    db::exec(
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
    .await
}

fn changed_rows(result: &worker::D1Result) -> WorkerResult<usize> {
    Ok(result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or_default())
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
