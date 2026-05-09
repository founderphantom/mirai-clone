use crate::auth_client::verify_session;
use crate::db;
use crate::http::error::ApiError;
use crate::services::accounts::{
    account_checkout_enabled, account_entitlement_snapshot, account_portal_enabled,
    account_usage_limits, upsert_account_from_identity, EntitlementSnapshot, UsageLimits,
    VerifiedIdentity,
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
#[serde(rename_all = "camelCase")]
struct AccountUser {
    id: String,
    email: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
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
    let response = build_account_response(identity, active_clones, billing_metadata(&ctx));

    Response::from_json(&response)
}

#[derive(Debug, Serialize)]
struct AccountUsageResponse {
    clones: Vec<UsageBucket>,
    generations: Vec<UsageBucket>,
    media: Vec<UsageBucket>,
}

#[derive(Debug, Deserialize, Serialize)]
struct UsageBucket {
    status: Option<String>,
    kind: Option<String>,
    count: u32,
}

pub async fn get_usage(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };

    let db = ctx.env.d1("DB")?;
    let user_id = auth.user_id.as_str();

    let clones = db::all::<UsageBucket>(
        &db,
        r#"
        SELECT status, NULL AS kind, COUNT(*) AS count
        FROM clone_profiles
        WHERE user_id = ?
          AND deleted_at IS NULL
        GROUP BY status
        ORDER BY status
        "#,
        vec![json!(user_id)],
    )
    .await?;
    let generations = db::all::<UsageBucket>(
        &db,
        r#"
        SELECT status, NULL AS kind, COUNT(*) AS count
        FROM generation_jobs
        WHERE user_id = ?
        GROUP BY status
        ORDER BY status
        "#,
        vec![json!(user_id)],
    )
    .await?;
    let media = db::all::<UsageBucket>(
        &db,
        r#"
        SELECT NULL AS status, kind, COUNT(*) AS count
        FROM media_assets
        WHERE user_id = ?
          AND deleted_at IS NULL
        GROUP BY kind
        ORDER BY kind
        "#,
        vec![json!(user_id)],
    )
    .await?;

    Response::from_json(&AccountUsageResponse {
        clones,
        generations,
        media,
    })
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
    billing: BillingMetadata,
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
        billing,
    }
}

fn billing_metadata(ctx: &RouteContext<()>) -> BillingMetadata {
    let checkout_enabled = env_var(ctx, "CHECKOUT_ENABLED");
    let portal_enabled = env_var(ctx, "PORTAL_ENABLED");
    let pro_product_id = env_var(ctx, "POLAR_PRO_PRODUCT_ID");
    let studio_product_id = env_var(ctx, "POLAR_STUDIO_PRODUCT_ID");
    let polar_access_token = env_var(ctx, "POLAR_ACCESS_TOKEN");

    BillingMetadata {
        checkout_enabled: account_checkout_enabled(
            checkout_enabled.as_deref(),
            polar_access_token.as_deref(),
            pro_product_id.as_deref(),
            studio_product_id.as_deref(),
        ),
        portal_enabled: account_portal_enabled(
            portal_enabled.as_deref(),
            polar_access_token.as_deref(),
        ),
        server: polar_server(ctx),
    }
}

fn polar_server(ctx: &RouteContext<()>) -> String {
    ctx.var("POLAR_SERVER")
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "sandbox".to_string())
}

fn env_var(ctx: &RouteContext<()>, name: &str) -> Option<String> {
    ctx.var(name)
        .ok()
        .map(|value| value.to_string())
        .filter(|value| !value.trim().is_empty())
}
