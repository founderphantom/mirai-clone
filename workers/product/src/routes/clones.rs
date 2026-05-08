use crate::auth_client::verify_session;
use crate::db;
use crate::domain::entitlements::{can_create_clone, Entitlements};
use crate::domain::idempotency::clone_upload_key;
use crate::domain::media_validation::{
    is_supported_reference_content_type, validate_reference_count, ReferenceCountError,
};
use crate::http::error::ApiError;
use crate::queues::messages::CloneTrainingMessage;
use crate::services::accounts::upsert_account_from_identity;
use crate::services::clones::slugify_handle;
use crate::services::media::media_storage_key;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use worker::{
    File, FormData, FormEntry, HttpMetadata, Request, Response, Result as WorkerResult,
    RouteContext,
};

const MIN_REFERENCES: usize = 5;
const MAX_REFERENCES: usize = 20;
const MAX_REFERENCE_BYTES: usize = 15 * 1024 * 1024;
const FILE_FIELDS: [&str; 3] = ["photos", "files", "file"];

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u32,
}

#[derive(Debug)]
struct PreparedReference {
    source_field: String,
    file_name: String,
    content_type: String,
    bytes: Vec<u8>,
    sha256_hex: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManualUploadResponse {
    clone: CloneResponse,
    training_job: TrainingJobResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloneResponse {
    id: String,
    display_name: String,
    handle: String,
    source: String,
    status: String,
    soul_status: String,
    reference_count_total: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrainingJobResponse {
    id: String,
    status: String,
    reference_count: usize,
}

pub async fn manual_upload(mut req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let verified = auth.verified_identity();

    let form = match req.form_data().await {
        Ok(form) => form,
        Err(_) => {
            return ApiError::bad_request(
                "invalid_multipart",
                "Expected multipart form data with 5 to 20 image references.",
            )
            .to_response()
        }
    };

    let display_name = display_name_from_form(&form);
    let mut references = Vec::new();
    for field in FILE_FIELDS {
        if let Some(entries) = form.get_all(field) {
            for entry in entries {
                let FormEntry::File(file) = entry else {
                    continue;
                };
                let reference = match prepare_reference(field, file).await? {
                    Ok(reference) => reference,
                    Err(error) => return error.to_response(),
                };
                references.push(reference);
            }
        }
    }

    if let Err(error) = validate_reference_count(references.len()) {
        return reference_count_error(error, references.len()).to_response();
    }

    let db = ctx.env.d1("DB")?;
    upsert_account_from_identity(&db, &verified).await?;

    let active_clones = count_active_clones(&db, &verified.user_id).await?;
    let entitlements = Entitlements {
        max_active_clones: verified.max_active_clones,
    };
    if can_create_clone(&entitlements, active_clones).is_err() {
        return ApiError::conflict(
            "clone_limit_reached",
            format!(
                "Your plan allows up to {} active clone{}.",
                entitlements.max_active_clones,
                if entitlements.max_active_clones == 1 {
                    ""
                } else {
                    "s"
                }
            ),
        )
        .to_response();
    }

    let clone_id = prefixed_id("clone");
    let training_job_id = prefixed_id("train");
    let handle = slugify_handle(&display_name);
    let hashes = references
        .iter()
        .map(|reference| reference.sha256_hex.clone())
        .collect::<Vec<_>>();
    let idempotency_key = clone_upload_key(&verified.user_id, &display_name, &hashes);
    let reference_count = references.len();
    let now = now_iso_string();

    db::exec(
        &db,
        r#"
        INSERT INTO clone_profiles (
          id,
          user_id,
          display_name,
          handle,
          source,
          status,
          soul_status,
          provider,
          provider_config_json,
          reference_count_total,
          reference_count_training_selected,
          created_at,
          updated_at
        )
        VALUES (?, ?, ?, ?, 'manual_upload', 'active', 'queued', 'higgsfield', '{}', ?, ?, ?, ?)
        "#,
        vec![
            json!(clone_id),
            json!(verified.user_id),
            json!(display_name),
            json!(handle),
            json!(reference_count),
            json!(reference_count),
            json!(now),
            json!(now),
        ],
    )
    .await?;

    let bucket = ctx.env.bucket("MEDIA")?;
    let mut media_asset_ids = Vec::with_capacity(reference_count);
    let mut reference_asset_ids = Vec::with_capacity(reference_count);

    for (index, reference) in references.into_iter().enumerate() {
        let PreparedReference {
            source_field,
            file_name,
            content_type,
            bytes,
            sha256_hex,
        } = reference;
        let media_id = prefixed_id("media");
        let reference_id = prefixed_id("ref");
        let byte_count = bytes.len();
        let storage_key = media_storage_key(&verified.user_id, &clone_id, &media_id, &content_type);

        bucket
            .put(storage_key.clone(), bytes)
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
            &db,
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
              sha256,
              metadata_json,
              created_at
            )
            VALUES (?, ?, ?, 'image', 'manual_upload', ?, ?, ?, ?, ?, ?)
            "#,
            vec![
                json!(media_id),
                json!(verified.user_id),
                json!(clone_id),
                json!(storage_key),
                json!(content_type),
                json!(byte_count),
                json!(sha256_hex),
                json!(json!({
                    "originalFilename": file_name,
                    "uploadField": source_field,
                })
                .to_string()),
                json!(now),
            ],
        )
        .await?;

        db::exec(
            &db,
            r#"
            INSERT INTO clone_reference_assets (
              id,
              user_id,
              clone_id,
              media_asset_id,
              sort_order,
              role,
              eligibility_status,
              variety_tags_json,
              training_selected,
              audit_json,
              created_at
            )
            VALUES (?, ?, ?, ?, ?, 'identity', 'accepted', '[]', 1, '{}', ?)
            "#,
            vec![
                json!(reference_id),
                json!(verified.user_id),
                json!(clone_id),
                json!(media_id),
                json!(index),
                json!(now),
            ],
        )
        .await?;

        media_asset_ids.push(media_id);
        reference_asset_ids.push(reference_id);
    }

    db::exec(
        &db,
        r#"
        INSERT INTO soul_training_jobs (
          id,
          user_id,
          clone_id,
          provider,
          status,
          idempotency_key,
          reference_count,
          request_json,
          response_json,
          queued_at,
          updated_at
        )
        VALUES (?, ?, ?, 'higgsfield', 'queued', ?, ?, ?, '{}', ?, ?)
        "#,
        vec![
            json!(training_job_id),
            json!(verified.user_id),
            json!(clone_id),
            json!(idempotency_key),
            json!(reference_count),
            json!(json!({
                "source": "manual_upload",
                "mediaAssetIds": media_asset_ids,
                "referenceAssetIds": reference_asset_ids,
            })
            .to_string()),
            json!(now),
            json!(now),
        ],
    )
    .await?;

    ctx.env
        .queue("CLONE_TRAINING_QUEUE")?
        .send(CloneTrainingMessage::SubmitCloneTraining {
            job_id: training_job_id.clone(),
            clone_id: clone_id.clone(),
            user_id: verified.user_id.clone(),
            idempotency_key,
        })
        .await?;

    Response::from_json(&ManualUploadResponse {
        clone: CloneResponse {
            id: clone_id,
            display_name,
            handle,
            source: "manual_upload".to_string(),
            status: "active".to_string(),
            soul_status: "queued".to_string(),
            reference_count_total: reference_count,
        },
        training_job: TrainingJobResponse {
            id: training_job_id,
            status: "queued".to_string(),
            reference_count,
        },
    })
}

async fn prepare_reference(
    source_field: &str,
    file: File,
) -> WorkerResult<Result<PreparedReference, ApiError>> {
    let content_type = file.type_();
    if !is_supported_reference_content_type(&content_type) {
        return Ok(Err(ApiError::bad_request(
            "unsupported_reference_content_type",
            format!(
                "Reference image '{}' has unsupported content type '{}'.",
                file.name(),
                if content_type.trim().is_empty() {
                    "unknown"
                } else {
                    content_type.as_str()
                }
            ),
        )));
    }

    if file.size() > MAX_REFERENCE_BYTES {
        return Ok(Err(ApiError::bad_request(
            "reference_too_large",
            format!(
                "Reference image '{}' is larger than the 15 MB limit.",
                file.name()
            ),
        )));
    }

    let bytes = file.bytes().await?;
    if bytes.len() > MAX_REFERENCE_BYTES {
        return Ok(Err(ApiError::bad_request(
            "reference_too_large",
            format!(
                "Reference image '{}' is larger than the 15 MB limit.",
                file.name()
            ),
        )));
    }

    let mut hasher = Sha256::new();
    hasher.update(&bytes);

    Ok(Ok(PreparedReference {
        source_field: source_field.to_string(),
        file_name: file.name(),
        content_type,
        bytes,
        sha256_hex: hex::encode(hasher.finalize()),
    }))
}

async fn count_active_clones(db: &worker::D1Database, user_id: &str) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM clone_profiles
        WHERE user_id = ?
          AND status = 'active'
          AND deleted_at IS NULL
        "#,
        vec![json!(user_id)],
    )
    .await?;

    Ok(row.map(|row| row.count).unwrap_or(0))
}

fn display_name_from_form(form: &FormData) -> String {
    form.get_field("displayName")
        .or_else(|| form.get_field("name"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "My Soul".to_string())
}

fn reference_count_error(error: ReferenceCountError, count: usize) -> ApiError {
    match error {
        ReferenceCountError::TooFew => ApiError::bad_request(
            "invalid_reference_count",
            format!("Upload at least {MIN_REFERENCES} image references. Received {count}."),
        ),
        ReferenceCountError::TooMany => ApiError::bad_request(
            "invalid_reference_count",
            format!("Upload no more than {MAX_REFERENCES} image references. Received {count}."),
        ),
    }
}

fn prefixed_id(prefix: &str) -> String {
    format!("{}_{}", prefix, Uuid::new_v4().simple())
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}
