use crate::auth_client::verify_session;
use crate::db;
use crate::http::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use worker::{Request, Response, Result as WorkerResult, RouteContext};

#[derive(Debug, Deserialize, Serialize)]
struct DiscoveryItemResponse {
    id: String,
    title: String,
    platform: String,
    image_url: Option<String>,
    thumbnail_url: Option<String>,
    source_url: Option<String>,
    author_handle: String,
}

#[derive(Debug, Serialize)]
struct DiscoveryFeedResponse {
    items: Vec<DiscoveryItemResponse>,
}

pub async fn discovery_feed(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };

    let source = req
        .url()
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "source")
                .map(|(_, value)| value.to_string())
        })
        .filter(|value| !value.trim().is_empty());

    let db = ctx.env.d1("DB")?;
    let items = if let Some(source) = source {
        db::all::<DiscoveryItemResponse>(
            &db,
            r#"
            SELECT id, title, platform, image_url, thumbnail_url, source_url, author_handle
            FROM discovery_items
            WHERE platform = ?
              AND (expires_at IS NULL OR expires_at > ?)
            ORDER BY discovered_at DESC
            LIMIT 60
            "#,
            vec![json!(source_platform(&source)), json!(now_iso_string())],
        )
        .await?
    } else {
        db::all::<DiscoveryItemResponse>(
            &db,
            r#"
            SELECT id, title, platform, image_url, thumbnail_url, source_url, author_handle
            FROM discovery_items
            WHERE expires_at IS NULL OR expires_at > ?
            ORDER BY discovered_at DESC
            LIMIT 60
            "#,
            vec![json!(now_iso_string())],
        )
        .await?
    };

    let _ = auth;
    Response::from_json(&DiscoveryFeedResponse { items })
}

pub async fn refresh_discovery(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    discovery_feed(req, ctx).await
}

fn source_platform(source: &str) -> &str {
    source.split('-').next().unwrap_or(source)
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

#[cfg(test)]
mod tests {
    use super::source_platform;

    #[test]
    fn source_platform_uses_prefix() {
        assert_eq!(source_platform("instagram-reels"), "instagram");
        assert_eq!(source_platform("tiktok-trending"), "tiktok");
    }
}
