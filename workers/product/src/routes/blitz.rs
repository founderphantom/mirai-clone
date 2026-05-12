use crate::auth_client::verify_session;
use crate::http::error::ApiError;
use crate::services::blitz;
use crate::services::generation_usage::usage_snapshot;
use serde::Deserialize;
use worker::{Request, Response, Result as WorkerResult, RouteContext, Url};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwipeRequest {
    batch_id: String,
    output_id: String,
    action: String,
}

pub async fn current(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let url = req.url()?;
    let clone_id = match read_required_query_param(&url, "clone_id") {
        Ok(value) => value,
        Err(_) => {
            return ApiError::bad_request("missing_clone_id", "clone_id is required.").to_response()
        }
    };
    let db = ctx.env.d1("DB")?;
    let usage = usage_snapshot(&db, &auth.user_id, &auth.plan, 10, 50).await?;
    let response = blitz::current_batch(&db, &auth.user_id, &clone_id, usage).await?;
    Response::from_json(&response)
}

pub async fn swipe(mut req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let input = match req.json::<SwipeRequest>().await {
        Ok(input) => input,
        Err(_) => {
            return ApiError::bad_request(
                "invalid_blitz_swipe_request",
                "Expected batchId, outputId, and action.",
            )
            .to_response()
        }
    };
    let db = ctx.env.d1("DB")?;
    match blitz::record_swipe(
        &db,
        &ctx.env,
        &auth.user_id,
        &input.batch_id,
        &input.output_id,
        &input.action,
    )
    .await
    {
        Ok(response) => Response::from_json(&response),
        Err(error) if error.to_string().contains("duplicate_swipe") => {
            ApiError::bad_request("duplicate_swipe", "This Blitz card was already swiped.")
                .to_response()
        }
        Err(error) if error.to_string().contains("invalid_swipe_action") => ApiError::bad_request(
            "invalid_swipe_action",
            "Swipe action must be like or dislike.",
        )
        .to_response(),
        Err(error) => Err(error),
    }
}

pub async fn history(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let url = req.url()?;
    let clone_id = match read_required_query_param(&url, "clone_id") {
        Ok(value) => value,
        Err(_) => {
            return ApiError::bad_request("missing_clone_id", "clone_id is required.").to_response()
        }
    };
    let limit = parse_history_limit(
        url.query_pairs()
            .find(|(key, _)| key == "limit")
            .map(|(_, value)| value.to_string())
            .as_deref(),
    );
    let db = ctx.env.d1("DB")?;
    Response::from_json(&blitz::history(&db, &auth.user_id, &clone_id, limit).await?)
}

pub fn read_required_query_param(url: &Url, key: &str) -> Result<String, &'static str> {
    url.query_pairs()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or("missing_clone_id")
}

pub fn parse_history_limit(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(10)
        .clamp(1, 50)
}
