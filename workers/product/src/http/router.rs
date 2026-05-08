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
}

pub async fn run(req: Request, env: Env) -> WorkerResult<Response> {
    Router::new()
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
        },
    })
}
