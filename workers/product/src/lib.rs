pub mod ai;
mod auth_client;
mod db;
pub mod domain;
mod env;
mod http;
mod providers;
pub mod queues;
pub mod routes;
pub mod services;

pub use providers::instagram_references;
pub use providers::scrapecreators;
pub use providers::seedream;

use serde_json::Value;
use worker::{
    console_error, event, Context, Env, MessageBatch, Request, Response, Result as WorkerResult,
    ScheduleContext, ScheduledEvent,
};

#[event(fetch, respond_with_errors)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> WorkerResult<Response> {
    let path = req.path();
    if path.starts_with("/api/auth/") || path == "/polar/webhooks" {
        return env.service("AUTH_SERVICE")?.fetch_request(req).await;
    }

    if path.starts_with("/api/") {
        return http::router::run(req, env).await;
    }

    env.assets("ASSETS")?.fetch_request(req).await
}

#[event(queue)]
pub async fn queue(batch: MessageBatch<Value>, env: Env, _ctx: Context) -> WorkerResult<()> {
    queues::handle_batch(batch, env).await
}

#[event(scheduled)]
pub async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    if let Err(error) = services::blitz::reconcile_stale_batches(&env).await {
        console_error!("scheduled blitz reconciliation failed: {}", error);
    }
}
