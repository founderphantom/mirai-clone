use crate::auth_client::verify_session;
use crate::db;
use crate::http::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use worker::{Request, Response, Result as WorkerResult, RouteContext};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateGenerationRequest {
    clone_id: String,
    prompt: Option<String>,
    inspiration_asset_id: Option<String>,
    discovery_item_id: Option<String>,
    quality: Option<String>,
    batch_size: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GenerationJobResponse {
    id: String,
    clone_id: String,
    clone_name: Option<String>,
    status: String,
    prompt: Option<String>,
    updated_at: String,
    output_count: u32,
    preview_media_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct GenerationListResponse {
    jobs: Vec<GenerationJobResponse>,
}

#[derive(Debug, Serialize)]
struct CreateGenerationResponse {
    job: GenerationJobResponse,
}

#[derive(Debug, Deserialize)]
struct CloneExistsRow {
    count: u32,
}

pub async fn list_generations(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };

    let db = ctx.env.d1("DB")?;
    let jobs = db::all::<GenerationJobResponse>(
        &db,
        r#"
        SELECT
          gj.id AS id,
          gj.clone_id AS clone_id,
          cp.display_name AS clone_name,
          gj.status AS status,
          gj.prompt AS prompt,
          gj.updated_at AS updated_at,
          COUNT(go.id) AS output_count,
          MIN(go.media_asset_id) AS preview_media_id
        FROM generation_jobs gj
        LEFT JOIN clone_profiles cp
          ON cp.id = gj.clone_id
         AND cp.user_id = gj.user_id
        LEFT JOIN generation_outputs go
          ON go.job_id = gj.id
         AND go.user_id = gj.user_id
        WHERE gj.user_id = ?
        GROUP BY gj.id, gj.clone_id, cp.display_name, gj.status, gj.prompt, gj.updated_at
        ORDER BY gj.updated_at DESC
        LIMIT 100
        "#,
        vec![json!(auth.user_id)],
    )
    .await?;

    Response::from_json(&GenerationListResponse { jobs })
}

pub async fn create_generation(mut req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let input = match req.json::<CreateGenerationRequest>().await {
        Ok(input) => input,
        Err(_) => {
            return ApiError::bad_request(
                "invalid_generation_request",
                "Expected cloneId and generation options.",
            )
            .to_response()
        }
    };

    let db = ctx.env.d1("DB")?;
    let clone = db::first::<CloneExistsRow>(
        &db,
        r#"
        SELECT COUNT(*) AS count
        FROM clone_profiles
        WHERE id = ?
          AND user_id = ?
          AND deleted_at IS NULL
        "#,
        vec![json!(input.clone_id), json!(auth.user_id)],
    )
    .await?;
    if clone.map(|row| row.count).unwrap_or(0) == 0 {
        return ApiError::not_found("clone_not_found", "Clone profile was not found.")
            .to_response();
    }

    let job_id = format!("gen_{}", Uuid::new_v4().simple());
    let now = now_iso_string();
    let request_json = json!({
        "batchSize": input.batch_size.unwrap_or(4),
        "discoveryItemId": input.discovery_item_id,
        "inspirationAssetId": input.inspiration_asset_id,
    })
    .to_string();
    let prompt = input.prompt.filter(|value| !value.trim().is_empty());
    let quality = input.quality.unwrap_or_else(|| "1080p".to_string());

    db::exec(
        &db,
        r#"
        INSERT INTO generation_jobs (
          id,
          user_id,
          clone_id,
          input_media_asset_id,
          status,
          prompt,
          quality,
          request_json,
          queued_at,
          updated_at
        )
        VALUES (?, ?, ?, ?, 'queued', ?, ?, ?, ?, ?)
        "#,
        vec![
            json!(job_id),
            json!(auth.user_id),
            json!(input.clone_id),
            json!(input.inspiration_asset_id),
            json!(prompt),
            json!(quality),
            json!(request_json),
            json!(now),
            json!(now),
        ],
    )
    .await?;

    Response::from_json(&CreateGenerationResponse {
        job: GenerationJobResponse {
            id: job_id,
            clone_id: input.clone_id,
            clone_name: None,
            status: "queued".to_string(),
            prompt,
            updated_at: now,
            output_count: 0,
            preview_media_id: None,
        },
    })
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}
