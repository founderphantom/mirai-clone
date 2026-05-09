use serde::Serialize;
use worker::{Env, Request, Response, Result as WorkerResult, RouteContext, Router};

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
    app: String,
    bindings: HealthBindings,
}

#[derive(Serialize)]
struct HealthBindings {
    d1: bool,
    r2: bool,
    clone_training_queue: bool,
    generation_queue: bool,
    niche_research_queue: bool,
    ai: bool,
    auth_service: bool,
    assets: bool,
}

pub async fn run(req: Request, env: Env) -> WorkerResult<Response> {
    Router::new()
        .get_async("/api/account", crate::routes::account::get_account)
        .get_async("/api/account/usage", crate::routes::account::get_usage)
        .get_async("/api/clones", crate::routes::clones::list_clones)
        .get_async(
            "/api/generations",
            crate::routes::generations::list_generations,
        )
        .post_async(
            "/api/generations",
            crate::routes::generations::create_generation,
        )
        .get_async(
            "/api/discovery/feed",
            crate::routes::discovery::discovery_feed,
        )
        .post_async(
            "/api/discovery/refresh",
            crate::routes::discovery::refresh_discovery,
        )
        .get_async(
            "/api/onboarding/state",
            crate::routes::onboarding::onboarding_state,
        )
        .post_async(
            "/api/onboarding/bubbles/generate",
            crate::routes::onboarding::generate_bubbles,
        )
        .post_async(
            "/api/onboarding/bubbles",
            crate::routes::onboarding::save_bubbles,
        )
        .post_async(
            "/api/clones/manual-upload",
            crate::routes::clones::manual_upload,
        )
        .post_async("/api/media/upload", crate::routes::media::upload_media)
        .get_async("/api/media/:id", crate::routes::media::get_media)
        .get_async("/api/health", |_req, ctx| async move { health(ctx).await })
        .run(req, env)
        .await
}

async fn health(ctx: RouteContext<()>) -> WorkerResult<Response> {
    let app = ctx
        .var("APP_NAME")
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "Mirai".to_string());

    Response::from_json(&HealthResponse {
        ok: true,
        app,
        bindings: HealthBindings {
            d1: ctx.env.d1("DB").is_ok(),
            r2: ctx.env.bucket("MEDIA").is_ok(),
            clone_training_queue: ctx.env.queue("CLONE_TRAINING_QUEUE").is_ok(),
            generation_queue: ctx.env.queue("GENERATION_QUEUE").is_ok(),
            niche_research_queue: ctx.env.queue("NICHE_RESEARCH_QUEUE").is_ok(),
            ai: ctx.env.ai("AI").is_ok(),
            auth_service: ctx.env.service("AUTH_SERVICE").is_ok(),
            assets: ctx.env.assets("ASSETS").is_ok(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn health_bindings_serializes_assets_status() {
        let body = serde_json::to_value(HealthResponse {
            ok: true,
            app: "Mirai".to_string(),
            bindings: HealthBindings {
                d1: true,
                r2: true,
                clone_training_queue: true,
                generation_queue: true,
                niche_research_queue: true,
                ai: true,
                auth_service: true,
                assets: true,
            },
        })
        .expect("health response should serialize");

        assert_eq!(body["bindings"]["assets"], json!(true));
    }
}
