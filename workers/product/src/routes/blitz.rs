use crate::auth_client::verify_session;
use crate::http::error::ApiError;
use crate::services::blitz;
use crate::services::generation_usage::usage_snapshot;
use serde::Deserialize;
use worker::{Error, Request, Response, Result as WorkerResult, RouteContext, Url};

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
    let response = match blitz::current_batch(&db, &auth.user_id, &clone_id, usage).await {
        Ok(response) => response,
        Err(error) => return map_or_return_blitz_error(error),
    };
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
        Err(error) => map_or_return_blitz_error(error),
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

pub fn map_blitz_service_error(error: &Error) -> Option<ApiError> {
    let Error::RustError(code) = error else {
        return None;
    };

    match code.as_str() {
        "clone_not_found" => Some(ApiError::not_found(
            "clone_not_found",
            "Clone profile was not found.",
        )),
        "blitz_batch_not_found" => Some(ApiError::not_found(
            "blitz_batch_not_found",
            "Blitz batch was not found.",
        )),
        "generation_output_not_found" => Some(ApiError::not_found(
            "generation_output_not_found",
            "Blitz output was not found.",
        )),
        "blitz_batch_not_swipeable" => Some(ApiError::bad_request(
            "blitz_batch_not_swipeable",
            "This Blitz batch is not ready for swipes.",
        )),
        "invalid_swipe_action" => Some(ApiError::bad_request(
            "invalid_swipe_action",
            "Swipe action must be like or dislike.",
        )),
        "duplicate_swipe" => Some(ApiError::conflict(
            "duplicate_swipe",
            "This Blitz card was already swiped.",
        )),
        "provider_soul_id_missing" => Some(ApiError::bad_request(
            "provider_soul_id_missing",
            "Soul is not ready for Blitz generation.",
        )),
        _ => None,
    }
}

fn map_or_return_blitz_error(error: Error) -> WorkerResult<Response> {
    if let Some(api_error) = map_blitz_service_error(&error) {
        return api_error.to_response();
    }

    Err(error)
}

#[cfg(test)]
mod tests {
    use super::SwipeRequest;

    #[test]
    fn swipe_request_deserializes_camel_case_fields() {
        let request: SwipeRequest =
            serde_json::from_str(r#"{"batchId":"batch_1","outputId":"output_1","action":"like"}"#)
                .expect("camelCase swipe request should deserialize");

        assert_eq!(request.batch_id, "batch_1");
        assert_eq!(request.output_id, "output_1");
        assert_eq!(request.action, "like");
    }
}
