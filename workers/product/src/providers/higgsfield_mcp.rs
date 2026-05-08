use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use wasm_bindgen::JsValue;
use worker::{Fetch, Headers, Method, Request, RequestInit};

const HIGGSFIELD_MCP_URL: &str = "https://mcp.higgsfield.ai/mcp";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiggsfieldMcpResponse {
    pub status: u16,
    pub raw_json: Value,
}

#[derive(Debug, Error)]
pub enum HiggsfieldMcpError {
    #[error("higgsfield mcp endpoint returned status {status}")]
    HttpStatus {
        status: u16,
        raw_json: Option<Value>,
    },
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
    let raw_json = response.json::<Value>().await.unwrap_or_else(|_| json!({}));

    if status >= 400 {
        return Err(HiggsfieldMcpError::HttpStatus {
            status,
            raw_json: Some(raw_json),
        });
    }

    Ok(HiggsfieldMcpResponse { status, raw_json })
}
