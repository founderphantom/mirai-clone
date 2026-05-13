use crate::db;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;
use wasm_bindgen::JsValue;
use worker::{D1Database, Env, Fetch, Headers, Method, Request, RequestInit};

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
    #[serde(default, alias = "refresh_token")]
    pub refresh_token: Option<String>,
    #[serde(default, alias = "refresh_expires_in")]
    pub refresh_expires_in: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiggsfieldValidateResponse {
    #[serde(alias = "user_id")]
    pub user_id: String,
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
struct RefreshRequest<'a> {
    #[serde(rename = "refresh_token")]
    refresh_token: &'a str,
}

#[derive(Debug, Serialize)]
struct ValidateRequest<'a> {
    token: &'a str,
}

#[derive(Debug, Deserialize)]
struct ProviderAuthStateRow {
    secret_refs_json: String,
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

    refresh_access_token_value(&refresh_token).await
}

pub async fn refresh_provider_account_access_token(
    db: &D1Database,
    env: &Env,
    provider_account_id: &str,
    refresh_secret_name: &str,
) -> Result<HiggsfieldTokenResponse, HiggsfieldAuthError> {
    let account = db::first::<ProviderAuthStateRow>(
        db,
        r#"
        SELECT secret_refs_json
        FROM provider_accounts
        WHERE id = ?
        "#,
        vec![json!(provider_account_id)],
    )
    .await?;
    let secret_refs_json = account
        .as_ref()
        .map(|row| row.secret_refs_json.as_str())
        .unwrap_or("{}");
    let refresh_token = if has_rotated_refresh_token(secret_refs_json)? {
        refresh_token_for_account(secret_refs_json, "")?
    } else {
        let refresh_secret =
            env.secret(refresh_secret_name)
                .map_err(|_| HiggsfieldAuthError::MissingSecret {
                    secret_name: refresh_secret_name.to_string(),
                })?;
        refresh_token_for_account(secret_refs_json, &refresh_secret.to_string())?
    };

    let response = refresh_access_token_value(&refresh_token).await?;
    if let (Some(_account), Some(rotated_refresh_token)) =
        (account.as_ref(), response.refresh_token.as_deref())
    {
        persist_rotated_refresh_token(
            db,
            provider_account_id,
            secret_refs_json,
            rotated_refresh_token,
            response.refresh_expires_in,
        )
        .await?;
    }

    Ok(response)
}

async fn refresh_access_token_value(
    refresh_token: &str,
) -> Result<HiggsfieldTokenResponse, HiggsfieldAuthError> {
    post_json(
        &format!("{DEVICE_AUTH_BASE_URL}/refresh"),
        &RefreshRequest { refresh_token },
    )
    .await
}

pub async fn validate_access_token(
    access_token: &str,
) -> Result<HiggsfieldValidateResponse, HiggsfieldAuthError> {
    post_json(
        &format!("{DEVICE_AUTH_BASE_URL}/validate"),
        &ValidateRequest {
            token: access_token,
        },
    )
    .await
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

async fn persist_rotated_refresh_token(
    db: &D1Database,
    provider_account_id: &str,
    secret_refs_json: &str,
    rotated_refresh_token: &str,
    refresh_expires_in: Option<u64>,
) -> Result<(), HiggsfieldAuthError> {
    let now = now_iso_string();
    let updated_secret_refs = secret_refs_with_rotated_refresh_token(
        secret_refs_json,
        rotated_refresh_token,
        refresh_expires_in,
        &now,
    )?;
    db::exec(
        db,
        r#"
        UPDATE provider_accounts
        SET secret_refs_json = ?,
            last_auth_check_at = ?,
            updated_at = ?
        WHERE id = ?
        "#,
        vec![
            json!(updated_secret_refs),
            json!(now),
            json!(now),
            json!(provider_account_id),
        ],
    )
    .await?;
    Ok(())
}

fn refresh_token_for_account(
    secret_refs_json: &str,
    fallback_refresh_token: &str,
) -> Result<String, HiggsfieldAuthError> {
    let refs = secret_refs_object(secret_refs_json)?;
    if let Some(token) = refs
        .get("refreshTokenValue")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(token.to_string());
    }
    Ok(fallback_refresh_token.trim().to_string())
}

fn has_rotated_refresh_token(secret_refs_json: &str) -> Result<bool, HiggsfieldAuthError> {
    Ok(!refresh_token_for_account(secret_refs_json, "")?.is_empty())
}

fn secret_refs_with_rotated_refresh_token(
    secret_refs_json: &str,
    rotated_refresh_token: &str,
    refresh_expires_in: Option<u64>,
    now: &str,
) -> Result<String, HiggsfieldAuthError> {
    let mut refs = secret_refs_object(secret_refs_json)?;
    refs.insert(
        "refreshTokenValue".to_string(),
        Value::String(rotated_refresh_token.to_string()),
    );
    refs.insert(
        "refreshTokenUpdatedAt".to_string(),
        Value::String(now.to_string()),
    );
    if let Some(refresh_expires_in) = refresh_expires_in {
        refs.insert(
            "refreshTokenExpiresIn".to_string(),
            json!(refresh_expires_in),
        );
    }

    Ok(Value::Object(refs).to_string())
}

fn secret_refs_object(secret_refs_json: &str) -> Result<Map<String, Value>, HiggsfieldAuthError> {
    if secret_refs_json.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(secret_refs_json)? {
        Value::Object(map) => Ok(map),
        _ => Ok(Map::new()),
    }
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

#[cfg(test)]
mod tests {
    use super::{
        refresh_token_for_account, secret_refs_with_rotated_refresh_token, RefreshRequest,
        ValidateRequest,
    };
    use serde_json::json;

    #[test]
    fn refresh_request_uses_device_auth_payload_name() {
        let payload = serde_json::to_value(RefreshRequest {
            refresh_token: "hfr_test",
        })
        .unwrap();

        assert_eq!(
            payload,
            json!({
                "refresh_token": "hfr_test"
            })
        );
    }

    #[test]
    fn validate_request_uses_device_auth_payload_name() {
        let payload = serde_json::to_value(ValidateRequest {
            token: "access_token",
        })
        .unwrap();

        assert_eq!(
            payload,
            json!({
                "token": "access_token"
            })
        );
    }

    #[test]
    fn token_response_captures_rotated_refresh_token() {
        let response: super::HiggsfieldTokenResponse = serde_json::from_value(json!({
            "access_token": "access_token",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "hfr_next",
            "refresh_expires_in": 2_592_000
        }))
        .unwrap();

        assert_eq!(response.refresh_token.as_deref(), Some("hfr_next"));
        assert_eq!(response.refresh_expires_in, Some(2_592_000));
    }

    #[test]
    fn validate_response_accepts_device_auth_user_id() {
        let response: super::HiggsfieldValidateResponse =
            serde_json::from_value(json!({ "user_id": "user_1" })).unwrap();

        assert_eq!(response.user_id, "user_1");
    }

    #[test]
    fn provider_auth_state_prefers_rotated_refresh_token() {
        let token = refresh_token_for_account(
            r#"{"refreshToken":"HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER","refreshTokenValue":"hfr_rotated"}"#,
            "hfr_secret",
        )
        .unwrap();

        assert_eq!(token, "hfr_rotated");
    }

    #[test]
    fn provider_auth_state_preserves_secret_ref_when_rotated() {
        let merged = secret_refs_with_rotated_refresh_token(
            r#"{"refreshToken":"HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER","other":"value"}"#,
            "hfr_next",
            Some(2_592_000),
            "2026-05-13T03:00:00.000Z",
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();

        assert_eq!(
            value["refreshToken"],
            json!("HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER")
        );
        assert_eq!(value["refreshTokenValue"], json!("hfr_next"));
        assert_eq!(
            value["refreshTokenUpdatedAt"],
            json!("2026-05-13T03:00:00.000Z")
        );
        assert_eq!(value["refreshTokenExpiresIn"], json!(2_592_000));
        assert_eq!(value["other"], json!("value"));
    }
}
