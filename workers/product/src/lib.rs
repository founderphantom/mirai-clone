pub mod db;
pub mod domain;
pub mod env;
pub mod http;

use worker::{event, Context, Env, Request, Response, Result as WorkerResult};

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
