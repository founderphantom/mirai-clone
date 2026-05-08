use crate::auth_client::verify_session;
use crate::db;
use crate::http::error::ApiError;
use crate::services::accounts::{
    account_entitlement_snapshot, account_usage_limits, upsert_account_from_identity,
    EntitlementSnapshot, UsageLimits, VerifiedIdentity,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use worker::{Request, Response, Result as WorkerResult, RouteContext};

#[derive(Debug, Serialize)]
struct AccountResponse {
    user: AccountUser,
    plan: String,
    entitlements: EntitlementSnapshot,
    usage: UsageLimits,
    billing: BillingMetadata,
}

#[derive(Debug, Serialize)]
struct AccountUser {
    id: String,
    email: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Serialize)]
struct BillingMetadata {
    checkout_enabled: bool,
    portal_enabled: bool,
    server: String,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u32,
}

pub async fn get_account(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };

    let identity = auth.verified_identity();
    let db = ctx.env.d1("DB")?;
    upsert_account_from_identity(&db, &identity).await?;

    let active_clones = count_active_clones(&db, &identity.user_id).await?;
    let response = build_account_response(identity, active_clones, polar_server(&ctx));

    Response::from_json(&response)
}

async fn count_active_clones(db: &worker::D1Database, user_id: &str) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM clone_profiles
        WHERE user_id = ?
          AND status = 'active'
          AND deleted_at IS NULL
        "#,
        vec![json!(user_id)],
    )
    .await?;

    Ok(row.map(|row| row.count).unwrap_or(0))
}

fn build_account_response(
    identity: VerifiedIdentity,
    active_clones: u32,
    billing_server: String,
) -> AccountResponse {
    let usage = account_usage_limits(&identity, active_clones);
    let entitlements = account_entitlement_snapshot(&identity);

    AccountResponse {
        user: AccountUser {
            id: identity.user_id,
            email: identity.email,
            name: identity.name,
        },
        plan: identity.plan,
        entitlements,
        usage,
        billing: BillingMetadata {
            checkout_enabled: true,
            portal_enabled: true,
            server: billing_server,
        },
    }
}

fn polar_server(ctx: &RouteContext<()>) -> String {
    ctx.var("POLAR_SERVER")
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "sandbox".to_string())
}
