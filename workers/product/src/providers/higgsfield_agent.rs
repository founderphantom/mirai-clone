use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use wasm_bindgen::JsValue;
use worker::{Fetch, Headers, Method, Request, RequestInit};

pub const HIGGSFIELD_AGENT_API_BASE_URL: &str = "https://fnf.higgsfield.ai";

const CLIENT_NAME: &str = "codex";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiggsfieldAgentResponse {
    pub status: u16,
    pub raw_json: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiggsfieldAgentMediaFile {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiggsfieldAgentUploadedMedia {
    pub media_id: String,
    pub url: Option<String>,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HiggsfieldAgentUploadSlot {
    media_id: String,
    upload_url: String,
    url: Option<String>,
}

#[derive(Debug, Error)]
pub enum HiggsfieldAgentError {
    #[error("higgsfield agent endpoint returned status {status}")]
    HttpStatus {
        status: u16,
        raw_json: Option<Value>,
    },
    #[error("higgsfield agent response was invalid: {message}")]
    InvalidResponse { message: String, raw_json: Value },
    #[error("higgsfield agent request failed: {0}")]
    Worker(#[from] worker::Error),
    #[error("failed to serialize higgsfield agent request: {0}")]
    Serde(#[from] serde_json::Error),
}

pub async fn upload_media_files(
    api_base_url: &str,
    access_token: &str,
    files: &[HiggsfieldAgentMediaFile],
) -> Result<Vec<HiggsfieldAgentUploadedMedia>, HiggsfieldAgentError> {
    let mut uploaded = Vec::with_capacity(files.len());
    for file in files {
        let slot = create_upload_slot(api_base_url, access_token).await?;
        put_media_bytes(&slot.upload_url, &file.content_type, &file.bytes).await?;
        uploaded.push(confirm_uploaded_media(api_base_url, access_token, &slot, file).await?);
    }
    Ok(uploaded)
}

pub async fn create_soul_reference(
    api_base_url: &str,
    access_token: &str,
    name: &str,
    media_ids: &[String],
) -> Result<HiggsfieldAgentResponse, HiggsfieldAgentError> {
    request_json(
        api_base_url,
        access_token,
        Method::Post,
        "/agents/custom-references",
        Some(soul_reference_create_body(name, media_ids)),
    )
    .await
}

pub async fn get_soul_reference(
    api_base_url: &str,
    access_token: &str,
    soul_id: &str,
) -> Result<HiggsfieldAgentResponse, HiggsfieldAgentError> {
    request_json(
        api_base_url,
        access_token,
        Method::Get,
        &format!("/agents/custom-references/{soul_id}"),
        None,
    )
    .await
}

async fn create_upload_slot(
    api_base_url: &str,
    access_token: &str,
) -> Result<HiggsfieldAgentUploadSlot, HiggsfieldAgentError> {
    let response = request_json(
        api_base_url,
        access_token,
        Method::Post,
        "/agents/uploads?type=image",
        None,
    )
    .await?;
    extract_upload_slot(&response.raw_json)
}

async fn confirm_uploaded_media(
    api_base_url: &str,
    access_token: &str,
    slot: &HiggsfieldAgentUploadSlot,
    file: &HiggsfieldAgentMediaFile,
) -> Result<HiggsfieldAgentUploadedMedia, HiggsfieldAgentError> {
    let response = request_json(
        api_base_url,
        access_token,
        Method::Post,
        &format!("/agents/uploads/{}/confirm?type=image", slot.media_id),
        None,
    )
    .await?;
    Ok(uploaded_media_from_confirm_response(
        &response.raw_json,
        slot,
        &file.content_type,
    ))
}

async fn request_json(
    api_base_url: &str,
    access_token: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<HiggsfieldAgentResponse, HiggsfieldAgentError> {
    let headers = Headers::new();
    headers.set("authorization", &format!("Bearer {access_token}"))?;
    headers.set("accept", "application/json")?;
    headers.set("content-type", "application/json")?;
    headers.set("x-hf-mcp-client-name", CLIENT_NAME)?;

    let mut init = RequestInit::new();
    init.with_method(method).with_headers(headers);
    if let Some(body) = body {
        init.with_body(Some(JsValue::from_str(&serde_json::to_string(&body)?)));
    }

    let request = Request::new_with_init(&api_url(api_base_url, path), &init)?;
    let mut response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    let response_text = response.text().await.unwrap_or_default();
    let raw_json = parse_response_text(&response_text);

    if status >= 400 {
        return Err(HiggsfieldAgentError::HttpStatus {
            status,
            raw_json: Some(raw_json),
        });
    }

    Ok(HiggsfieldAgentResponse { status, raw_json })
}

async fn put_media_bytes(
    upload_url: &str,
    content_type: &str,
    bytes: &[u8],
) -> Result<(), HiggsfieldAgentError> {
    let headers = Headers::new();
    headers.set("content-type", content_type)?;

    let body = js_sys::Uint8Array::from(bytes);
    let mut init = RequestInit::new();
    init.with_method(Method::Put)
        .with_headers(headers)
        .with_body(Some(JsValue::from(body)));

    let request = Request::new_with_init(upload_url, &init)?;
    let response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    if status >= 400 {
        return Err(HiggsfieldAgentError::HttpStatus {
            status,
            raw_json: None,
        });
    }

    Ok(())
}

fn soul_reference_create_body(name: &str, media_ids: &[String]) -> Value {
    let input_images = media_ids
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "type": "media_input",
            })
        })
        .collect::<Vec<_>>();

    json!({
        "input_images": input_images,
        "name": name,
        "type": "soul_2",
    })
}

fn uploaded_media_from_confirm_response(
    raw_json: &Value,
    slot: &HiggsfieldAgentUploadSlot,
    content_type: &str,
) -> HiggsfieldAgentUploadedMedia {
    HiggsfieldAgentUploadedMedia {
        media_id: json_string_at_any(
            raw_json,
            &[
                "/id",
                "/media_id",
                "/mediaId",
                "/upload_id",
                "/uploadId",
                "/result/id",
                "/result/media_id",
                "/result/mediaId",
                "/data/id",
                "/data/media_id",
                "/data/mediaId",
            ],
        )
        .unwrap_or_else(|| slot.media_id.clone()),
        url: json_string_at_any(
            raw_json,
            &[
                "/url",
                "/public_url",
                "/publicUrl",
                "/cdn_url",
                "/cdnUrl",
                "/result/url",
                "/result/public_url",
                "/result/publicUrl",
                "/data/url",
                "/data/public_url",
                "/data/publicUrl",
            ],
        )
        .or_else(|| slot.url.clone()),
        content_type: content_type.to_string(),
    }
}

fn extract_upload_slot(raw_json: &Value) -> Result<HiggsfieldAgentUploadSlot, HiggsfieldAgentError> {
    for payload in upload_payloads(raw_json) {
        if let Some(slot) = upload_slot_from_value(&payload) {
            return Ok(slot);
        }
    }

    Err(HiggsfieldAgentError::InvalidResponse {
        message: "upload response did not include id and upload_url".to_string(),
        raw_json: raw_json.clone(),
    })
}

fn upload_slot_from_value(value: &Value) -> Option<HiggsfieldAgentUploadSlot> {
    let media_id = json_string_at_any(
        value,
        &[
            "/id",
            "/media_id",
            "/mediaId",
            "/upload_id",
            "/uploadId",
            "/result/id",
            "/result/media_id",
            "/result/mediaId",
            "/data/id",
            "/data/media_id",
            "/data/mediaId",
        ],
    )?;
    let upload_url = json_string_at_any(
        value,
        &[
            "/upload_url",
            "/uploadUrl",
            "/presigned_url",
            "/presignedUrl",
            "/result/upload_url",
            "/result/uploadUrl",
            "/data/upload_url",
            "/data/uploadUrl",
        ],
    )?;
    let url = json_string_at_any(
        value,
        &[
            "/url",
            "/public_url",
            "/publicUrl",
            "/cdn_url",
            "/cdnUrl",
            "/result/url",
            "/result/public_url",
            "/result/publicUrl",
            "/data/url",
            "/data/public_url",
            "/data/publicUrl",
        ],
    );

    Some(HiggsfieldAgentUploadSlot {
        media_id,
        upload_url,
        url,
    })
}

fn upload_payloads(raw_json: &Value) -> Vec<Value> {
    let mut payloads = vec![raw_json.clone()];
    for path in ["/result", "/data", "/media", "/upload"] {
        if let Some(payload) = raw_json.pointer(path) {
            payloads.push(payload.clone());
        }
    }
    payloads
}

fn json_string_at_any(value: &Value, paths: &[&str]) -> Option<String> {
    paths
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn parse_response_text(response_text: &str) -> Value {
    serde_json::from_str::<Value>(response_text.trim()).unwrap_or_else(|_| {
        json!({
            "rawText": response_text,
        })
    })
}

fn api_url(api_base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        api_base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::{
        extract_upload_slot, soul_reference_create_body, uploaded_media_from_confirm_response,
        HiggsfieldAgentUploadSlot,
    };
    use serde_json::json;

    #[test]
    fn parses_upload_create_response() {
        let raw_json = json!({
            "id": "22222222-2222-2222-2222-222222222222",
            "type": "image",
            "url": "https://d2ol7oe51mr4n9.cloudfront.net/u/222.png",
            "upload_url": "https://upload.example/signed"
        });

        let slot = extract_upload_slot(&raw_json).expect("upload slot");

        assert_eq!(slot.media_id, "22222222-2222-2222-2222-222222222222");
        assert_eq!(slot.upload_url, "https://upload.example/signed");
        assert_eq!(
            slot.url.as_deref(),
            Some("https://d2ol7oe51mr4n9.cloudfront.net/u/222.png")
        );
    }

    #[test]
    fn confirm_response_keeps_media_input_id_for_soul_training() {
        let slot = HiggsfieldAgentUploadSlot {
            media_id: "22222222-2222-2222-2222-222222222222".to_string(),
            upload_url: "https://upload.example/signed".to_string(),
            url: Some("https://d2ol7oe51mr4n9.cloudfront.net/u/222.png".to_string()),
        };
        let raw_json = json!({
            "id": "22222222-2222-2222-2222-222222222222",
            "type": "image",
            "url": "https://d2ol7oe51mr4n9.cloudfront.net/u/222.png"
        });

        let uploaded = uploaded_media_from_confirm_response(&raw_json, &slot, "image/jpeg");

        assert_eq!(
            uploaded.media_id,
            "22222222-2222-2222-2222-222222222222"
        );
        assert_eq!(
            uploaded.url.as_deref(),
            Some("https://d2ol7oe51mr4n9.cloudfront.net/u/222.png")
        );
        assert_eq!(uploaded.content_type, "image/jpeg");
    }

    #[test]
    fn soul_create_body_matches_higgsfield_cli_payload() {
        assert_eq!(
            soul_reference_create_body(
                "Fake Soul Trace",
                &[
                    "22222222-2222-2222-2222-222222222222".to_string(),
                    "33333333-3333-3333-3333-333333333333".to_string(),
                ],
            ),
            json!({
                "input_images": [
                    {
                        "id": "22222222-2222-2222-2222-222222222222",
                        "type": "media_input"
                    },
                    {
                        "id": "33333333-3333-3333-3333-333333333333",
                        "type": "media_input"
                    }
                ],
                "name": "Fake Soul Trace",
                "type": "soul_2"
            })
        );
    }
}
