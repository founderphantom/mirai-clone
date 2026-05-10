use serde::{Deserialize, Serialize};
use thiserror::Error;
use wasm_bindgen::JsValue;
use worker::{Env, Fetch, Headers, Method, Request, RequestInit};

const DEVICE_AUTH_BASE_URL: &str = "https://fnf-device-auth.higgsfield.ai";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiggsfieldTokenResponse {
    #[serde(alias = "access_token")]
    pub access_token: String,
    #[serde(default, alias = "token_type")]
    pub token_type: Option<String>,
    #[serde(default, alias = "expires_in")]
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiggsfieldValidateResponse {
    #[serde(default)]
    pub valid: bool,
    #[serde(default, alias = "expires_at")]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
}

#[derive(Debug, Error)]
pub enum HiggsfieldAuthError {
    #[error("higgsfield refresh secret is missing: {secret_name}")]
    MissingSecret { secret_name: String },
    #[error("higgsfield auth endpoint returned status {status}")]
    HttpStatus { status: u16 },
    #[error("higgsfield auth request failed: {0}")]
    Worker(#[from] worker::Error),
    #[error("failed to serialize higgsfield auth request: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshRequest<'a> {
    refresh_token: &'a str,
}

pub async fn refresh_access_token(
    env: &Env,
    refresh_secret_name: &str,
) -> Result<HiggsfieldTokenResponse, HiggsfieldAuthError> {
    let refresh_secret =
        env.secret(refresh_secret_name)
            .map_err(|_| HiggsfieldAuthError::MissingSecret {
                secret_name: refresh_secret_name.to_string(),
            })?;
    let refresh_token = refresh_secret.to_string();

    post_json(
        &format!("{DEVICE_AUTH_BASE_URL}/refresh"),
        &RefreshRequest {
            refresh_token: &refresh_token,
        },
    )
    .await
}

pub async fn validate_access_token(
    access_token: &str,
) -> Result<HiggsfieldValidateResponse, HiggsfieldAuthError> {
    let headers = Headers::new();
    headers.set("authorization", &format!("Bearer {access_token}"))?;
    headers.set("accept", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post).with_headers(headers);

    let request = Request::new_with_init(&format!("{DEVICE_AUTH_BASE_URL}/validate"), &init)?;
    let mut response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    if status >= 400 {
        return Err(HiggsfieldAuthError::HttpStatus { status });
    }

    Ok(response.json::<HiggsfieldValidateResponse>().await?)
}

async fn post_json<TRequest, TResponse>(
    url: &str,
    body: &TRequest,
) -> Result<TResponse, HiggsfieldAuthError>
where
    TRequest: Serialize,
    TResponse: for<'de> Deserialize<'de>,
{
    let headers = Headers::new();
    headers.set("content-type", "application/json")?;
    headers.set("accept", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&serde_json::to_string(body)?)));

    let request = Request::new_with_init(url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    if status >= 400 {
        return Err(HiggsfieldAuthError::HttpStatus { status });
    }

    Ok(response.json::<TResponse>().await?)
}
