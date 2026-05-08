use crate::auth_client::verify_session;
use crate::db;
use crate::http::error::ApiError;
use serde::Deserialize;
use serde_json::json;
use worker::{Headers, Request, Response, Result as WorkerResult, RouteContext};

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
