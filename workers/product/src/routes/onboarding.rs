use crate::ai::model_router::clamp_moderation_level;
use crate::auth_client::verify_session;
use crate::db;
use crate::http::error::ApiError;
use crate::queues::niche_research::NicheResearchMessage;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use worker::{Request, Response, Result as WorkerResult, RouteContext};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BubbleSeed {
    pub slug: String,
    pub title: String,
    pub vibe_summary: String,
    pub search_queries: Vec<String>,
}

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
struct BubbleRow {
    id: String,
    slug: String,
    title: String,
    vibe_summary: String,
    search_queries_json: String,
    selected: u8,
}

#[derive(Debug, Serialize)]
struct BubbleResponse {
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
    bubbles: Vec<BubbleResponse>,
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
struct BubblesResponse {
    bubbles: Vec<BubbleResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveBubblesRequest {
    #[serde(alias = "bubbleIds")]
    selected_bubble_ids: Vec<String>,
    clone_id: Option<String>,
    moderation_level: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateBubblesRequest {
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

pub fn default_bubbles() -> Vec<BubbleSeed> {
    vec![
        BubbleSeed {
            slug: "y2k-cafe".to_string(),
            title: "Y2K Cafe".to_string(),
            vibe_summary: "Glossy cafe snapshots, playful accessories, compact cameras, and early-2000s color pops.".to_string(),
            search_queries: vec![
                "y2k cafe outfit flash photography".to_string(),
                "early 2000s cafe aesthetic fashion".to_string(),
                "glossy y2k creator cafe photo".to_string(),
            ],
        },
        BubbleSeed {
            slug: "tokyo-neon".to_string(),
            title: "Tokyo Neon".to_string(),
            vibe_summary: "Night streets, vending-machine glow, reflective jackets, and saturated city color.".to_string(),
            search_queries: vec![
                "tokyo neon street fashion night".to_string(),
                "japan city night creator portrait".to_string(),
                "neon streetwear rain reflections".to_string(),
            ],
        },
        BubbleSeed {
            slug: "streetwear-fit".to_string(),
            title: "Streetwear Fit".to_string(),
            vibe_summary: "Layered fits, sneaker details, city backdrops, and confident full-body framing.".to_string(),
            search_queries: vec![
                "streetwear fit check city photo".to_string(),
                "sneaker outfit creator street style".to_string(),
                "urban layered fashion full body".to_string(),
            ],
        },
        BubbleSeed {
            slug: "clean-girl".to_string(),
            title: "Clean Girl".to_string(),
            vibe_summary: "Minimal styling, dewy skin, slick hair, calm rooms, and polished everyday routines.".to_string(),
            search_queries: vec![
                "clean girl aesthetic creator portrait".to_string(),
                "minimal dewy skincare lifestyle photo".to_string(),
                "neutral outfit slick hair apartment light".to_string(),
            ],
        },
        BubbleSeed {
            slug: "coastal-weekend".to_string(),
            title: "Coastal Weekend".to_string(),
            vibe_summary: "Linen layers, sea air, beach walks, patio lunches, and relaxed vacation polish.".to_string(),
            search_queries: vec![
                "coastal weekend linen outfit".to_string(),
                "beach walk lifestyle fashion creator".to_string(),
                "summer patio lunch vacation aesthetic".to_string(),
            ],
        },
        BubbleSeed {
            slug: "golden-hour".to_string(),
            title: "Golden Hour".to_string(),
            vibe_summary: "Warm sunlight, soft shadows, glowing skin, and outdoor portraits near sunset.".to_string(),
            search_queries: vec![
                "golden hour creator portrait fashion".to_string(),
                "sunset lifestyle photo glowing skin".to_string(),
                "warm outdoor editorial portrait".to_string(),
            ],
        },
        BubbleSeed {
            slug: "editorial-flash".to_string(),
            title: "Editorial Flash".to_string(),
            vibe_summary: "Direct flash, crisp styling, dramatic makeup, studio walls, and magazine energy.".to_string(),
            search_queries: vec![
                "direct flash editorial fashion portrait".to_string(),
                "magazine style creator studio photo".to_string(),
                "dramatic makeup flash photography".to_string(),
            ],
        },
        BubbleSeed {
            slug: "pilates-morning".to_string(),
            title: "Pilates Morning".to_string(),
            vibe_summary: "Bright activewear, studio mirrors, smoothie runs, and quiet wellness routines.".to_string(),
            search_queries: vec![
                "pilates morning activewear creator".to_string(),
                "wellness routine smoothie lifestyle photo".to_string(),
                "clean gym mirror fitness aesthetic".to_string(),
            ],
        },
    ]
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
    let bubbles = load_bubbles(
        &db,
        user_id,
        active_clone.as_ref().map(|clone| clone.id.as_str()),
    )
    .await?;
    let inspiration_pool_count = count_inspiration_pool(&db, user_id).await?;

    Response::from_json(&OnboardingStateResponse {
        clones,
        active_clone,
        bubbles,
        inspiration_pool_count,
        starters: Vec::new(),
        instagram: InstagramState {
            enabled: false,
            status: "coming_soon".to_string(),
        },
    })
}

pub async fn generate_bubbles(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let db = ctx.env.d1("DB")?;
    let input = read_optional_json::<GenerateBubblesRequest>(req).await?;
    let active_clone = match input.and_then(|input| input.clone_id) {
        Some(clone_id) => load_clone_by_id(&db, &auth.user_id, &clone_id).await?,
        None => load_active_clone(&db, &auth.user_id).await?,
    };
    let clone_id = active_clone.as_ref().map(|clone| clone.id.as_str());

    ensure_default_bubbles(&db, &auth.user_id, clone_id).await?;

    Response::from_json(&BubblesResponse {
        bubbles: load_bubbles(&db, &auth.user_id, clone_id).await?,
    })
}

pub async fn save_bubbles(mut req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = match verify_session(&ctx, req.headers()).await? {
        Some(auth) => auth,
        None => return ApiError::unauthorized().to_response(),
    };
    let input = match req.json::<SaveBubblesRequest>().await {
        Ok(input) => input,
        Err(_) => {
            return ApiError::bad_request(
                "invalid_bubbles_request",
                "Expected selectedBubbleIds and optional moderationLevel.",
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
        return ApiError::bad_request("missing_clone", "Create a clone before saving bubbles.")
            .to_response();
    };

    let requested_bubble_ids = unique_selected_bubble_ids(input.selected_bubble_ids);
    if requested_bubble_ids.is_empty() || requested_bubble_ids.len() > 5 {
        return ApiError::bad_request(
            "invalid_bubble_selection",
            "Choose between 1 and 5 inspiration bubbles.",
        )
        .to_response();
    }

    let selected_bubble_ids =
        load_matching_bubble_ids(&db, &auth.user_id, &active_clone.id, &requested_bubble_ids)
            .await?;
    if !all_requested_bubbles_matched(&selected_bubble_ids, &requested_bubble_ids) {
        return ApiError::bad_request(
            "invalid_bubble_selection",
            "Choose only available inspiration bubbles.",
        )
        .to_response();
    }
    save_selected_bubbles(&db, &auth.user_id, &active_clone.id, &selected_bubble_ids).await?;

    let moderation_level = clamp_moderation_level(input.moderation_level.unwrap_or(4));
    ctx.env
        .queue("NICHE_RESEARCH_QUEUE")?
        .send(NicheResearchMessage::SeedFromBubbles {
            user_id: auth.user_id.clone(),
            clone_id: active_clone.id.clone(),
            bubble_ids: selected_bubble_ids,
            moderation_level,
        })
        .await?;

    Response::from_json(&BubblesResponse {
        bubbles: load_bubbles(&db, &auth.user_id, Some(&active_clone.id)).await?,
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

async fn load_bubbles(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: Option<&str>,
) -> WorkerResult<Vec<BubbleResponse>> {
    let rows = db::all::<BubbleRow>(
        db,
        r#"
        SELECT id, slug, title, vibe_summary, search_queries_json, selected
        FROM inspiration_bubbles
        WHERE user_id = ?
          AND ((clone_id = ?) OR (clone_id IS NULL AND ? IS NULL))
        ORDER BY sort_order ASC, created_at ASC
        "#,
        vec![json!(user_id), json!(clone_id), json!(clone_id)],
    )
    .await?;

    Ok(rows.into_iter().map(BubbleResponse::from).collect())
}

async fn ensure_default_bubbles(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: Option<&str>,
) -> WorkerResult<()> {
    let existing = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM inspiration_bubbles
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
    let statements = default_bubbles()
        .into_iter()
        .enumerate()
        .map(|(index, seed)| {
            let id = deterministic_bubble_id(user_id, clone_id, &seed.slug);
            let search_queries_json =
                serde_json::to_string(&seed.search_queries).unwrap_or_else(|_| "[]".to_string());
            (
                r#"
                INSERT OR IGNORE INTO inspiration_bubbles (
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

async fn save_selected_bubbles(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: &str,
    selected_bubble_ids: &[String],
) -> WorkerResult<()> {
    let selected_json = serde_json::to_string(selected_bubble_ids)?;
    db::exec(
        db,
        r#"
        UPDATE inspiration_bubbles
        SET selected = CASE
          WHEN EXISTS (
            SELECT 1
            FROM json_each(?)
            WHERE json_each.value = inspiration_bubbles.id
          )
          THEN 1 ELSE 0 END
        WHERE user_id = ?
          AND clone_id = ?
        "#,
        vec![json!(selected_json), json!(user_id), json!(clone_id)],
    )
    .await
}

async fn load_matching_bubble_ids(
    db: &worker::D1Database,
    user_id: &str,
    clone_id: &str,
    selected_bubble_ids: &[String],
) -> WorkerResult<Vec<String>> {
    let selected_json = serde_json::to_string(selected_bubble_ids)?;
    let rows = db::all::<IdRow>(
        db,
        r#"
        SELECT id
        FROM inspiration_bubbles
        WHERE user_id = ?
          AND clone_id = ?
          AND EXISTS (
            SELECT 1
            FROM json_each(?)
            WHERE json_each.value = inspiration_bubbles.id
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

fn unique_selected_bubble_ids(ids: Vec<String>) -> Vec<String> {
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

fn all_requested_bubbles_matched(matched_ids: &[String], requested_ids: &[String]) -> bool {
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

impl From<BubbleRow> for BubbleResponse {
    fn from(row: BubbleRow) -> Self {
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

fn deterministic_bubble_id(user_id: &str, clone_id: Option<&str>, slug: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(user_id.as_bytes());
    hasher.update(b":");
    hasher.update(clone_id.unwrap_or("user"));
    hasher.update(b":");
    hasher.update(slug.as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("bubble_{}", &hash[..24])
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

#[cfg(test)]
mod tests {
    use super::{
        all_requested_bubbles_matched, unique_selected_bubble_ids, BubbleResponse, CloneSummary,
        SaveBubblesRequest,
    };
    use serde_json::json;

    #[test]
    fn save_bubbles_request_accepts_existing_bubble_ids_contract() {
        let request = serde_json::from_value::<SaveBubblesRequest>(json!({
            "cloneId": "clone_1",
            "bubbleIds": ["bubble_1", "bubble_2"],
            "moderationLevel": 7
        }))
        .unwrap();

        assert_eq!(request.clone_id.as_deref(), Some("clone_1"));
        assert_eq!(request.selected_bubble_ids, vec!["bubble_1", "bubble_2"]);
        assert_eq!(request.moderation_level, Some(7));
    }

    #[test]
    fn selected_bubble_ids_are_deduped_and_trimmed() {
        assert_eq!(
            unique_selected_bubble_ids(vec![
                " bubble_1 ".to_string(),
                "bubble_1".to_string(),
                "".to_string(),
                "bubble_2".to_string(),
            ]),
            vec!["bubble_1".to_string(), "bubble_2".to_string()]
        );
    }

    #[test]
    fn all_requested_bubbles_must_match_available_bubbles() {
        assert!(all_requested_bubbles_matched(
            &["bubble_1".to_string(), "bubble_2".to_string()],
            &["bubble_2".to_string(), "bubble_1".to_string()]
        ));
        assert!(!all_requested_bubbles_matched(
            &["bubble_1".to_string()],
            &["bubble_1".to_string(), "foreign".to_string()]
        ));
        assert!(!all_requested_bubbles_matched(
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
        let bubble = serde_json::to_value(BubbleResponse {
            id: "bubble_1".to_string(),
            slug: "y2k-cafe".to_string(),
            title: "Y2K Cafe".to_string(),
            vibe_summary: "Cafe flash.".to_string(),
            search_queries: vec!["y2k cafe".to_string()],
            selected: true,
        })
        .unwrap();

        assert_eq!(clone["name"], json!("My Soul"));
        assert_eq!(clone["display_name"], json!("My Soul"));
        assert_eq!(clone["soul_status"], json!("queued"));
        assert_eq!(bubble["vibe_summary"], json!("Cafe flash."));
        assert_eq!(bubble["searchQueries"], json!(["y2k cafe"]));
    }
}
