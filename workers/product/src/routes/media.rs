use crate::auth_client::verify_session;
use crate::db;
use crate::domain::media_validation::is_supported_reference_content_type;
use crate::http::error::ApiError;
use crate::services::media::media_storage_key;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use worker::{
    FormData, FormEntry, Headers, HttpMetadata, Request, Response, Result as WorkerResult,
    RouteContext,
};

#[derive(Debug, Deserialize)]
struct MediaAssetRow {
    storage_key: Option<String>,
    content_type: Option<String>,
}

pub async fn get_media(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };

    let media_id = match ctx.param("id") {
        Some(id) => id,
        None => {
            return ApiError::bad_request("missing_media_id", "Media id is required.").to_response()
        }
    };

    let db = ctx.env.d1("DB")?;
    let row = db::first::<MediaAssetRow>(
        &db,
        r#"
        SELECT storage_key, content_type
        FROM media_assets
        WHERE id = ?
          AND user_id = ?
          AND deleted_at IS NULL
        "#,
        vec![json!(media_id), json!(auth.user_id)],
    )
    .await?;

    let Some(row) = row else {
        return ApiError::not_found("media_not_found", "Media asset was not found.").to_response();
    };

    let Some(storage_key) = row
        .storage_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return ApiError::not_found("media_unavailable", "Media asset has no storage object.")
            .to_response();
    };

    let object = match ctx.env.bucket("MEDIA")?.get(storage_key).execute().await? {
        Some(object) => object,
        None => {
            return ApiError::not_found(
                "media_object_missing",
                "Media object was not found in storage.",
            )
            .to_response()
        }
    };

    let Some(body) = object.body() else {
        return ApiError::not_found(
            "media_object_missing",
            "Media object was not found in storage.",
        )
        .to_response();
    };

    let headers = Headers::new();
    headers.set(
        "content-type",
        safe_response_content_type(row.content_type.as_deref()),
    )?;
    headers.set("cache-control", "private, max-age=300")?;
    headers.set("x-content-type-options", "nosniff")?;

    Ok(Response::from_body(body.response_body()?)?.with_headers(headers))
}

#[derive(Debug, Serialize)]
struct UploadMediaResponse {
    media: UploadedMedia,
}

#[derive(Debug, Serialize)]
struct UploadedMedia {
    id: String,
}

#[derive(Debug, Deserialize)]
struct CloneExistsRow {
    count: u32,
}

pub async fn upload_media(mut req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };

    let form = match req.form_data().await {
        Ok(form) => form,
        Err(_) => {
            return ApiError::bad_request(
                "invalid_multipart",
                "Expected multipart form data with one image file.",
            )
            .to_response()
        }
    };
    let clone_id = optional_form_field(&form, "cloneId");
    if let Some(clone_id) = clone_id.as_deref() {
        if !clone_belongs_to_user(&ctx.env.d1("DB")?, &auth.user_id, clone_id).await? {
            return ApiError::not_found("clone_not_found", "Clone profile was not found.")
                .to_response();
        }
    }

    let Some(file) = first_file(&form) else {
        return ApiError::bad_request("missing_file", "Image file is required.").to_response();
    };
    let content_type = {
        let value = file.type_();
        if value.trim().is_empty() {
            "application/octet-stream".to_string()
        } else {
            value
        }
    };
    if !is_supported_reference_content_type(&content_type) {
        return ApiError::bad_request(
            "unsupported_media_type",
            "Upload a JPG, PNG, WebP, HEIC, or HEIF image.",
        )
        .to_response();
    }

    let bytes = file.bytes().await?;
    let byte_count = bytes.len();
    let media_id = format!("media_{}", Uuid::new_v4().simple());
    let storage_key = media_storage_key(
        &auth.user_id,
        clone_id.as_deref().unwrap_or("unassigned"),
        &media_id,
        &content_type,
    );

    ctx.env
        .bucket("MEDIA")?
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

    let now: String = js_sys::Date::new_0().to_iso_string().into();
    db::exec(
        &ctx.env.d1("DB")?,
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
          created_at
        )
        VALUES (?, ?, ?, 'inspiration', 'manual_upload', ?, ?, ?, ?)
        "#,
        vec![
            json!(media_id),
            json!(auth.user_id),
            json!(clone_id),
            json!(storage_key),
            json!(content_type),
            json!(byte_count),
            json!(now),
        ],
    )
    .await?;

    Response::from_json(&UploadMediaResponse {
        media: UploadedMedia { id: media_id },
    })
}

fn safe_response_content_type(value: Option<&str>) -> &'static str {
    let Some(value) = value else {
        return "application/octet-stream";
    };
    let normalized = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    match normalized.as_str() {
        "image/jpeg" | "image/jpg" => "image/jpeg",
        "image/png" => "image/png",
        "image/webp" => "image/webp",
        "image/heic" => "image/heic",
        "image/heif" => "image/heif",
        _ => "application/octet-stream",
    }
}

async fn clone_belongs_to_user(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: &str,
) -> WorkerResult<bool> {
    let row = db::first::<CloneExistsRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM clone_profiles
        WHERE id = ?
          AND user_id = ?
          AND deleted_at IS NULL
        "#,
        vec![json!(clone_id), json!(user_id)],
    )
    .await?;

    Ok(row.map(|row| row.count).unwrap_or(0) > 0)
}

fn optional_form_field(form: &FormData, name: &str) -> Option<String> {
    form.get(name).and_then(|entry| match entry {
        FormEntry::Field(value) => {
            let value = value.trim().to_string();
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        }
        FormEntry::File(_) => None,
    })
}

fn first_file(form: &FormData) -> Option<worker::File> {
    ["file", "media", "photo", "image"]
        .into_iter()
        .find_map(|name| match form.get(name) {
            Some(FormEntry::File(file)) => Some(file),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::safe_response_content_type;

    #[test]
    fn response_content_type_is_allowlisted() {
        assert_eq!(safe_response_content_type(Some("image/jpeg")), "image/jpeg");
        assert_eq!(
            safe_response_content_type(Some("image/png; charset=binary")),
            "image/png"
        );
        assert_eq!(safe_response_content_type(Some("IMAGE/WEBP")), "image/webp");
        assert_eq!(
            safe_response_content_type(Some("text/html")),
            "application/octet-stream"
        );
        assert_eq!(
            safe_response_content_type(Some("image/svg+xml")),
            "application/octet-stream"
        );
        assert_eq!(safe_response_content_type(None), "application/octet-stream");
    }
}
