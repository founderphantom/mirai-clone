use crate::auth_client::verify_session;
use crate::db;
use crate::domain::entitlements::Entitlements;
use crate::domain::idempotency::clone_upload_key;
use crate::domain::media_validation::{
    is_supported_reference_content_type, validate_reference_count, ReferenceCountError,
};
use crate::http::error::ApiError;
use crate::queues::messages::CloneTrainingMessage;
use crate::services::accounts::upsert_account_from_identity;
use crate::services::clones::{handle_with_suffix, slugify_handle};
use crate::services::media::media_storage_key;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use worker::{
    Bucket, File, FormData, FormEntry, HttpMetadata, Request, Response, Result as WorkerResult,
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

#[derive(Debug, Deserialize)]
struct ExistingUploadRow {
    clone_id: String,
    display_name: String,
    handle: String,
    source: String,
    status: String,
    soul_status: String,
    reference_count_total: usize,
    training_job_id: String,
    training_job_status: String,
    training_job_reference_count: usize,
}

#[derive(Debug)]
struct ReferenceFile {
    source_field: String,
    file: File,
}

#[derive(Debug)]
struct PreparedReference {
    source_field: String,
    file_name: String,
    content_type: String,
    byte_count: usize,
    sha256_hex: String,
    file: File,
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
    let reference_files = collect_reference_files(&form);
    if let Err(error) = validate_reference_count(reference_files.len()) {
        return reference_count_error(error, reference_files.len()).to_response();
    }

    let mut references = Vec::with_capacity(reference_files.len());
    for reference_file in reference_files {
        let reference = match prepare_reference(reference_file).await? {
            Ok(reference) => reference,
            Err(error) => return error.to_response(),
        };
        references.push(reference);
    }

    let hashes = references
        .iter()
        .map(|reference| reference.sha256_hex.clone())
        .collect::<Vec<_>>();
    let idempotency_key = clone_upload_key(&verified.user_id, &display_name, &hashes);
    let reference_count = references.len();

    let db = ctx.env.d1("DB")?;
    if let Some(existing) = find_existing_upload(&db, &verified.user_id, &idempotency_key).await? {
        return Response::from_json(&ManualUploadResponse {
            clone: CloneResponse {
                id: existing.clone_id,
                display_name: existing.display_name,
                handle: existing.handle,
                source: existing.source,
                status: existing.status,
                soul_status: existing.soul_status,
                reference_count_total: existing.reference_count_total,
            },
            training_job: TrainingJobResponse {
                id: existing.training_job_id,
                status: existing.training_job_status,
                reference_count: existing.training_job_reference_count,
            },
        });
    }

    upsert_account_from_identity(&db, &verified).await?;

    let entitlements = Entitlements {
        max_active_clones: verified.max_active_clones,
    };
    let clone_id = prefixed_id("clone");
    let training_job_id = prefixed_id("train");
    let base_handle = slugify_handle(&display_name);
    let handle = unique_clone_handle(&db, &verified.user_id, &base_handle).await?;
    let now = now_iso_string();
    let bucket = ctx.env.bucket("MEDIA")?;
    let queue = ctx.env.queue("CLONE_TRAINING_QUEUE")?;

    if !reserve_clone_profile(
        &db,
        &clone_id,
        &verified.user_id,
        &display_name,
        &handle,
        reference_count,
        &now,
        entitlements.max_active_clones,
    )
    .await?
    {
        return clone_limit_error(&entitlements).to_response();
    }

    let mut media_asset_ids = Vec::with_capacity(reference_count);
    let mut reference_asset_ids = Vec::with_capacity(reference_count);
    let mut uploaded_storage_keys = Vec::with_capacity(reference_count);
    let mut d1_statements = Vec::with_capacity(reference_count * 2 + 1);

    for (index, reference) in references.iter().enumerate() {
        let media_id = prefixed_id("media");
        let reference_id = prefixed_id("ref");
        let storage_key = media_storage_key(
            &verified.user_id,
            &clone_id,
            &media_id,
            &reference.content_type,
        );
        let bytes = match reference.file.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                cleanup_upload_artifacts(
                    &db,
                    &bucket,
                    &verified.user_id,
                    &clone_id,
                    &uploaded_storage_keys,
                )
                .await?;
                return Err(error);
            }
        };

        let upload_result = bucket
            .put(storage_key.clone(), bytes)
            .http_metadata(HttpMetadata {
                content_type: Some(reference.content_type.clone()),
                content_language: None,
                content_disposition: None,
                content_encoding: None,
                cache_control: None,
                cache_expiry: None,
            })
            .execute()
            .await;
        if let Err(error) = upload_result {
            cleanup_upload_artifacts(
                &db,
                &bucket,
                &verified.user_id,
                &clone_id,
                &uploaded_storage_keys,
            )
            .await?;
            return Err(error);
        }
        uploaded_storage_keys.push(storage_key.clone());

        d1_statements.push((
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
                json!(media_id.clone()),
                json!(verified.user_id),
                json!(clone_id),
                json!(storage_key),
                json!(reference.content_type.clone()),
                json!(reference.byte_count),
                json!(reference.sha256_hex.clone()),
                json!(json!({
                    "originalFilename": reference.file_name.clone(),
                    "uploadField": reference.source_field.clone(),
                })
                .to_string()),
                json!(now),
            ],
        ));

        d1_statements.push((
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
                json!(reference_id.clone()),
                json!(verified.user_id),
                json!(clone_id),
                json!(media_id),
                json!(index),
                json!(now),
            ],
        ));

        media_asset_ids.push(media_id);
        reference_asset_ids.push(reference_id);
    }

    d1_statements.push((
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
    ));

    match db::batch(&db, d1_statements).await {
        Ok(results) => {
            if let Some(batch_error) = results.iter().find_map(|result| {
                (!result.success()).then(|| {
                    result
                        .error()
                        .unwrap_or_else(|| "D1 batch insert failed.".to_string())
                })
            }) {
                cleanup_upload_artifacts(
                    &db,
                    &bucket,
                    &verified.user_id,
                    &clone_id,
                    &uploaded_storage_keys,
                )
                .await?;
                return Err(batch_error.into());
            }
        }
        Err(error) => {
            cleanup_upload_artifacts(
                &db,
                &bucket,
                &verified.user_id,
                &clone_id,
                &uploaded_storage_keys,
            )
            .await?;
            return Err(error);
        }
    }

    let queue_result = queue
        .send(CloneTrainingMessage::SubmitCloneTraining {
            job_id: training_job_id.clone(),
            clone_id: clone_id.clone(),
            user_id: verified.user_id.clone(),
            idempotency_key: idempotency_key.clone(),
        })
        .await;
    if let Err(error) = queue_result {
        cleanup_upload_artifacts(
            &db,
            &bucket,
            &verified.user_id,
            &clone_id,
            &uploaded_storage_keys,
        )
        .await?;
        return Err(error);
    }

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

fn collect_reference_files(form: &FormData) -> Vec<ReferenceFile> {
    let mut references = Vec::new();
    for field in FILE_FIELDS {
        if let Some(entries) = form.get_all(field) {
            for entry in entries {
                let FormEntry::File(file) = entry else {
                    continue;
                };
                references.push(ReferenceFile {
                    source_field: field.to_string(),
                    file,
                });
            }
        }
    }
    references
}

async fn prepare_reference(
    reference_file: ReferenceFile,
) -> WorkerResult<Result<PreparedReference, ApiError>> {
    let ReferenceFile { source_field, file } = reference_file;
    let file_name = file.name();
    let content_type = file.type_();
    if !is_supported_reference_content_type(&content_type) {
        return Ok(Err(ApiError::bad_request(
            "unsupported_reference_content_type",
            format!(
                "Reference image '{}' has unsupported content type '{}'.",
                file_name,
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
                file_name
            ),
        )));
    }

    let bytes = file.bytes().await?;
    let byte_count = bytes.len();
    if byte_count > MAX_REFERENCE_BYTES {
        return Ok(Err(ApiError::bad_request(
            "reference_too_large",
            format!(
                "Reference image '{}' is larger than the 15 MB limit.",
                file_name
            ),
        )));
    }

    let mut hasher = Sha256::new();
    hasher.update(&bytes);

    Ok(Ok(PreparedReference {
        source_field,
        file_name,
        content_type,
        byte_count,
        sha256_hex: hex::encode(hasher.finalize()),
        file,
    }))
}

async fn find_existing_upload(
    db: &worker::D1Database,
    user_id: &str,
    idempotency_key: &str,
) -> WorkerResult<Option<ExistingUploadRow>> {
    db::first::<ExistingUploadRow>(
        db,
        r#"
        SELECT
          cp.id AS clone_id,
          cp.display_name AS display_name,
          cp.handle AS handle,
          cp.source AS source,
          cp.status AS status,
          cp.soul_status AS soul_status,
          cp.reference_count_total AS reference_count_total,
          stj.id AS training_job_id,
          stj.status AS training_job_status,
          stj.reference_count AS training_job_reference_count
        FROM soul_training_jobs stj
        JOIN clone_profiles cp
          ON cp.id = stj.clone_id
         AND cp.user_id = stj.user_id
        WHERE stj.user_id = ?
          AND stj.idempotency_key = ?
          AND cp.deleted_at IS NULL
        LIMIT 1
        "#,
        vec![json!(user_id), json!(idempotency_key)],
    )
    .await
}

async fn unique_clone_handle(
    db: &worker::D1Database,
    user_id: &str,
    base_handle: &str,
) -> WorkerResult<String> {
    for suffix in 1..=10_000 {
        let candidate = handle_with_suffix(base_handle, suffix);
        if !clone_handle_exists(db, user_id, &candidate).await? {
            return Ok(candidate);
        }
    }

    Err("Unable to reserve a unique clone handle.".into())
}

async fn clone_handle_exists(
    db: &worker::D1Database,
    user_id: &str,
    handle: &str,
) -> WorkerResult<bool> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM clone_profiles
        WHERE user_id = ?
          AND handle = ?
        "#,
        vec![json!(user_id), json!(handle)],
    )
    .await?;

    Ok(row.map(|row| row.count).unwrap_or(0) > 0)
}

async fn reserve_clone_profile(
    db: &worker::D1Database,
    clone_id: &str,
    user_id: &str,
    display_name: &str,
    handle: &str,
    reference_count: usize,
    now: &str,
    max_active_clones: u32,
) -> WorkerResult<bool> {
    let result = db::run(
        db,
        r#"
        INSERT OR IGNORE INTO clone_profiles (
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
        SELECT ?, ?, ?, ?, 'manual_upload', 'active', 'queued', 'higgsfield', '{}', ?, ?, ?, ?
        WHERE (
          SELECT COUNT(*)
          FROM clone_profiles
          WHERE user_id = ?
            AND status = 'active'
            AND deleted_at IS NULL
        ) < ?
        "#,
        vec![
            json!(clone_id),
            json!(user_id),
            json!(display_name),
            json!(handle),
            json!(reference_count),
            json!(reference_count),
            json!(now),
            json!(now),
            json!(user_id),
            json!(max_active_clones),
        ],
    )
    .await?;

    let changes = result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or_default();
    Ok(changes > 0)
}

async fn cleanup_upload_artifacts(
    db: &worker::D1Database,
    bucket: &Bucket,
    user_id: &str,
    clone_id: &str,
    storage_keys: &[String],
) -> WorkerResult<()> {
    for storage_key in storage_keys {
        bucket.delete(storage_key.clone()).await?;
    }
    cleanup_clone_rows(db, user_id, clone_id).await
}

async fn cleanup_clone_rows(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: &str,
) -> WorkerResult<()> {
    db::batch(
        db,
        vec![
            (
                "DELETE FROM clone_reference_assets WHERE user_id = ? AND clone_id = ?",
                vec![json!(user_id), json!(clone_id)],
            ),
            (
                "DELETE FROM soul_training_jobs WHERE user_id = ? AND clone_id = ?",
                vec![json!(user_id), json!(clone_id)],
            ),
            (
                "DELETE FROM media_assets WHERE user_id = ? AND clone_id = ?",
                vec![json!(user_id), json!(clone_id)],
            ),
            (
                "DELETE FROM clone_profiles WHERE user_id = ? AND id = ?",
                vec![json!(user_id), json!(clone_id)],
            ),
        ],
    )
    .await?;
    Ok(())
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

fn clone_limit_error(entitlements: &Entitlements) -> ApiError {
    ApiError::conflict(
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
}

fn prefixed_id(prefix: &str) -> String {
    format!("{}_{}", prefix, Uuid::new_v4().simple())
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}
