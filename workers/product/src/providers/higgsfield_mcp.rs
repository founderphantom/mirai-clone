use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;
use wasm_bindgen::JsValue;
use worker::{Delay, Fetch, Headers, Method, Request, RequestInit};

const HIGGSFIELD_MCP_URL: &str = "https://mcp.higgsfield.ai/mcp";
const MEDIA_UPLOAD_TOOL: &str = "media_upload";
const MEDIA_CONFIRM_TOOL: &str = "media_confirm";
const MEDIA_READY_ATTEMPTS: usize = 6;
const MEDIA_READY_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiggsfieldMcpResponse {
    pub status: u16,
    pub raw_json: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiggsfieldMcpMediaFile {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiggsfieldUploadedMedia {
    pub media_id: String,
    pub url: String,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaUploadSlot {
    media_id: String,
    upload_url: String,
    url: String,
    content_type: Option<String>,
}

#[derive(Debug, Error)]
pub enum HiggsfieldMcpError {
    #[error("higgsfield mcp endpoint returned status {status}")]
    HttpStatus {
        status: u16,
        raw_json: Option<Value>,
    },
    #[error("higgsfield mcp response was invalid: {message}")]
    InvalidResponse { message: String, raw_json: Value },
    #[error("higgsfield mcp media URL was not ready: {url}")]
    MediaUrlNotReady { url: String },
    #[error("higgsfield mcp request failed: {0}")]
    Worker(#[from] worker::Error),
    #[error("failed to serialize higgsfield mcp request: {0}")]
    Serde(#[from] serde_json::Error),
}

pub async fn call_tool(
    access_token: &str,
    request_id: Value,
    tool_name: &str,
    arguments: Value,
) -> Result<HiggsfieldMcpResponse, HiggsfieldMcpError> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments,
        },
    });

    let headers = Headers::new();
    headers.set("authorization", &format!("Bearer {access_token}"))?;
    headers.set("content-type", "application/json")?;
    headers.set("accept", "application/json, text/event-stream")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&serde_json::to_string(&body)?)));

    let request = Request::new_with_init(HIGGSFIELD_MCP_URL, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    let response_text = response.text().await.unwrap_or_default();
    let raw_json = parse_mcp_response_text(&response_text);

    if status >= 400 {
        return Err(HiggsfieldMcpError::HttpStatus {
            status,
            raw_json: Some(raw_json),
        });
    }

    Ok(HiggsfieldMcpResponse { status, raw_json })
}

pub async fn upload_media_files(
    access_token: &str,
    files: &[HiggsfieldMcpMediaFile],
) -> Result<Vec<HiggsfieldUploadedMedia>, HiggsfieldMcpError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }

    let file_args = files
        .iter()
        .map(|file| {
            json!({
                "filename": file.filename,
                "content_type": file.content_type,
            })
        })
        .collect::<Vec<_>>();
    let upload_response = call_tool(
        access_token,
        json!("media-upload"),
        MEDIA_UPLOAD_TOOL,
        json!({
            "method": "upload_url",
            "files": file_args,
        }),
    )
    .await?;
    let slots = extract_media_upload_slots(&upload_response.raw_json)?;
    if slots.len() != files.len() {
        return Err(HiggsfieldMcpError::InvalidResponse {
            message: format!(
                "media_upload returned {} upload slots for {} files",
                slots.len(),
                files.len()
            ),
            raw_json: upload_response.raw_json,
        });
    }

    for (file, slot) in files.iter().zip(slots.iter()) {
        let upload_content_type = slot.content_type.as_deref().unwrap_or(&file.content_type);
        put_media_bytes(&slot.upload_url, upload_content_type, &file.bytes).await?;
    }

    let media_ids = slots
        .iter()
        .map(|slot| slot.media_id.clone())
        .collect::<Vec<_>>();
    call_tool(
        access_token,
        json!("media-confirm"),
        MEDIA_CONFIRM_TOOL,
        json!({
            "type": "image",
            "media_ids": media_ids,
        }),
    )
    .await?;

    let mut uploaded = Vec::with_capacity(slots.len());
    for (file, slot) in files.iter().zip(slots.into_iter()) {
        wait_until_media_url_fetchable(&slot.url).await?;
        uploaded.push(HiggsfieldUploadedMedia {
            media_id: slot.media_id,
            url: slot.url,
            content_type: slot
                .content_type
                .unwrap_or_else(|| file.content_type.clone()),
        });
    }

    Ok(uploaded)
}

pub fn extract_provider_job_id(raw_json: &Value) -> Option<String> {
    provider_payloads(raw_json).iter().find_map(|payload| {
        [
            "/result/id",
            "/result/job_id",
            "/result/jobId",
            "/id",
            "/job_id",
            "/jobId",
        ]
        .into_iter()
        .find_map(|path| json_string_at(payload, path))
    })
}

pub fn extract_provider_soul_id(raw_json: &Value) -> Option<String> {
    provider_payloads(raw_json).iter().find_map(|payload| {
        [
            "/result/soul_id",
            "/result/soulId",
            "/result/id",
            "/soul_id",
            "/soulId",
            "/id",
        ]
        .into_iter()
        .find_map(|path| json_string_at(payload, path))
    })
}

pub fn extract_provider_status(raw_json: &Value) -> Option<String> {
    provider_payloads(raw_json).iter().find_map(|payload| {
        ["/result/status", "/result/state", "/status", "/state"]
            .into_iter()
            .find_map(|path| json_string_at(payload, path))
            .map(|value| value.trim().to_ascii_lowercase())
    })
}

fn parse_mcp_response_text(response_text: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(response_text.trim()) {
        return value;
    }

    let mut last_json = None;
    let mut data_lines = Vec::new();
    for line in response_text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if let Some(parsed) = parse_sse_data_lines(&data_lines) {
                last_json = Some(parsed);
            }
            data_lines.clear();
            continue;
        }
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_string());
        }
    }
    if let Some(parsed) = parse_sse_data_lines(&data_lines) {
        last_json = Some(parsed);
    }

    last_json.unwrap_or_else(|| json!({ "rawText": response_text }))
}

fn parse_sse_data_lines(data_lines: &[String]) -> Option<Value> {
    if data_lines.is_empty() {
        return None;
    }
    let data = data_lines.join("\n");
    if data.trim() == "[DONE]" {
        return None;
    }
    serde_json::from_str::<Value>(&data).ok()
}

async fn put_media_bytes(
    upload_url: &str,
    content_type: &str,
    bytes: &[u8],
) -> Result<(), HiggsfieldMcpError> {
    let headers = Headers::new();
    headers.set("content-type", content_type)?;

    let body = js_sys::Uint8Array::from(bytes);
    let mut init = RequestInit::new();
    init.with_method(Method::Put)
        .with_headers(headers)
        .with_body(Some(JsValue::from(body)));

    let request = Request::new_with_init(upload_url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    if status >= 400 {
        let text = response.text().await.unwrap_or_default();
        return Err(HiggsfieldMcpError::HttpStatus {
            status,
            raw_json: Some(json!({ "rawText": text })),
        });
    }

    Ok(())
}

async fn wait_until_media_url_fetchable(url: &str) -> Result<(), HiggsfieldMcpError> {
    for attempt in 1..=MEDIA_READY_ATTEMPTS {
        let request = Request::new(url, Method::Get)?;
        if let Ok(response) = Fetch::Request(request).send().await {
            if response.status_code() < 400 {
                return Ok(());
            }
        }
        if attempt < MEDIA_READY_ATTEMPTS {
            Delay::from(MEDIA_READY_DELAY).await;
        }
    }

    Err(HiggsfieldMcpError::MediaUrlNotReady {
        url: url.to_string(),
    })
}

fn extract_media_upload_slots(
    raw_json: &Value,
) -> Result<Vec<MediaUploadSlot>, HiggsfieldMcpError> {
    let mut slots = Vec::new();
    for payload in provider_payloads(raw_json) {
        collect_media_upload_slots(&payload, &mut slots);
    }

    if slots.is_empty() {
        return Err(HiggsfieldMcpError::InvalidResponse {
            message: "media_upload response did not include upload slots".to_string(),
            raw_json: raw_json.clone(),
        });
    }

    Ok(slots)
}

fn collect_media_upload_slots(payload: &Value, slots: &mut Vec<MediaUploadSlot>) {
    if let Some(items) = payload.as_array() {
        for item in items {
            push_media_upload_slot(item, slots);
        }
    }

    for path in [
        "/result/files",
        "/result/uploads",
        "/result/media",
        "/result/data",
        "/files",
        "/uploads",
        "/media",
        "/data/files",
        "/data/uploads",
        "/data/media",
        "/data",
    ] {
        let Some(items) = payload.pointer(path).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            push_media_upload_slot(item, slots);
        }
    }

    push_media_upload_slot(payload, slots);
}

fn push_media_upload_slot(value: &Value, slots: &mut Vec<MediaUploadSlot>) {
    if let Some(slot) = media_upload_slot_from_value(value) {
        if !slots
            .iter()
            .any(|existing| existing.media_id == slot.media_id)
        {
            slots.push(slot);
        }
    }
}

fn media_upload_slot_from_value(value: &Value) -> Option<MediaUploadSlot> {
    let media_id = [
        "/media_id",
        "/mediaId",
        "/id",
        "/result/media_id",
        "/result/mediaId",
        "/result/id",
    ]
    .into_iter()
    .find_map(|path| json_string_at(value, path))?;
    let upload_url = [
        "/upload_url",
        "/uploadUrl",
        "/presigned_url",
        "/presignedUrl",
        "/result/upload_url",
        "/result/uploadUrl",
    ]
    .into_iter()
    .find_map(|path| json_string_at(value, path))?;
    let url = [
        "/url",
        "/public_url",
        "/publicUrl",
        "/cdn_url",
        "/cdnUrl",
        "/result/url",
        "/result/public_url",
        "/result/publicUrl",
    ]
    .into_iter()
    .find_map(|path| json_string_at(value, path))?;
    let content_type = ["/content_type", "/contentType", "/result/content_type"]
        .into_iter()
        .find_map(|path| json_string_at(value, path));

    Some(MediaUploadSlot {
        media_id,
        upload_url,
        url,
        content_type,
    })
}

fn provider_payloads(raw_json: &Value) -> Vec<Value> {
    let mut payloads = vec![raw_json.clone()];
    collect_mcp_content_payloads(raw_json, &mut payloads, 0);
    payloads
}

fn collect_mcp_content_payloads(value: &Value, payloads: &mut Vec<Value>, depth: u8) {
    if depth >= 3 {
        return;
    }

    for path in ["/result/content", "/content"] {
        let Some(content) = value.pointer(path).and_then(Value::as_array) else {
            continue;
        };
        for item in content {
            let Some(text) = item
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            payloads.push(parsed.clone());
            collect_mcp_content_payloads(&parsed, payloads, depth + 1);
        }
    }
}

fn json_string_at(value: &Value, path: &str) -> Option<String> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::{
        extract_media_upload_slots, extract_provider_job_id, extract_provider_soul_id,
        parse_mcp_response_text,
    };
    use serde_json::json;

    #[test]
    fn parses_mcp_text_event_stream_response_json() {
        let parsed = parse_mcp_response_text(
            r#"event: message
data: {"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"{\"id\":\"hf_job_1\"}"}]}}

"#,
        );

        assert_eq!(parsed["result"]["content"][0]["type"], json!("text"));
        assert_eq!(
            parsed["result"]["content"][0]["text"],
            json!(r#"{"id":"hf_job_1"}"#)
        );
    }

    #[test]
    fn extracts_provider_ids_from_mcp_content_text_wrappers() {
        let wrapped = json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "{\"soul_id\":\"soul_1\",\"id\":\"hf_job_1\"}"
                }]
            }
        });

        assert_eq!(
            extract_provider_job_id(&wrapped),
            Some("hf_job_1".to_string())
        );
        assert_eq!(
            extract_provider_soul_id(&wrapped),
            Some("soul_1".to_string())
        );
    }

    #[test]
    fn extracts_media_upload_slots_from_mcp_content_text_array() {
        let wrapped = json!({
            "result": {
                "content": [{
                    "type": "text",
                    "text": "[{\"id\":\"media_1\",\"upload_url\":\"https://upload.example/1\",\"url\":\"https://cdn.example/1.jpg\",\"content_type\":\"image/jpeg\"}]"
                }]
            }
        });

        let slots = extract_media_upload_slots(&wrapped).expect("upload slots");

        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].media_id, "media_1");
        assert_eq!(slots[0].upload_url, "https://upload.example/1");
        assert_eq!(slots[0].url, "https://cdn.example/1.jpg");
        assert_eq!(slots[0].content_type.as_deref(), Some("image/jpeg"));
    }

    #[test]
    fn extracts_media_upload_slots_from_raw_top_level_array() {
        let raw = json!([
            {
                "id": "media_1",
                "upload_url": "https://upload.example/1",
                "cdn_url": "https://cdn.example/1.jpg"
            },
            {
                "media_id": "media_2",
                "uploadUrl": "https://upload.example/2",
                "publicUrl": "https://cdn.example/2.jpg"
            }
        ]);

        let slots = extract_media_upload_slots(&raw).expect("upload slots");

        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].media_id, "media_1");
        assert_eq!(slots[0].url, "https://cdn.example/1.jpg");
        assert_eq!(slots[1].media_id, "media_2");
        assert_eq!(slots[1].upload_url, "https://upload.example/2");
    }

    #[test]
    fn extracts_media_upload_slots_from_existing_nested_shapes() {
        let raw = json!({
            "result": {
                "files": [{
                    "mediaId": "media_1",
                    "presignedUrl": "https://upload.example/1",
                    "publicUrl": "https://cdn.example/1.jpg"
                }]
            }
        });

        let slots = extract_media_upload_slots(&raw).expect("upload slots");

        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].media_id, "media_1");
        assert_eq!(slots[0].upload_url, "https://upload.example/1");
        assert_eq!(slots[0].url, "https://cdn.example/1.jpg");
    }
}
