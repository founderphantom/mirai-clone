use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use wasm_bindgen::JsValue;
use worker::{durable_object, Env, Fetch, Headers, Method, Request, RequestInit, Response, State};

const DEVICE_AUTH_BASE_URL: &str = "https://fnf-device-auth.higgsfield.ai";
const HIGGSFIELD_CREDENTIALS_BINDING: &str = "HIGGSFIELD_CREDENTIALS";
const HIGGSFIELD_CREDENTIALS_OBJECT_NAME: &str = "higgsfield-credentials";

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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiggsfieldAccessToken {
    #[serde(alias = "access_token")]
    pub access_token: String,
    #[serde(default, alias = "token_type")]
    pub token_type: Option<String>,
    #[serde(default, alias = "expires_in")]
    pub expires_in: Option<u64>,
    #[serde(default, alias = "user_id")]
    pub validated_user_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HiggsfieldAuthPhase {
    CredentialsObject,
    Refresh,
    Validate,
}

impl HiggsfieldAuthPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::CredentialsObject => "credentials_object",
            Self::Refresh => "refresh",
            Self::Validate => "validate",
        }
    }
}

impl std::fmt::Display for HiggsfieldAuthPhase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Error)]
pub enum HiggsfieldAuthError {
    #[error("higgsfield refresh secret is missing: {secret_name}")]
    MissingSecret { secret_name: String },
    #[error("higgsfield auth endpoint returned status {status} during {phase}")]
    HttpStatus {
        phase: HiggsfieldAuthPhase,
        status: u16,
    },
    #[error("higgsfield credentials object returned status {status} during {phase}: {message}")]
    CredentialsObjectStatus {
        phase: HiggsfieldAuthPhase,
        status: u16,
        message: String,
    },
    #[error("higgsfield auth request failed: {0}")]
    Worker(#[from] worker::Error),
    #[error("failed to serialize higgsfield auth request: {0}")]
    Serde(#[from] serde_json::Error),
}

impl HiggsfieldAuthError {
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::HttpStatus { status, .. } | Self::CredentialsObjectStatus { status, .. } => {
                Some(*status)
            }
            _ => None,
        }
    }

    pub fn phase(&self) -> Option<HiggsfieldAuthPhase> {
        match self {
            Self::HttpStatus { phase, .. } | Self::CredentialsObjectStatus { phase, .. } => {
                Some(*phase)
            }
            _ => None,
        }
    }

    pub fn sanitized_message(&self) -> String {
        match self {
            Self::MissingSecret { secret_name } => {
                format!("Higgsfield provider auth failed: phase=secret status=missing secret={secret_name}")
            }
            Self::HttpStatus { phase, status } => {
                format!("Higgsfield provider auth failed: phase={phase} status={status}")
            }
            Self::CredentialsObjectStatus {
                phase,
                status,
                message,
            } => format!(
                "Higgsfield provider auth failed: phase={phase} status={status} detail={}",
                sanitize_error_detail(message)
            ),
            Self::Worker(error) => format!(
                "Higgsfield provider auth failed: phase=worker status=unavailable detail={}",
                sanitize_error_detail(&error.to_string())
            ),
            Self::Serde(error) => format!(
                "Higgsfield provider auth failed: phase=serde status=invalid_response detail={}",
                sanitize_error_detail(&error.to_string())
            ),
        }
    }
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsAccessTokenRequest {
    provider_account_id: String,
    refresh_secret_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsErrorResponse {
    error: String,
    phase: HiggsfieldAuthPhase,
    status: Option<u16>,
    message: String,
    secret_name: Option<String>,
}

#[durable_object(fetch)]
pub struct HiggsfieldCredentials {
    state: State,
    env: Env,
}

impl worker::DurableObject for HiggsfieldCredentials {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> worker::Result<Response> {
        if req.method() != Method::Post || req.path() != "/access-token" {
            return Response::builder()
                .with_status(404)
                .from_json(&json!({ "error": "not_found" }));
        }

        let input = match req.json::<CredentialsAccessTokenRequest>().await {
            Ok(input) => input,
            Err(error) => {
                return Response::builder()
                    .with_status(400)
                    .from_json(&CredentialsErrorResponse {
                        error: "invalid_request".to_string(),
                        phase: HiggsfieldAuthPhase::CredentialsObject,
                        status: Some(400),
                        message: sanitize_error_detail(&error.to_string()),
                        secret_name: None,
                    });
            }
        };

        match self
            .access_token_for_provider(
                input.provider_account_id.trim(),
                input.refresh_secret_name.trim(),
            )
            .await
        {
            Ok(token) => Response::from_json(&token),
            Err(error) => higgsfield_auth_error_response(&error),
        }
    }
}

impl HiggsfieldCredentials {
    async fn access_token_for_provider(
        &self,
        provider_account_id: &str,
        refresh_secret_name: &str,
    ) -> Result<HiggsfieldAccessToken, HiggsfieldAuthError> {
        let storage = self.state.storage();
        let storage_key = credentials_storage_key(provider_account_id);

        if let Some(stored_refresh_token) =
            optional_stored_refresh_token(&storage, &storage_key).await?
        {
            match refresh_validate_rotate_and_store(&storage, &storage_key, &stored_refresh_token)
                .await
            {
                Ok(access_token) => return Ok(access_token),
                Err(error) if should_retry_from_secret(&error) => {
                    if let Some(latest_refresh_token) =
                        optional_stored_refresh_token(&storage, &storage_key).await?
                    {
                        if stored_refresh_token_was_replaced(
                            Some(&latest_refresh_token),
                            &stored_refresh_token,
                        ) {
                            match refresh_validate_rotate_and_store(
                                &storage,
                                &storage_key,
                                &latest_refresh_token,
                            )
                            .await
                            {
                                Ok(access_token) => return Ok(access_token),
                                Err(error) if should_retry_from_secret(&error) => {
                                    delete_stored_refresh_token_if_unchanged(
                                        &storage,
                                        &storage_key,
                                        &latest_refresh_token,
                                    )
                                    .await?;
                                }
                                Err(error) => return Err(error),
                            }
                        } else {
                            delete_stored_refresh_token_if_unchanged(
                                &storage,
                                &storage_key,
                                &stored_refresh_token,
                            )
                            .await?;
                        }
                    }
                }
                Err(error) => return Err(error),
            }
        }

        let fallback_refresh_token = self
            .env
            .secret(refresh_secret_name)
            .map_err(|_| HiggsfieldAuthError::MissingSecret {
                secret_name: refresh_secret_name.to_string(),
            })?
            .to_string();
        refresh_validate_rotate_and_store(&storage, &storage_key, &fallback_refresh_token).await
    }
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

pub async fn provider_account_access_token(
    env: &Env,
    provider_account_id: &str,
    refresh_secret_name: &str,
) -> Result<HiggsfieldAccessToken, HiggsfieldAuthError> {
    let body = CredentialsAccessTokenRequest {
        provider_account_id: provider_account_id.to_string(),
        refresh_secret_name: refresh_secret_name.to_string(),
    };
    let headers = Headers::new();
    headers.set("content-type", "application/json")?;
    headers.set("accept", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&serde_json::to_string(&body)?)));

    let request = Request::new_with_init(
        "https://higgsfield-credentials.internal/access-token",
        &init,
    )?;
    let namespace = env.durable_object(HIGGSFIELD_CREDENTIALS_BINDING)?;
    let stub = namespace.get_by_name(HIGGSFIELD_CREDENTIALS_OBJECT_NAME)?;
    let mut response = stub.fetch_with_request(request).await?;
    let status = response.status_code();
    let response_text = response.text().await.unwrap_or_default();
    if status >= 400 {
        return Err(credentials_error_from_response(status, &response_text));
    }

    Ok(serde_json::from_str::<HiggsfieldAccessToken>(
        &response_text,
    )?)
}

async fn refresh_access_token_value(
    refresh_token: &str,
) -> Result<HiggsfieldTokenResponse, HiggsfieldAuthError> {
    post_json(
        &format!("{DEVICE_AUTH_BASE_URL}/refresh"),
        &RefreshRequest { refresh_token },
        HiggsfieldAuthPhase::Refresh,
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
        HiggsfieldAuthPhase::Validate,
    )
    .await
}

async fn post_json<TRequest, TResponse>(
    url: &str,
    body: &TRequest,
    phase: HiggsfieldAuthPhase,
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
        return Err(HiggsfieldAuthError::HttpStatus { phase, status });
    }

    Ok(response.json::<TResponse>().await?)
}

async fn refresh_validate_and_rotate(
    refresh_token: &str,
) -> Result<(HiggsfieldAccessToken, Option<String>), HiggsfieldAuthError> {
    let token_response = refresh_access_token_value(refresh_token).await?;
    let validate_response = validate_access_token(&token_response.access_token).await?;
    let rotated_refresh_token = rotated_refresh_token_from_response(&token_response);
    let access_token = HiggsfieldAccessToken {
        access_token: token_response.access_token,
        token_type: token_response.token_type,
        expires_in: token_response.expires_in,
        validated_user_id: Some(validate_response.user_id),
    };

    Ok((access_token, rotated_refresh_token))
}

async fn refresh_validate_rotate_and_store(
    storage: &worker::Storage,
    storage_key: &str,
    refresh_token: &str,
) -> Result<HiggsfieldAccessToken, HiggsfieldAuthError> {
    let (access_token, rotated_refresh_token) = refresh_validate_and_rotate(refresh_token).await?;
    if let Some(rotated_refresh_token) = rotated_refresh_token {
        storage.put(storage_key, rotated_refresh_token).await?;
    }

    Ok(access_token)
}

async fn optional_stored_refresh_token(
    storage: &worker::Storage,
    storage_key: &str,
) -> worker::Result<Option<String>> {
    match storage.get::<String>(storage_key).await {
        Ok(value) => {
            let token = value.trim();
            if token.is_empty() {
                Ok(None)
            } else {
                Ok(Some(token.to_string()))
            }
        }
        Err(error) if durable_storage_missing_error(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn durable_storage_missing_error(error: &worker::Error) -> bool {
    error.to_string().contains("No such value in storage")
}

async fn delete_stored_refresh_token_if_unchanged(
    storage: &worker::Storage,
    storage_key: &str,
    rejected_refresh_token: &str,
) -> worker::Result<()> {
    let storage_key = storage_key.to_string();
    let rejected_refresh_token = rejected_refresh_token.to_string();

    storage
        .transaction(move |transaction| async move {
            match transaction.get::<String>(&storage_key).await {
                Ok(current_refresh_token)
                    if !stored_refresh_token_was_replaced(
                        Some(&current_refresh_token),
                        &rejected_refresh_token,
                    ) =>
                {
                    let _ = transaction.delete(&storage_key).await?;
                    Ok(())
                }
                Ok(_) => Ok(()),
                Err(error) if durable_storage_missing_error(&error) => Ok(()),
                Err(error) => Err(error),
            }
        })
        .await
}

fn stored_refresh_token_was_replaced(current_refresh_token: Option<&str>, rejected: &str) -> bool {
    current_refresh_token
        .map(str::trim)
        .filter(|current| !current.is_empty())
        .is_some_and(|current| current != rejected.trim())
}

fn rotated_refresh_token_from_response(response: &HiggsfieldTokenResponse) -> Option<String> {
    response
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn credentials_storage_key(provider_account_id: &str) -> String {
    format!("refresh_token:{}", provider_account_id.trim())
}

fn should_retry_from_secret(error: &HiggsfieldAuthError) -> bool {
    matches!(
        error,
        HiggsfieldAuthError::HttpStatus {
            phase: HiggsfieldAuthPhase::Refresh,
            status: 401 | 422,
        }
    )
}

fn higgsfield_auth_error_response(error: &HiggsfieldAuthError) -> worker::Result<Response> {
    let (status, body) = match error {
        HiggsfieldAuthError::MissingSecret { secret_name } => (
            500,
            CredentialsErrorResponse {
                error: "higgsfield_secret_missing".to_string(),
                phase: HiggsfieldAuthPhase::CredentialsObject,
                status: Some(500),
                message: error.sanitized_message(),
                secret_name: Some(secret_name.clone()),
            },
        ),
        HiggsfieldAuthError::HttpStatus { phase, status } => (
            *status,
            CredentialsErrorResponse {
                error: "higgsfield_auth_http_status".to_string(),
                phase: *phase,
                status: Some(*status),
                message: error.sanitized_message(),
                secret_name: None,
            },
        ),
        other => (
            502,
            CredentialsErrorResponse {
                error: "higgsfield_credentials_error".to_string(),
                phase: other
                    .phase()
                    .unwrap_or(HiggsfieldAuthPhase::CredentialsObject),
                status: Some(502),
                message: other.sanitized_message(),
                secret_name: None,
            },
        ),
    };

    Response::builder().with_status(status).from_json(&body)
}

fn credentials_error_from_response(status: u16, response_text: &str) -> HiggsfieldAuthError {
    let parsed = serde_json::from_str::<CredentialsErrorResponse>(response_text).ok();
    match parsed {
        Some(CredentialsErrorResponse {
            error,
            phase,
            status: body_status,
            message,
            secret_name,
        }) if error == "higgsfield_secret_missing" => HiggsfieldAuthError::MissingSecret {
            secret_name: secret_name.unwrap_or_else(|| "unknown".to_string()),
        },
        Some(CredentialsErrorResponse {
            phase,
            status: body_status,
            ..
        }) if matches!(status, 401 | 422) || matches!(body_status, Some(401 | 422)) => {
            HiggsfieldAuthError::HttpStatus {
                phase,
                status: body_status.unwrap_or(status),
            }
        }
        Some(CredentialsErrorResponse {
            phase,
            status: body_status,
            message,
            ..
        }) => HiggsfieldAuthError::CredentialsObjectStatus {
            phase,
            status: body_status.unwrap_or(status),
            message,
        },
        None => HiggsfieldAuthError::CredentialsObjectStatus {
            phase: HiggsfieldAuthPhase::CredentialsObject,
            status,
            message: sanitize_error_detail(response_text),
        },
    }
}

fn sanitize_error_detail(message: &str) -> String {
    let compact = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated = compact.chars().take(240).collect::<String>();
    if truncated.len() < compact.len() {
        format!("{truncated}...")
    } else if compact.is_empty() {
        "unavailable".to_string()
    } else {
        compact
    }
}

#[cfg(test)]
mod tests {
    use super::{
        credentials_error_from_response, credentials_storage_key,
        rotated_refresh_token_from_response, sanitize_error_detail, should_retry_from_secret,
        stored_refresh_token_was_replaced, HiggsfieldAuthError, HiggsfieldAuthPhase,
        RefreshRequest, ValidateRequest,
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
    fn access_token_response_does_not_require_refresh_token() {
        let response: super::HiggsfieldAccessToken = serde_json::from_value(json!({
            "access_token": "access_token",
            "token_type": "Bearer",
            "expires_in": 3600,
            "user_id": "user_1"
        }))
        .unwrap();

        assert_eq!(response.access_token, "access_token");
        assert_eq!(response.validated_user_id.as_deref(), Some("user_1"));
    }

    #[test]
    fn credentials_storage_is_keyed_by_provider_account_id() {
        assert_eq!(
            credentials_storage_key(" pa_higgsfield_founder "),
            "refresh_token:pa_higgsfield_founder"
        );
    }

    #[test]
    fn rotated_refresh_token_is_captured_for_durable_storage() {
        let response: super::HiggsfieldTokenResponse = serde_json::from_value(json!({
            "access_token": "access_token",
            "refresh_token": " hfr_next "
        }))
        .unwrap();

        assert_eq!(
            rotated_refresh_token_from_response(&response),
            Some("hfr_next".to_string())
        );
    }

    #[test]
    fn stored_refresh_token_rejection_falls_back_to_secret_once() {
        assert!(should_retry_from_secret(&HiggsfieldAuthError::HttpStatus {
            phase: HiggsfieldAuthPhase::Refresh,
            status: 401,
        }));
        assert!(should_retry_from_secret(&HiggsfieldAuthError::HttpStatus {
            phase: HiggsfieldAuthPhase::Refresh,
            status: 422,
        }));
        assert!(!should_retry_from_secret(
            &HiggsfieldAuthError::HttpStatus {
                phase: HiggsfieldAuthPhase::Validate,
                status: 401,
            }
        ));
        assert!(!should_retry_from_secret(
            &HiggsfieldAuthError::HttpStatus {
                phase: HiggsfieldAuthPhase::Refresh,
                status: 500,
            }
        ));
    }

    #[test]
    fn stale_stored_refresh_token_detection_ignores_same_token() {
        assert!(!stored_refresh_token_was_replaced(None, "hfr_old"));
        assert!(!stored_refresh_token_was_replaced(
            Some("hfr_old"),
            " hfr_old "
        ));
        assert!(stored_refresh_token_was_replaced(
            Some(" hfr_rotated "),
            "hfr_old"
        ));
    }

    #[test]
    fn credentials_object_http_status_maps_back_to_auth_status() {
        let error = credentials_error_from_response(
            401,
            &json!({
                "error": "higgsfield_auth_http_status",
                "phase": "refresh",
                "status": 401,
                "message": "Higgsfield provider auth failed: phase=refresh status=401"
            })
            .to_string(),
        );

        assert!(matches!(
            error,
            HiggsfieldAuthError::HttpStatus {
                phase: HiggsfieldAuthPhase::Refresh,
                status: 401,
            }
        ));
    }

    #[test]
    fn sanitized_auth_message_keeps_phase_and_status_without_raw_rust_error() {
        let message = HiggsfieldAuthError::HttpStatus {
            phase: HiggsfieldAuthPhase::Refresh,
            status: 422,
        }
        .sanitized_message();

        assert_eq!(
            message,
            "Higgsfield provider auth failed: phase=refresh status=422"
        );
        assert!(!message.contains("RustError"));
        assert_eq!(
            sanitize_error_detail("  one\n two\tthree  "),
            "one two three"
        );
    }

    #[test]
    fn provider_metadata_keeps_only_secret_reference_contract() {
        let value = json!({
            "refreshToken": "HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER"
        });
        assert_eq!(
            value["refreshToken"],
            json!("HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER")
        );
        assert!(value.get("refreshTokenValue").is_none());
    }
}
