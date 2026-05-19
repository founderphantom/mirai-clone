use crate::auth_client::verify_session;
use crate::db;
use crate::domain::moodboards::{
    default_moodboards, deterministic_user_moodboard_id, selected_moodboard_count_is_valid,
};
use crate::http::error::ApiError;
use crate::queues::niche_research::NicheResearchMessage;
use serde::{Deserialize, Serialize};
use serde_json::json;
use worker::{Request, Response, Result as WorkerResult, RouteContext};

pub use crate::domain::moodboards::MoodboardSeed;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CloneSummary {
    id: String,
    name: String,
    display_name: String,
    handle: String,
    source: String,
    status: String,
    soul_status: String,
    reference_count_total: u32,
}

#[derive(Debug, Deserialize)]
struct MoodboardRow {
    id: String,
    slug: String,
    title: String,
    vibe_summary: String,
    search_queries_json: String,
    selected: u8,
}

#[derive(Debug, Serialize)]
struct MoodboardResponse {
    id: String,
    slug: String,
    title: String,
    vibe_summary: String,
    #[serde(rename = "searchQueries")]
    search_queries: Vec<String>,
    selected: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OnboardingStateResponse {
    clones: Vec<CloneSummary>,
    active_clone: Option<CloneSummary>,
    moodboards: Vec<MoodboardResponse>,
    inspiration_pool_count: u32,
    starters: Vec<serde_json::Value>,
    instagram: InstagramState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstagramState {
    enabled: bool,
    status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MoodboardsResponse {
    moodboards: Vec<MoodboardResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveMoodboardsRequest {
    moodboard_ids: Vec<String>,
    clone_id: Option<String>,
    moderation_level: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateMoodboardsRequest {
    clone_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u32,
}

#[derive(Debug, Deserialize)]
struct IdRow {
    id: String,
}

pub async fn onboarding_state(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let db = ctx.env.d1("DB")?;
    let user_id = auth.user_id.as_str();
    let clones = load_clones(&db, user_id).await?;
    let active_clone = clones.first().cloned();
    let moodboards = load_moodboards(
        &db,
        user_id,
        active_clone.as_ref().map(|clone| clone.id.as_str()),
    )
    .await?;
    let inspiration_pool_count = count_inspiration_pool(&db, user_id).await?;

    Response::from_json(&OnboardingStateResponse {
        clones,
        active_clone,
        moodboards,
        inspiration_pool_count,
        starters: Vec::new(),
        instagram: InstagramState {
            enabled: false,
            status: "coming_soon".to_string(),
        },
    })
}

pub async fn instagram_harvest_status(
    req: Request,
    ctx: RouteContext<()>,
) -> WorkerResult<Response> {
    match verify_session(&ctx, req.headers()).await? {
        Some(_) => ApiError::not_found(
            "instagram_onboarding_unavailable",
            "Instagram onboarding is not available in this backend phase.",
        )
        .to_response(),
        None => ApiError::unauthorized().to_response(),
    }
}

pub async fn adopt_starter(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    match verify_session(&ctx, req.headers()).await? {
        Some(_) => ApiError::not_found(
            "starter_onboarding_unavailable",
            "Starter Souls are not available in this backend phase.",
        )
        .to_response(),
        None => ApiError::unauthorized().to_response(),
    }
}

pub async fn generate_moodboards(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let db = ctx.env.d1("DB")?;
    let input = read_optional_json::<GenerateMoodboardsRequest>(req).await?;
    let active_clone = match input.and_then(|input| input.clone_id) {
        Some(clone_id) => load_clone_by_id(&db, &auth.user_id, &clone_id).await?,
        None => load_active_clone(&db, &auth.user_id).await?,
    };
    let clone_id = active_clone.as_ref().map(|clone| clone.id.as_str());

    ensure_default_moodboards(&db, &auth.user_id, clone_id).await?;

    Response::from_json(&MoodboardsResponse {
        moodboards: load_moodboards(&db, &auth.user_id, clone_id).await?,
    })
}

pub async fn save_moodboards(mut req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let input = match req.json::<SaveMoodboardsRequest>().await {
        Ok(input) => input,
        Err(_) => {
            return ApiError::bad_request(
                "invalid_moodboards_request",
                "Expected moodboardIds and optional moderationLevel.",
            )
            .to_response()
        }
    };

    let db = ctx.env.d1("DB")?;
    let active_clone = match input.clone_id {
        Some(clone_id) => load_clone_by_id(&db, &auth.user_id, &clone_id).await?,
        None => load_active_clone(&db, &auth.user_id).await?,
    };
    let Some(active_clone) = active_clone else {
        return ApiError::bad_request("missing_clone", "Create a clone before saving moodboards.")
            .to_response();
    };

    let requested_moodboard_ids = unique_selected_moodboard_ids(input.moodboard_ids);
    if !selected_moodboard_count_is_valid(requested_moodboard_ids.len()) {
        return ApiError::bad_request("invalid_moodboard_selection", "Choose 1 to 10 moodboards.")
            .to_response();
    }

    let selected_moodboard_ids = load_matching_moodboard_ids(
        &db,
        &auth.user_id,
        &active_clone.id,
        &requested_moodboard_ids,
    )
    .await?;
    if !all_requested_moodboards_matched(&selected_moodboard_ids, &requested_moodboard_ids) {
        return ApiError::bad_request(
            "invalid_moodboard_selection",
            "Choose only available moodboards.",
        )
        .to_response();
    }
    save_selected_moodboards(
        &db,
        &auth.user_id,
        &active_clone.id,
        &selected_moodboard_ids,
    )
    .await?;

    ctx.env
        .queue("NICHE_RESEARCH_QUEUE")?
        .send(NicheResearchMessage::ResearchMoodboardReferences {
            user_id: auth.user_id.clone(),
            clone_id: active_clone.id.clone(),
            moodboard_ids: selected_moodboard_ids,
            reason: "onboarding_selection".to_string(),
        })
        .await?;

    Response::from_json(&MoodboardsResponse {
        moodboards: load_moodboards(&db, &auth.user_id, Some(&active_clone.id)).await?,
    })
}

async fn load_clones(db: &worker::D1Database, user_id: &str) -> WorkerResult<Vec<CloneSummary>> {
    db::all(
        db,
        r#"
        SELECT
          id,
          display_name AS name,
          display_name,
          handle,
          source,
          status,
          soul_status,
          reference_count_total
        FROM clone_profiles
        WHERE user_id = ?
          AND deleted_at IS NULL
        ORDER BY
          CASE WHEN status = 'active' THEN 0 ELSE 1 END,
          CASE WHEN soul_status IN ('ready', 'completed') THEN 0 ELSE 1 END,
          updated_at DESC
        "#,
        vec![json!(user_id)],
    )
    .await
}

async fn load_active_clone(
    db: &worker::D1Database,
    user_id: &str,
) -> WorkerResult<Option<CloneSummary>> {
    Ok(load_clones(db, user_id).await?.into_iter().next())
}

async fn load_clone_by_id(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: &str,
) -> WorkerResult<Option<CloneSummary>> {
    db::first(
        db,
        r#"
        SELECT
          id,
          display_name AS name,
          display_name,
          handle,
          source,
          status,
          soul_status,
          reference_count_total
        FROM clone_profiles
        WHERE user_id = ?
          AND id = ?
          AND deleted_at IS NULL
        LIMIT 1
        "#,
        vec![json!(user_id), json!(clone_id)],
    )
    .await
}

async fn load_moodboards(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: Option<&str>,
) -> WorkerResult<Vec<MoodboardResponse>> {
    let rows = db::all::<MoodboardRow>(
        db,
        r#"
        SELECT id, slug, title, vibe_summary, search_queries_json, selected
        FROM moodboards
        WHERE user_id = ?
          AND ((clone_id = ?) OR (clone_id IS NULL AND ? IS NULL))
        ORDER BY sort_order ASC, created_at ASC
        "#,
        vec![json!(user_id), json!(clone_id), json!(clone_id)],
    )
    .await?;

    Ok(rows.into_iter().map(MoodboardResponse::from).collect())
}

async fn ensure_default_moodboards(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: Option<&str>,
) -> WorkerResult<()> {
    let existing = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM moodboards
        WHERE user_id = ?
          AND ((clone_id = ?) OR (clone_id IS NULL AND ? IS NULL))
        "#,
        vec![json!(user_id), json!(clone_id), json!(clone_id)],
    )
    .await?;
    if existing.map(|row| row.count).unwrap_or(0) > 0 {
        return Ok(());
    }

    let now = now_iso_string();
    let statements = default_moodboards()
        .into_iter()
        .enumerate()
        .map(|(index, seed)| {
            let id = deterministic_user_moodboard_id(user_id, &seed.slug);
            let search_queries_json =
                serde_json::to_string(&seed.search_queries).unwrap_or_else(|_| "[]".to_string());
            (
                r#"
                INSERT OR IGNORE INTO moodboards (
                  id,
                  user_id,
                  clone_id,
                  slug,
                  title,
                  vibe_summary,
                  search_queries_json,
                  selected,
                  weight,
                  sort_order,
                  source,
                  created_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, 0, 1, ?, 'default', ?)
                "#,
                vec![
                    json!(id),
                    json!(user_id),
                    json!(clone_id),
                    json!(seed.slug),
                    json!(seed.title),
                    json!(seed.vibe_summary),
                    json!(search_queries_json),
                    json!(index),
                    json!(now),
                ],
            )
        })
        .collect::<Vec<_>>();

    db::batch(db, statements).await?;
    Ok(())
}

async fn save_selected_moodboards(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: &str,
    selected_moodboard_ids: &[String],
) -> WorkerResult<()> {
    let selected_json = serde_json::to_string(selected_moodboard_ids)?;
    db::exec(
        db,
        r#"
        UPDATE moodboards
        SET selected = CASE
          WHEN EXISTS (
            SELECT 1
            FROM json_each(?)
            WHERE json_each.value = moodboards.id
          )
          THEN 1 ELSE 0 END
        WHERE user_id = ?
          AND clone_id = ?
        "#,
        vec![json!(selected_json), json!(user_id), json!(clone_id)],
    )
    .await
}

async fn load_matching_moodboard_ids(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: &str,
    selected_moodboard_ids: &[String],
) -> WorkerResult<Vec<String>> {
    let selected_json = serde_json::to_string(selected_moodboard_ids)?;
    let rows = db::all::<IdRow>(
        db,
        r#"
        SELECT id
        FROM moodboards
        WHERE user_id = ?
          AND clone_id = ?
          AND EXISTS (
            SELECT 1
            FROM json_each(?)
            WHERE json_each.value = moodboards.id
          )
        ORDER BY sort_order ASC, created_at ASC
        "#,
        vec![json!(user_id), json!(clone_id), json!(selected_json)],
    )
    .await?;

    Ok(rows.into_iter().map(|row| row.id).collect())
}

async fn read_optional_json<T: for<'de> Deserialize<'de>>(
    mut req: Request,
) -> WorkerResult<Option<T>> {
    Ok(req.json::<T>().await.ok())
}

fn unique_selected_moodboard_ids(ids: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for id in ids {
        let id = id.trim();
        if id.is_empty() || unique.iter().any(|existing| existing == id) {
            continue;
        }
        unique.push(id.to_string());
    }
    unique
}

fn all_requested_moodboards_matched(matched_ids: &[String], requested_ids: &[String]) -> bool {
    matched_ids.len() == requested_ids.len()
}

async fn count_inspiration_pool(db: &worker::D1Database, user_id: &str) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM user_inspiration_pool
        WHERE user_id = ?
          AND used_at IS NULL
        "#,
        vec![json!(user_id)],
    )
    .await?;

    Ok(row.map(|row| row.count).unwrap_or(0))
}

impl From<MoodboardRow> for MoodboardResponse {
    fn from(row: MoodboardRow) -> Self {
        let search_queries = serde_json::from_str::<Vec<String>>(&row.search_queries_json)
            .unwrap_or_else(|_| Vec::new());

        Self {
            id: row.id,
            slug: row.slug,
            title: row.title,
            vibe_summary: row.vibe_summary,
            search_queries,
            selected: row.selected != 0,
        }
    }
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

#[cfg(test)]
mod tests {
    use super::{
        all_requested_moodboards_matched, selected_moodboard_count_is_valid,
        unique_selected_moodboard_ids, CloneSummary, MoodboardResponse, SaveMoodboardsRequest,
    };
    use serde_json::json;

    #[test]
    fn save_moodboards_request_accepts_moodboard_ids_contract() {
        let request = serde_json::from_value::<SaveMoodboardsRequest>(json!({
            "cloneId": "clone_1",
            "moodboardIds": ["moodboard_1"],
            "moderationLevel": 7
        }))
        .unwrap();

        assert_eq!(request.clone_id.as_deref(), Some("clone_1"));
        assert_eq!(request.moodboard_ids, vec!["moodboard_1"]);
        assert_eq!(request.moderation_level, Some(7));
    }

    #[test]
    fn selected_moodboard_ids_are_deduped_and_trimmed() {
        assert_eq!(
            unique_selected_moodboard_ids(vec![
                " moodboard_1 ".to_string(),
                "moodboard_1".to_string(),
                "".to_string(),
                "moodboard_2".to_string(),
            ]),
            vec!["moodboard_1".to_string(), "moodboard_2".to_string()]
        );
    }

    #[test]
    fn selected_moodboard_count_accepts_one_to_ten_for_research() {
        assert!(!selected_moodboard_count_is_valid(0));
        assert!(selected_moodboard_count_is_valid(1));
        assert!(selected_moodboard_count_is_valid(5));
        assert!(selected_moodboard_count_is_valid(10));
        assert!(!selected_moodboard_count_is_valid(11));
    }

    #[test]
    fn moodboard_selection_dedupes_before_counting_for_research() {
        let selected = unique_selected_moodboard_ids(vec![
            "moodboard_1".to_string(),
            "moodboard_2".to_string(),
            "moodboard_2".to_string(),
            "moodboard_3".to_string(),
            "moodboard_4".to_string(),
            "moodboard_5".to_string(),
        ]);

        assert_eq!(selected.len(), 5);
        assert!(selected_moodboard_count_is_valid(selected.len()));
        assert!(selected_moodboard_count_is_valid(selected.len() - 1));
    }

    #[test]
    fn all_requested_moodboards_must_match_available_moodboards() {
        assert!(all_requested_moodboards_matched(
            &["moodboard_1".to_string(), "moodboard_2".to_string()],
            &["moodboard_2".to_string(), "moodboard_1".to_string()]
        ));
        assert!(!all_requested_moodboards_matched(
            &["moodboard_1".to_string()],
            &["moodboard_1".to_string(), "foreign".to_string()]
        ));
        assert!(!all_requested_moodboards_matched(
            &Vec::<String>::new(),
            &["foreign".to_string()]
        ));
    }

    #[test]
    fn onboarding_responses_keep_current_frontend_field_names() {
        let clone = serde_json::to_value(CloneSummary {
            id: "clone_1".to_string(),
            name: "My Soul".to_string(),
            display_name: "My Soul".to_string(),
            handle: "my-soul".to_string(),
            source: "manual_upload".to_string(),
            status: "active".to_string(),
            soul_status: "queued".to_string(),
            reference_count_total: 5,
        })
        .unwrap();
        let moodboard = serde_json::to_value(MoodboardResponse {
            id: "moodboard_1".to_string(),
            slug: "warm-ambient".to_string(),
            title: "Warm ambient".to_string(),
            vibe_summary: "Warm ambient light.".to_string(),
            search_queries: vec!["warm ambient".to_string()],
            selected: true,
        })
        .unwrap();

        assert_eq!(clone["name"], json!("My Soul"));
        assert_eq!(clone["display_name"], json!("My Soul"));
        assert_eq!(clone["soul_status"], json!("queued"));
        assert_eq!(moodboard["vibe_summary"], json!("Warm ambient light."));
        assert_eq!(moodboard["searchQueries"], json!(["warm ambient"]));
    }
}
