use crate::auth_client::verify_session;
use crate::db;
use crate::domain::moodboards::{
    default_moodboards, deterministic_user_moodboard_id, selected_moodboard_count_is_valid,
    selected_moodboard_hash,
};
use crate::http::error::ApiError;
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

#[derive(Debug, Deserialize)]
struct SelectedMoodboardStateRow {
    id: String,
    slug: String,
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
    ensure_default_user_moodboards(&db, user_id).await?;
    let moodboards = load_moodboards(&db, user_id).await?;
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
    let _clone_id = read_optional_json::<GenerateMoodboardsRequest>(req)
        .await?
        .and_then(|input| input.clone_id);
    ensure_default_user_moodboards(&db, &auth.user_id).await?;

    Response::from_json(&MoodboardsResponse {
        moodboards: load_moodboards(&db, &auth.user_id).await?,
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
    ensure_default_user_moodboards(&db, &auth.user_id).await?;

    let SaveMoodboardsRequest {
        moodboard_ids,
        clone_id: _clone_id,
        moderation_level: _moderation_level,
    } = input;
    let requested_moodboard_ids = unique_selected_moodboard_ids(moodboard_ids);
    if !selected_moodboard_count_is_valid(requested_moodboard_ids.len()) {
        return ApiError::bad_request("invalid_moodboard_selection", "Choose 1 to 10 moodboards.")
            .to_response();
    }

    if requested_disabled_moodboard_count(&db, &auth.user_id, &requested_moodboard_ids).await? > 0
    {
        return ApiError::bad_request(
            "disabled_moodboard",
            "One or more selected moodboards are no longer available.",
        )
        .to_response();
    }

    let selected_moodboard_ids =
        load_matching_active_moodboard_ids(&db, &auth.user_id, &requested_moodboard_ids).await?;
    if !all_requested_moodboards_matched(&selected_moodboard_ids, &requested_moodboard_ids) {
        return ApiError::bad_request(
            "invalid_moodboard_selection",
            "Choose only available moodboards.",
        )
        .to_response();
    }

    save_selected_moodboards(&db, &auth.user_id, &selected_moodboard_ids).await?;
    let selected_slugs = rebuild_user_reference_state(&db, &auth.user_id).await?;
    crate::services::reference_pipeline::enqueue_after_moodboard_save(
        &db,
        &ctx.env,
        &auth.user_id,
        &selected_slugs,
    )
    .await?;

    Response::from_json(&MoodboardsResponse {
        moodboards: load_moodboards(&db, &auth.user_id).await?,
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

async fn load_moodboards(
    db: &worker::D1Database,
    user_id: &str,
) -> WorkerResult<Vec<MoodboardResponse>> {
    let rows = db::all::<MoodboardRow>(
        db,
        r#"
        SELECT mb.id,
               mb.slug,
               gmd.title,
               gmd.vibe_summary,
               gmd.search_queries_json,
               CASE WHEN gmd.status = 'active' THEN mb.selected ELSE 0 END AS selected
        FROM moodboards mb
        INNER JOIN global_moodboard_definitions gmd
          ON gmd.slug = mb.slug
        WHERE mb.user_id = ?
          AND gmd.status = 'active'
        ORDER BY gmd.sort_order ASC, mb.created_at ASC
        "#,
        vec![json!(user_id)],
    )
    .await?;

    Ok(rows.into_iter().map(MoodboardResponse::from).collect())
}

async fn ensure_default_user_moodboards(
    db: &worker::D1Database,
    user_id: &str,
) -> WorkerResult<()> {
    sync_global_moodboard_definitions(db).await?;
    let now = now_iso_string();
    let statements = default_moodboards()
        .into_iter()
        .map(|seed| {
            let id = deterministic_user_moodboard_id(user_id, &seed.slug);
            (
                r#"
                INSERT OR IGNORE INTO moodboards (
                  id,
                  user_id,
                  slug,
                  selected,
                  created_at,
                  updated_at
                )
                VALUES (?, ?, ?, 0, ?, ?)
                "#,
                vec![
                    json!(id),
                    json!(user_id),
                    json!(seed.slug),
                    json!(now),
                    json!(now),
                ],
            )
        })
        .collect::<Vec<_>>();

    db::batch(db, statements).await?;
    rebuild_user_reference_state(db, user_id).await?;
    Ok(())
}

async fn sync_global_moodboard_definitions(db: &worker::D1Database) -> WorkerResult<()> {
    let now = now_iso_string();
    let statements = default_moodboards()
        .into_iter()
        .enumerate()
        .map(|(index, seed)| {
            let search_queries_json =
                serde_json::to_string(&seed.search_queries).unwrap_or_else(|_| "[]".to_string());
            (
                r#"
                INSERT INTO global_moodboard_definitions (
                  slug,
                  title,
                  vibe_summary,
                  search_queries_json,
                  sort_order,
                  status,
                  created_at,
                  updated_at
                )
                VALUES (?, ?, ?, ?, ?, 'active', ?, ?)
                ON CONFLICT(slug) DO UPDATE SET
                  title = excluded.title,
                  vibe_summary = excluded.vibe_summary,
                  search_queries_json = excluded.search_queries_json,
                  sort_order = excluded.sort_order,
                  updated_at = excluded.updated_at
                WHERE global_moodboard_definitions.status = 'active'
                "#,
                vec![
                    json!(seed.slug),
                    json!(seed.title),
                    json!(seed.vibe_summary),
                    json!(search_queries_json),
                    json!(index),
                    json!(now),
                    json!(now),
                ],
            )
        })
        .collect::<Vec<_>>();

    db::batch(db, statements).await?;
    Ok(())
}

async fn selected_active_moodboard_state(
    db: &worker::D1Database,
    user_id: &str,
) -> WorkerResult<Vec<SelectedMoodboardStateRow>> {
    db::all(
        db,
        r#"
        SELECT mb.id, mb.slug
        FROM moodboards mb
        INNER JOIN global_moodboard_definitions gmd
          ON gmd.slug = mb.slug
        WHERE mb.user_id = ?
          AND mb.selected = 1
          AND gmd.status = 'active'
        ORDER BY mb.slug ASC
        "#,
        vec![json!(user_id)],
    )
    .await
}

async fn rebuild_user_reference_state(
    db: &worker::D1Database,
    user_id: &str,
) -> WorkerResult<Vec<String>> {
    let rows = selected_active_moodboard_state(db, user_id).await?;
    let ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let slugs = rows.iter().map(|row| row.slug.clone()).collect::<Vec<_>>();
    let selected_hash = selected_moodboard_hash(&slugs);
    let now = now_iso_string();

    db::exec(
        db,
        r#"
        INSERT INTO user_reference_state (
          user_id,
          selected_moodboard_ids_json,
          selected_moodboard_slugs_json,
          selected_moodboard_hash,
          created_at,
          updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
          selected_moodboard_ids_json = excluded.selected_moodboard_ids_json,
          selected_moodboard_slugs_json = excluded.selected_moodboard_slugs_json,
          selected_moodboard_hash = excluded.selected_moodboard_hash,
          updated_at = excluded.updated_at
        "#,
        vec![
            json!(user_id),
            json!(serde_json::to_string(&ids)?),
            json!(serde_json::to_string(&slugs)?),
            json!(selected_hash),
            json!(now),
            json!(now),
        ],
    )
    .await?;

    Ok(slugs)
}

async fn save_selected_moodboards(
    db: &worker::D1Database,
    user_id: &str,
    selected_moodboard_ids: &[String],
) -> WorkerResult<()> {
    let selected_json = serde_json::to_string(selected_moodboard_ids)?;
    let now = now_iso_string();
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
          THEN 1 ELSE 0 END,
          updated_at = ?
        WHERE user_id = ?
        "#,
        vec![json!(selected_json), json!(now), json!(user_id)],
    )
    .await
}

async fn load_matching_active_moodboard_ids(
    db: &worker::D1Database,
    user_id: &str,
    selected_moodboard_ids: &[String],
) -> WorkerResult<Vec<String>> {
    let selected_json = serde_json::to_string(selected_moodboard_ids)?;
    let rows = db::all::<IdRow>(
        db,
        r#"
        SELECT mb.id
        FROM moodboards mb
        INNER JOIN global_moodboard_definitions gmd
          ON gmd.slug = mb.slug
        WHERE mb.user_id = ?
          AND gmd.status = 'active'
          AND EXISTS (
            SELECT 1
            FROM json_each(?)
            WHERE json_each.value = mb.id
          )
        ORDER BY gmd.sort_order ASC, mb.created_at ASC
        "#,
        vec![json!(user_id), json!(selected_json)],
    )
    .await?;

    Ok(rows.into_iter().map(|row| row.id).collect())
}

async fn requested_disabled_moodboard_count(
    db: &worker::D1Database,
    user_id: &str,
    selected_moodboard_ids: &[String],
) -> WorkerResult<u32> {
    let selected_json = serde_json::to_string(selected_moodboard_ids)?;
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM moodboards mb
        INNER JOIN global_moodboard_definitions gmd
          ON gmd.slug = mb.slug
        WHERE mb.user_id = ?
          AND gmd.status <> 'active'
          AND EXISTS (
            SELECT 1
            FROM json_each(?)
            WHERE json_each.value = mb.id
          )
        "#,
        vec![json!(user_id), json!(selected_json)],
    )
    .await?;

    Ok(row.map(|row| row.count).unwrap_or(0))
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
