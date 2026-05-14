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
pub struct MoodboardSeed {
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

pub fn default_moodboards() -> Vec<MoodboardSeed> {
    vec![
        moodboard_seed("warm-ambient", "Warm ambient", "Soft tungsten warmth, calm rooms, skin glow, and relaxed editorial framing."),
        moodboard_seed("y2k-studio", "Y2K studio", "Glossy flash studio portraits, chrome accents, playful styling, and polished social poses."),
        moodboard_seed("swag-era", "Swag era", "Bold accessories, confident casual poses, bright flash, and early social-era outfit energy."),
        moodboard_seed("theatrical-light", "Theatrical light", "Dramatic spotlights, sculpted shadows, stage color, and cinematic portrait contrast."),
        moodboard_seed("y2k-street", "Y2K street", "Street snapshots, low-rise layers, compact cameras, and saturated city color."),
        moodboard_seed("flash-editorial", "Flash editorial", "Direct flash, crisp styling, strong makeup, studio walls, and magazine energy."),
        moodboard_seed("old-smartphone", "Old smartphone", "Soft phone-camera grain, imperfect framing, casual mirror shots, and nostalgic texture."),
        moodboard_seed("street-photography", "Street photography", "Candid sidewalks, real city motion, natural outfits, and documentary framing."),
        moodboard_seed("asian-nostalgia", "Asian nostalgia", "Warm city evenings, intimate cafes, retro interiors, and soft nostalgic styling."),
        moodboard_seed("retro-bw", "Retro BW", "High-grain black and white portraits, strong contrast, and vintage editorial attitude."),
        moodboard_seed("subtle-flash", "Subtle flash", "Low-key direct flash, soft shadows, realistic skin, and understated nightlife polish."),
        moodboard_seed("surreal-solarization", "Surreal solarization", "Experimental color inversions, glowing edges, and dreamlike fashion portrait effects."),
        moodboard_seed("digital-camera", "Digital camera", "Compact-camera sharpness, glossy highlights, dated timestamps, and candid creator snaps."),
        moodboard_seed("siren", "Siren", "Sleek glam, moody nightlife, sharp silhouettes, and magnetic editorial confidence."),
        moodboard_seed("mystique-city", "Mystique city", "Dark urban atmosphere, reflective streets, elegant styling, and secretive cinematic light."),
        moodboard_seed("candy-pop", "Candy pop", "Bright color blocking, playful beauty details, glossy styling, and upbeat studio energy."),
        moodboard_seed("double-exposure", "Double exposure", "Layered portraits, ghosted motion, city overlays, and experimental photographic texture."),
        moodboard_seed("2000s-band", "2000s band", "Indie band flash, backstage styling, instrument-room texture, and casual group-photo attitude."),
        moodboard_seed("frutiger-aero", "Frutiger aero", "Glossy blue-green futurism, water reflections, glassy surfaces, and optimistic digital polish."),
        moodboard_seed("drain", "Drain", "Washed-out cool tones, underground styling, stark flash, and melancholic street energy."),
        moodboard_seed("extraterrestrial", "Extraterrestrial", "Alien color casts, metallic styling, unusual poses, and otherworldly editorial light."),
        moodboard_seed("nature-light", "Nature light", "Clean daylight, greenery, soft skin tones, and organic outdoor portrait calm."),
        moodboard_seed("editorial-street-style", "Editorial street style", "Runway-informed street outfits, confident full-body framing, and crisp city polish."),
        moodboard_seed("new-indie", "New Indie", "Modern indie styling, casual interiors, soft flash, and intimate music-scene energy."),
        moodboard_seed("underwater", "Underwater", "Blue cast light, floating fabric, softened movement, and submerged dreamlike portraits."),
        moodboard_seed("80s-horror", "80s horror", "Hard colored light, suspenseful shadows, retro styling, and cinematic genre tension."),
        moodboard_seed("disposable-camera", "Disposable camera", "Warm film grain, party flash, imperfect framing, and spontaneous memory-card texture."),
        moodboard_seed("neutral-pastel-film", "Neutral pastel film", "Soft muted pastels, low contrast, delicate grain, and gentle daylight portraits."),
        moodboard_seed("warm-vivid-film", "Warm vivid film", "Saturated warm film color, sunny skin tones, and energetic analog contrast."),
        moodboard_seed("bw-film", "BW film", "Classic black and white film grain, silver highlights, and timeless portrait contrast."),
        moodboard_seed("warm-contrast-film", "Warm contrast film", "Golden highlights, deep shadows, rich analog color, and confident editorial warmth."),
        moodboard_seed("muted-cool-film", "Muted cool film", "Cool gray-green film tones, restrained contrast, and quiet cinematic mood."),
    ]
}

fn moodboard_seed(slug: &str, title: &str, vibe_summary: &str) -> MoodboardSeed {
    let search_base = title.to_ascii_lowercase();
    MoodboardSeed {
        slug: slug.to_string(),
        title: title.to_string(),
        vibe_summary: vibe_summary.to_string(),
        search_queries: vec![
            format!("{search_base} creator aesthetic"),
            format!("{search_base} fashion portrait"),
            format!("{search_base} social photo style"),
        ],
    }
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
    if !valid_selected_moodboard_count(requested_moodboard_ids.len()) {
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
            let id = deterministic_moodboard_id(user_id, clone_id, &seed.slug);
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

fn valid_selected_moodboard_count(count: usize) -> bool {
    (1..=10).contains(&count)
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

fn deterministic_moodboard_id(user_id: &str, clone_id: Option<&str>, slug: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(user_id.as_bytes());
    hasher.update(b":");
    hasher.update(clone_id.unwrap_or("user"));
    hasher.update(b":");
    hasher.update(slug.as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("moodboard_{}", &hash[..24])
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

#[cfg(test)]
mod tests {
    use super::{
        all_requested_moodboards_matched, unique_selected_moodboard_ids,
        valid_selected_moodboard_count, CloneSummary, MoodboardResponse, SaveMoodboardsRequest,
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
        assert!(!valid_selected_moodboard_count(0));
        assert!(valid_selected_moodboard_count(1));
        assert!(valid_selected_moodboard_count(5));
        assert!(valid_selected_moodboard_count(10));
        assert!(!valid_selected_moodboard_count(11));
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
        assert!(valid_selected_moodboard_count(selected.len()));
        assert!(valid_selected_moodboard_count(selected.len() - 1));
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
