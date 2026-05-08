use crate::db;
use crate::queues::messages::CloneTrainingMessage;
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;
use worker::{D1Database, Env, Error, MessageBatch, MessageExt, Result as WorkerResult};

const HIGGSFIELD_REFRESH_SECRET_NAME: &str = "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FUFU";

#[derive(Debug, Deserialize)]
struct TrainingJobRow {
    status: String,
}

#[derive(Debug, Error)]
enum CloneTrainingProviderError {
    #[error("Higgsfield provider refresh token secret is not configured.")]
    HiggsfieldSecretMissing,
}

impl CloneTrainingProviderError {
    fn code(&self) -> &'static str {
        match self {
            Self::HiggsfieldSecretMissing => "higgsfield_secret_missing",
        }
    }
}

pub async fn handle_batch(batch: MessageBatch<CloneTrainingMessage>, env: Env) -> WorkerResult<()> {
    let db = env.d1("DB")?;

    for raw_message in batch.raw_iter() {
        let body = match serde_wasm_bindgen::from_value::<CloneTrainingMessage>(raw_message.body())
        {
            Ok(body) => body,
            Err(_) => {
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
            Err(_) => raw_message.retry(),
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
        SELECT status
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

    if job.status != "queued" {
        return Ok(());
    }

    let now = now_iso_string();
    run_batch_checked(
        db,
        vec![
            (
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
            ),
            (
                r#"
                UPDATE clone_profiles
                SET soul_status = 'training',
                    updated_at = ?
                WHERE id = ?
                  AND user_id = ?
                "#,
                vec![json!(now), json!(clone_id), json!(user_id)],
            ),
        ],
    )
    .await?;

    if env.secret(HIGGSFIELD_REFRESH_SECRET_NAME).is_err() {
        mark_provider_action_required(
            db,
            job_id,
            clone_id,
            user_id,
            &CloneTrainingProviderError::HiggsfieldSecretMissing,
        )
        .await?;
    }

    Ok(())
}

async fn mark_provider_action_required(
    db: &D1Database,
    job_id: &str,
    clone_id: &str,
    user_id: &str,
    error: &CloneTrainingProviderError,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let message = error.to_string();

    run_batch_checked(
        db,
        vec![
            (
                r#"
                UPDATE soul_training_jobs
                SET status = 'provider_action_required',
                    error_code = ?,
                    error_message = ?,
                    updated_at = ?
                WHERE id = ?
                  AND user_id = ?
                  AND clone_id = ?
                "#,
                vec![
                    json!(error.code()),
                    json!(message),
                    json!(now),
                    json!(job_id),
                    json!(user_id),
                    json!(clone_id),
                ],
            ),
            (
                r#"
                UPDATE clone_profiles
                SET soul_status = 'provider_action_required',
                    updated_at = ?
                WHERE id = ?
                  AND user_id = ?
                "#,
                vec![json!(now), json!(clone_id), json!(user_id)],
            ),
        ],
    )
    .await?;

    Ok(())
}

async fn run_batch_checked(
    db: &D1Database,
    statements: Vec<(&str, Vec<serde_json::Value>)>,
) -> WorkerResult<()> {
    let results = db::batch(db, statements).await?;
    if let Some(error) = results
        .iter()
        .find(|result| !result.success())
        .map(|result| {
            result
                .error()
                .unwrap_or_else(|| "D1 batch failed".to_string())
        })
    {
        return Err(Error::RustError(error));
    }

    Ok(())
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}
