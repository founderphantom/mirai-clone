use crate::db;
use crate::services::media::{normalize_extension, safe_segment};
use js_sys::{Reflect, Uint8Array};
use serde_json::json;
use sha2::{Digest, Sha256};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{ReadableStream, ReadableStreamDefaultReader};
use worker::{
    D1Database, Env, Error, Fetch, HttpMetadata, Method, Request, Response, ResponseBody,
    Result as WorkerResult,
};

const MAX_VISUAL_REFERENCE_BYTES: usize = 15 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedVisualReference {
    pub media_asset_id: String,
    pub storage_key: String,
    pub content_type: String,
    pub byte_size: usize,
    pub sha256_hex: String,
}

pub fn visual_reference_storage_key(
    user_id: &str,
    clone_id: &str,
    visual_reference_id: &str,
    content_type: &str,
) -> String {
    format!(
        "visual-references/{}/{}/{}/source.{}",
        safe_segment(user_id),
        safe_segment(clone_id),
        safe_segment(visual_reference_id),
        normalize_extension(content_type)
    )
}

pub fn supported_visual_reference_content_type(content_type: &str) -> bool {
    matches!(
        content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "image/jpeg" | "image/jpg" | "image/png" | "image/webp" | "image/heic" | "image/heif"
    )
}

pub async fn cache_approved_visual_reference(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    visual_reference_id: &str,
    original_image_url: &str,
    width: Option<u32>,
    height: Option<u32>,
) -> WorkerResult<CachedVisualReference> {
    let (bytes, content_type) = fetch_visual_reference_image(original_image_url).await?;
    let byte_size = bytes.len();
    let sha256_hex = sha256_hex(&bytes);
    let media_asset_id = visual_reference_media_asset_id(user_id, clone_id, visual_reference_id);
    let storage_key =
        visual_reference_storage_key(user_id, clone_id, visual_reference_id, &content_type);

    env.bucket("MEDIA")?
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
          width,
          height,
          remote_url,
          sha256,
          metadata_json,
          created_at
        )
        VALUES (?, ?, ?, 'visual_reference', 'instagram', ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        vec![
            json!(media_asset_id.clone()),
            json!(user_id),
            json!(clone_id),
            json!(storage_key.clone()),
            json!(content_type.clone()),
            json!(byte_size),
            json!(width),
            json!(height),
            json!(original_image_url),
            json!(sha256_hex.clone()),
            json!(json!({ "visualReferenceId": visual_reference_id }).to_string()),
            json!(now),
        ],
    )
    .await?;

    Ok(CachedVisualReference {
        media_asset_id,
        storage_key,
        content_type,
        byte_size,
        sha256_hex,
    })
}

pub async fn fetch_visual_reference_image(image_url: &str) -> WorkerResult<(Vec<u8>, String)> {
    let request = Request::new(image_url, Method::Get)?;
    let response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    if status >= 400 {
        return Err(Error::RustError(format!(
            "visual_reference_image_fetch_failed:{status}"
        )));
    }

    let content_type =
        required_visual_reference_content_type(response.headers().get("content-type")?)?;
    if content_length_too_large(response.headers().get("content-length")?.as_deref()) {
        return Err(Error::RustError(
            "visual_reference_image_too_large".to_string(),
        ));
    }

    let bytes = read_response_bytes_with_limit(&response).await?;

    Ok((bytes, content_type))
}

async fn read_response_bytes_with_limit(response: &Response) -> WorkerResult<Vec<u8>> {
    match response.body().clone() {
        ResponseBody::Body(bytes) => validate_visual_reference_bytes(bytes),
        ResponseBody::Empty => visual_reference_image_empty_error(),
        ResponseBody::Stream(stream) => read_stream_bytes_with_limit(stream).await,
    }
}

async fn read_stream_bytes_with_limit(stream: ReadableStream) -> WorkerResult<Vec<u8>> {
    let reader = stream
        .get_reader()
        .dyn_into::<ReadableStreamDefaultReader>()
        .map_err(|_| Error::RustError("visual_reference_image_stream_reader_failed".to_string()))?;
    let mut bytes = Vec::new();

    loop {
        let chunk = JsFuture::from(reader.read()).await.map_err(Error::from)?;
        if read_result_done(&chunk)? {
            break;
        }

        let chunk_bytes = read_result_bytes(&chunk)?;
        if bytes.len().saturating_add(chunk_bytes.len()) > MAX_VISUAL_REFERENCE_BYTES {
            let _ = reader.cancel();
            reader.release_lock();
            return visual_reference_image_too_large_error();
        }
        bytes.extend_from_slice(&chunk_bytes);
    }

    reader.release_lock();
    validate_visual_reference_bytes(bytes)
}

fn read_result_done(result: &JsValue) -> WorkerResult<bool> {
    Ok(Reflect::get(result, &JsValue::from_str("done"))
        .map_err(Error::from)?
        .as_bool()
        .unwrap_or(false))
}

fn read_result_bytes(result: &JsValue) -> WorkerResult<Vec<u8>> {
    let value = Reflect::get(result, &JsValue::from_str("value")).map_err(Error::from)?;
    if value.is_null() || value.is_undefined() {
        return Ok(Vec::new());
    }
    Ok(Uint8Array::from(value).to_vec())
}

fn validate_visual_reference_bytes(bytes: Vec<u8>) -> WorkerResult<Vec<u8>> {
    if bytes.is_empty() {
        return visual_reference_image_empty_error();
    }
    if bytes.len() > MAX_VISUAL_REFERENCE_BYTES {
        return visual_reference_image_too_large_error();
    }
    Ok(bytes)
}

fn required_visual_reference_content_type(content_type: Option<String>) -> WorkerResult<String> {
    let Some(content_type) = content_type else {
        return visual_reference_image_unsupported_content_type_error();
    };
    if !supported_visual_reference_content_type(&content_type) {
        return visual_reference_image_unsupported_content_type_error();
    }
    Ok(content_type)
}

fn content_length_too_large(content_length: Option<&str>) -> bool {
    content_length
        .and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value > MAX_VISUAL_REFERENCE_BYTES)
        .unwrap_or(false)
}

fn visual_reference_media_asset_id(
    user_id: &str,
    clone_id: &str,
    visual_reference_id: &str,
) -> String {
    let mut hasher = Sha256::new();
    update_hash_part(&mut hasher, user_id);
    update_hash_part(&mut hasher, clone_id);
    update_hash_part(&mut hasher, visual_reference_id);
    let digest = hasher.finalize();
    format!("media_visual_{}", &hex::encode(digest)[..24])
}

fn update_hash_part(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn visual_reference_image_unsupported_content_type_error<T>() -> WorkerResult<T> {
    Err(Error::RustError(
        "visual_reference_image_unsupported_content_type".to_string(),
    ))
}

fn visual_reference_image_empty_error<T>() -> WorkerResult<T> {
    Err(Error::RustError("visual_reference_image_empty".to_string()))
}

fn visual_reference_image_too_large_error<T>() -> WorkerResult<T> {
    Err(Error::RustError(
        "visual_reference_image_too_large".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_reference_media_asset_id_is_scoped_to_reference_identity() {
        let first = visual_reference_media_asset_id("user_1", "clone_1", "vref_1");
        let same = visual_reference_media_asset_id("user_1", "clone_1", "vref_1");
        let different_reference = visual_reference_media_asset_id("user_1", "clone_1", "vref_2");

        assert_eq!(first, same);
        assert_ne!(first, different_reference);
        assert!(first.starts_with("media_visual_"));
        assert_eq!(first.len(), "media_visual_".len() + 24);
    }

    #[test]
    fn visual_reference_content_type_requires_header() {
        assert!(required_visual_reference_content_type(None).is_err());
        assert!(required_visual_reference_content_type(Some("text/html".to_string())).is_err());
        assert_eq!(
            required_visual_reference_content_type(Some("image/png; charset=binary".to_string()))
                .unwrap(),
            "image/png; charset=binary"
        );
    }
}
