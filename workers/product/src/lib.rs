mod auth_client;
mod db;
pub mod domain;
mod env;
mod http;
mod providers;
mod queues;
mod routes;
pub mod services;

use queues::CloneTrainingMessage;
use worker::{event, Context, Env, MessageBatch, Request, Response, Result as WorkerResult};

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
pub async fn queue(
    batch: MessageBatch<CloneTrainingMessage>,
    env: Env,
    _ctx: Context,
) -> WorkerResult<()> {
    queues::clone_training::handle_batch(batch, env).await
}
