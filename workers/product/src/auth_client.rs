use crate::services::accounts::VerifiedIdentity;
use serde::Deserialize;
use worker::{Error, Headers, Method, Request, RequestInit, Result as WorkerResult, RouteContext};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthVerifyResponse {
    pub user_id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub plan: String,
    pub entitlements: AuthEntitlements,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthEntitlements {
    pub max_active_clones: u32,
    pub generation_priority: String,
    pub watermark_exports: bool,
}

impl AuthVerifyResponse {
    pub fn verified_identity(&self) -> VerifiedIdentity {
        VerifiedIdentity {
            user_id: self.user_id.clone(),
            email: self.email.clone(),
            name: self.name.clone(),
            plan: self.plan.clone(),
            max_active_clones: self.entitlements.max_active_clones,
        }
    }
}

pub async fn verify_session(
    ctx: &RouteContext<()>,
    original_headers: &Headers,
) -> WorkerResult<Option<AuthVerifyResponse>> {
    let headers = Headers::new();
    headers.set("content-type", "application/json")?;
    if let Some(cookie) = original_headers.get("cookie")? {
        headers.set("cookie", &cookie)?;
    }

    let mut init = RequestInit::new();
    init.with_method(Method::Post).with_headers(headers);

    let request = Request::new_with_init("https://auth.internal/internal/session/verify", &init)?;
    let mut response = ctx
        .env
        .service("AUTH_SERVICE")?
        .fetch_request(request)
        .await?;
    let status = response.status_code();

    if status == 401 {
        return Ok(None);
    }

    if status >= 400 {
        return Err(Error::RustError(format!(
            "AUTH_SERVICE session verification failed with status {status}"
        )));
    }

    Ok(Some(response.json::<AuthVerifyResponse>().await?))
}
