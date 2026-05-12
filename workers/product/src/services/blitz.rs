use crate::db;
use crate::domain::blitz::{
    accumulate_influence, select_visual_references, SwipeMetadata, VisualReferenceForSelection,
};
use crate::queues::messages::GenerationMessage;
use crate::queues::niche_research::NicheResearchMessage;
use crate::services::generation_usage::GenerationUsageSnapshot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;
use worker::{console_error, console_log, D1Database, Env, Error, Result as WorkerResult};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzCurrentResponse {
    pub batch: Option<BlitzBatchResponse>,
    pub status: Option<String>,
    pub progress: Option<BlitzProgressResponse>,
    pub usage: GenerationUsageSnapshot,
    pub next_batch_status: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzBatchResponse {
    pub id: String,
    pub batch_number: u32,
    pub status: String,
    pub images: Vec<BlitzImageResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzImageResponse {
    pub output_id: String,
    pub media_url: String,
    pub visual_reference_id: Option<String>,
    pub swipe_index: u32,
    pub swiped: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzProgressResponse {
    pub phase: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwipeResponse {
    pub swipe_index: u32,
    pub batch_progress: String,
    pub batch_complete: bool,
    pub next_batch_triggered: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzHistoryResponse {
    pub batches: Vec<BlitzHistoryBatch>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlitzHistoryBatch {
    pub id: String,
    pub batch_number: u32,
    pub like_count: u32,
    pub dislike_count: u32,
    pub completed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConfigRow {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct ExistingBatchRow {
    id: String,
    status: String,
    batch_number: u32,
    batch_size: u32,
}

#[derive(Debug, Deserialize)]
struct MaxBatchNumberRow {
    next_batch_number: u32,
}

#[derive(Debug, Deserialize)]
struct SwipeInfluenceRow {
    action: String,
    output_metadata_json: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwipeMetadataSnapshot {
    #[serde(default)]
    aesthetic_tags: Vec<String>,
    niche_cluster: Option<String>,
    #[serde(default)]
    source_platform: String,
    visual_reference_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VisualReferenceRow {
    id: String,
    source_platform: String,
    source_published_at: Option<String>,
    niche_cluster: Option<String>,
    aesthetic_tags_json: String,
    human_presence_score: f64,
    organic_photo_score: f64,
    freshness_visual_score: f64,
    generation_use_count: u32,
    last_liked_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CloneRow {
    provider_config_json: String,
}

#[derive(Debug, Deserialize)]
struct WaitingReadyPoolRow {
    user_id: String,
    clone_id: String,
    provider_soul_id: String,
}

#[derive(Debug, Deserialize)]
struct BatchRow {
    id: String,
    clone_id: String,
    batch_number: u32,
    status: String,
    batch_size: u32,
    provider_soul_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OutputResponseRow {
    output_id: String,
    media_url: String,
    visual_reference_id: Option<String>,
    swipe_index: u32,
    swiped: u32,
}

#[derive(Debug, Deserialize)]
struct OutputSwipeRow {
    visual_reference_id: Option<String>,
    source_platform: Option<String>,
    niche_cluster: Option<String>,
    aesthetic_tags_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SwipeIndexRow {
    swipe_index: u32,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u32,
}

#[derive(Debug, Deserialize)]
struct BatchGenerationCountsRow {
    job_count: u32,
    output_count: u32,
}

#[derive(Debug, Deserialize)]
struct ExistingSwipeRow {
    action: String,
    visual_reference_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LatestBatchStateRow {
    status: String,
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StatusRow {
    status: String,
}

pub fn swipe_action_to_db_value(action: &str) -> Result<&'static str, &'static str> {
    match action {
        "like" => Ok("like"),
        "dislike" => Ok("dislike"),
        _ => Err("invalid_swipe_action"),
    }
}

pub fn next_batch_should_trigger(existing_swipes_in_batch: u32) -> bool {
    existing_swipes_in_batch == 0
}

pub fn trigger_influence_cutoff_batch_number(current_batch_number: u32) -> u32 {
    current_batch_number.saturating_sub(2)
}

pub fn stored_batch_size_for_selected_refs(
    configured_batch_size: u32,
    selected_reference_count: usize,
) -> u32 {
    u32::try_from(selected_reference_count).unwrap_or(configured_batch_size)
}

pub fn first_swipe_prefetch_should_run(swipes_after_attempt: u32) -> bool {
    swipes_after_attempt > 0
}

pub fn batch_complete_for_swipe_count(swipe_count: u32, output_count: u32) -> bool {
    output_count > 0 && swipe_count >= output_count
}

pub fn swipeable_batch_status(status: &str) -> bool {
    matches!(status, "active" | "ready")
}

pub async fn create_next_batch(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    provider_soul_id: &str,
) -> WorkerResult<Option<String>> {
    if load_user_clone(db, user_id, clone_id).await?.is_none() {
        return Err(Error::RustError("clone_not_found".to_string()));
    }

    if let Some(existing) = db::first::<ExistingBatchRow>(
        db,
        r#"
        SELECT id
             , status
             , batch_number
             , batch_size
        FROM blitz_batches
        WHERE user_id = ?
          AND clone_id = ?
          AND status IN ('pending', 'generating', 'ready')
        ORDER BY batch_number ASC
        LIMIT 1
        "#,
        vec![json!(user_id), json!(clone_id)],
    )
    .await?
    {
        ensure_batch_generation_enqueued(db, env, user_id, clone_id, provider_soul_id, &existing)
            .await?;
        return Ok(Some(existing.id));
    }

    let (batch_size, max_reference_generation_uses) = load_blitz_selection_config(db).await?;
    let next_batch_number = db::first::<MaxBatchNumberRow>(
        db,
        r#"
        SELECT COALESCE(MAX(batch_number), 0) + 1 AS next_batch_number
        FROM blitz_batches
        WHERE clone_id = ?
        "#,
        vec![json!(clone_id)],
    )
    .await?
    .map(|row| row.next_batch_number)
    .unwrap_or(1);

    let (influence, selected) = select_references_for_batch(
        db,
        clone_id,
        next_batch_number,
        batch_size as usize,
        max_reference_generation_uses,
    )
    .await?;

    if selected.is_empty() {
        enqueue_refresh_pool(env, user_id, clone_id, "pool_depleted").await?;
        return Ok(None);
    }

    let batch_id = prefixed_id("blitz_batch");
    let stored_batch_size = stored_batch_size_for_selected_refs(batch_size, selected.len());
    let now = now_iso_string();
    let insert_result = db::run(
        db,
        r#"
        INSERT OR IGNORE INTO blitz_batches (
          id,
          user_id,
          clone_id,
          batch_number,
          batch_size,
          status,
          influence_json,
          created_at
        )
        VALUES (?, ?, ?, ?, ?, 'generating', ?, ?)
        "#,
        vec![
            json!(batch_id),
            json!(user_id),
            json!(clone_id),
            json!(next_batch_number),
            json!(stored_batch_size),
            json!(serde_json::to_string(&influence).unwrap_or_else(|_| "{}".to_string())),
            json!(now),
        ],
    )
    .await?;
    if changed_rows(&insert_result)? == 0 {
        let Some(existing) = load_batch_by_number(db, user_id, clone_id, next_batch_number).await?
        else {
            return Err(Error::RustError("blitz_batch_insert_conflict".to_string()));
        };
        ensure_batch_generation_enqueued(db, env, user_id, clone_id, provider_soul_id, &existing)
            .await?;
        return Ok(Some(existing.id));
    }

    let selected_ids = selected
        .into_iter()
        .map(|reference| reference.id)
        .collect::<Vec<_>>();
    enqueue_generation_batch(
        env,
        user_id,
        clone_id,
        &batch_id,
        provider_soul_id,
        selected_ids,
    )
    .await?;

    Ok(Some(batch_id))
}

pub async fn start_waiting_ready_pools(env: &Env) -> WorkerResult<u32> {
    let db = env.d1("DB")?;
    let rows = db::all::<WaitingReadyPoolRow>(
        &db,
        r#"
        SELECT user_id,
               id AS clone_id,
               TRIM(provider_soul_id) AS provider_soul_id
        FROM clone_profiles
        WHERE soul_status = 'ready'
          AND provider_soul_id IS NOT NULL
          AND TRIM(provider_soul_id) <> ''
          AND deleted_at IS NULL
          AND CASE
                WHEN json_valid(provider_config_json)
                THEN json_extract(provider_config_json, '$.nicheResearchStatus')
              END = 'pool_ready_awaiting_soul'
        ORDER BY updated_at ASC
        LIMIT 20
        "#,
        vec![],
    )
    .await?;

    let mut started = 0;
    for row in rows {
        if create_next_batch(&db, env, &row.user_id, &row.clone_id, &row.provider_soul_id)
            .await?
            .is_some()
        {
            let now = now_iso_string();
            let update_result = db::run(
                &db,
                r#"
                UPDATE clone_profiles
                SET provider_config_json = json_set(
                      CASE
                        WHEN json_valid(provider_config_json) THEN provider_config_json
                        ELSE '{}'
                      END,
                      '$.nicheResearchStatus',
                      'batch_generation_started',
                      '$.nicheResearchDetail',
                      'First Blitz batch queued after Soul readiness.',
                      '$.nicheResearchUpdatedAt',
                      ?
                    ),
                    updated_at = ?
                WHERE user_id = ?
                  AND id = ?
                  AND soul_status = 'ready'
                  AND provider_soul_id IS NOT NULL
                  AND TRIM(provider_soul_id) = ?
                  AND deleted_at IS NULL
                  AND CASE
                        WHEN json_valid(provider_config_json)
                        THEN json_extract(provider_config_json, '$.nicheResearchStatus')
                      END = 'pool_ready_awaiting_soul'
                "#,
                vec![
                    json!(now),
                    json!(now),
                    json!(row.user_id),
                    json!(row.clone_id),
                    json!(row.provider_soul_id),
                ],
            )
            .await?;
            if changed_rows(&update_result)? > 0 {
                started += 1;
            }
        }
    }

    Ok(started)
}

pub async fn reconcile_stale_batches(env: &Env) -> WorkerResult<()> {
    let db = env.d1("DB")?;
    if let Err(error) = start_waiting_ready_pools(env).await {
        console_error!("scheduled waiting ready pool startup failed: {}", error);
    }

    let stale_minutes = parse_stale_minutes_config(
        env.var("BLITZ_BATCH_STALE_MINUTES")
            .ok()
            .map(|value| value.to_string()),
    );
    let now = now_iso_string();
    let cutoff = stale_cutoff_iso(stale_minutes);

    let ready_result = db::run(
        &db,
        r#"
        UPDATE blitz_batches
        SET status = 'ready',
            generation_count = (
              SELECT COUNT(*)
              FROM generation_outputs go
              INNER JOIN generation_jobs gj
                ON gj.id = go.job_id
              WHERE gj.blitz_batch_id = blitz_batches.id
            ),
            ready_at = COALESCE(ready_at, ?),
            error_code = NULL,
            error_message = NULL
        WHERE status = 'generating'
          AND created_at < ?
          AND EXISTS (
            SELECT 1
            FROM generation_outputs go
            INNER JOIN generation_jobs gj
              ON gj.id = go.job_id
            WHERE gj.blitz_batch_id = blitz_batches.id
          )
        "#,
        vec![json!(&now), json!(&cutoff)],
    )
    .await?;

    let failed_result = db::run(
        &db,
        r#"
        UPDATE blitz_batches
        SET status = 'failed',
            generation_count = 0,
            error_code = 'stale_generation_batch',
            error_message = 'Batch was generating beyond the configured timeout.'
        WHERE status = 'generating'
          AND created_at < ?
          AND NOT EXISTS (
            SELECT 1
            FROM generation_outputs go
            INNER JOIN generation_jobs gj
              ON gj.id = go.job_id
            WHERE gj.blitz_batch_id = blitz_batches.id
          )
        "#,
        vec![json!(&cutoff)],
    )
    .await?;

    let ready_count = changed_rows(&ready_result)?;
    let failed_count = changed_rows(&failed_result)?;
    if ready_count > 0 || failed_count > 0 {
        console_log!(
            "stale blitz reconciliation marked ready={} failed={}",
            ready_count,
            failed_count
        );
    }

    Ok(())
}

pub async fn current_batch(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    usage: GenerationUsageSnapshot,
) -> WorkerResult<BlitzCurrentResponse> {
    let clone = load_user_clone(db, user_id, clone_id).await?;
    let Some(clone) = clone else {
        return Err(Error::RustError("clone_not_found".to_string()));
    };

    let batch = db::first::<BatchRow>(
        db,
        r#"
        SELECT
          bb.id,
          bb.clone_id,
          bb.batch_number,
          bb.status,
          bb.batch_size,
          cp.provider_soul_id
        FROM blitz_batches bb
        INNER JOIN clone_profiles cp
          ON cp.id = bb.clone_id
        WHERE bb.user_id = ?
          AND bb.clone_id = ?
          AND bb.status IN ('ready', 'active')
        ORDER BY bb.batch_number ASC, bb.created_at ASC
        LIMIT 1
        "#,
        vec![json!(user_id), json!(clone_id)],
    )
    .await?;

    let Some(batch) = batch else {
        if let Some(latest) = load_latest_unavailable_batch(db, user_id, clone_id).await? {
            return Ok(BlitzCurrentResponse {
                batch: None,
                status: Some(latest.status.clone()),
                progress: Some(progress_for_unavailable_batch(&latest)),
                usage,
                next_batch_status: Some(latest.status),
            });
        }

        let status = provider_niche_research_status(&clone.provider_config_json)
            .unwrap_or_else(|| "generating".to_string());
        return Ok(BlitzCurrentResponse {
            batch: None,
            status: Some(status.clone()),
            progress: Some(progress_for_provider_status(&status)),
            usage,
            next_batch_status: Some(status),
        });
    };

    let status = if batch.status == "ready" {
        mark_batch_active(db, &batch.id).await?;
        "active".to_string()
    } else {
        batch.status.clone()
    };
    let images = load_batch_images(db, user_id, clone_id, &batch.id).await?;
    if !images.is_empty() {
        sync_batch_size_to_output_count(db, &batch.id, images.len() as u32).await?;
    }
    let next_batch_status =
        load_next_batch_status(db, user_id, clone_id, batch.batch_number).await?;

    Ok(BlitzCurrentResponse {
        batch: Some(BlitzBatchResponse {
            id: batch.id,
            batch_number: batch.batch_number,
            status,
            images,
        }),
        status: None,
        progress: None,
        usage,
        next_batch_status,
    })
}

pub async fn record_swipe(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    batch_id: &str,
    output_id: &str,
    action: &str,
) -> WorkerResult<SwipeResponse> {
    let action =
        swipe_action_to_db_value(action).map_err(|error| Error::RustError(error.to_string()))?;
    let Some(batch) = load_batch_for_swipe(db, user_id, batch_id).await? else {
        return Err(Error::RustError("blitz_batch_not_found".to_string()));
    };
    if !swipeable_batch_status(&batch.status) {
        return Err(Error::RustError("blitz_batch_not_swipeable".to_string()));
    }
    let Some(output) = load_output_for_swipe(db, user_id, batch_id, output_id).await? else {
        return Err(Error::RustError("generation_output_not_found".to_string()));
    };
    let swipe_index = load_swipe_index(db, batch_id, output_id).await?;
    let now = now_iso_string();
    let metadata = json!({
        "aestheticTags": parse_string_array(output.aesthetic_tags_json.as_deref().unwrap_or("[]")),
        "nicheCluster": output.niche_cluster,
        "sourcePlatform": output.source_platform.clone().unwrap_or_default(),
        "visualReferenceId": output.visual_reference_id,
    });

    let insert_result = db::run(
        db,
        r#"
        INSERT OR IGNORE INTO blitz_swipes (
          id,
          user_id,
          clone_id,
          batch_id,
          generation_output_id,
          visual_reference_id,
          action,
          output_metadata_json,
          swipe_index,
          created_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        vec![
            json!(prefixed_id("blitz_swipe")),
            json!(user_id),
            json!(batch.clone_id),
            json!(batch_id),
            json!(output_id),
            json!(metadata
                .get("visualReferenceId")
                .cloned()
                .unwrap_or(Value::Null)),
            json!(action),
            json!(metadata.to_string()),
            json!(swipe_index),
            json!(now),
        ],
    )
    .await?;
    let inserted_new_swipe = changed_rows(&insert_result)? > 0;

    repair_batch_swipe_counts(db, batch_id).await?;
    repair_liked_reference_for_output(db, batch_id, output_id, &now).await?;

    let mut next_batch_triggered = false;
    let swipe_count = count_swipes(db, batch_id).await?;
    if first_swipe_prefetch_should_run(swipe_count) {
        let provider_soul_id = batch
            .provider_soul_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::RustError("provider_soul_id_missing".to_string()))?;
        next_batch_triggered =
            create_next_batch(db, env, user_id, &batch.clone_id, provider_soul_id)
                .await?
                .is_some();
    }

    let output_count = count_swipeable_outputs(db, batch_id).await?;
    if output_count > 0 {
        sync_batch_size_to_output_count(db, batch_id, output_count).await?;
    }
    let progress_total = if output_count > 0 {
        output_count
    } else {
        batch.batch_size
    };
    let batch_complete = batch_complete_for_swipe_count(swipe_count, output_count);
    if batch_complete {
        mark_batch_completed(db, batch_id, &now).await?;
    }

    if !inserted_new_swipe {
        return Err(Error::RustError("duplicate_swipe".to_string()));
    }

    Ok(SwipeResponse {
        swipe_index,
        batch_progress: format!("{swipe_count}/{progress_total}"),
        batch_complete,
        next_batch_triggered,
    })
}

pub async fn history(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    limit: u32,
) -> WorkerResult<BlitzHistoryResponse> {
    let batches = db::all::<BlitzHistoryBatch>(
        db,
        r#"
        SELECT id, batch_number, like_count, dislike_count, completed_at
        FROM blitz_batches
        WHERE user_id = ?
          AND clone_id = ?
          AND status = 'completed'
        ORDER BY batch_number DESC
        LIMIT ?
        "#,
        vec![json!(user_id), json!(clone_id), json!(limit)],
    )
    .await?;

    Ok(BlitzHistoryResponse { batches })
}

async fn load_blitz_selection_config(db: &D1Database) -> WorkerResult<(u32, u32)> {
    let rows = db::all::<ConfigRow>(
        db,
        r#"
        SELECT key, value
        FROM blitz_config
        WHERE key IN ('batch_size', 'max_reference_generation_uses')
        "#,
        vec![],
    )
    .await?;

    let mut batch_size = 5;
    let mut max_reference_generation_uses = 4;
    for row in rows {
        let parsed = row.value.parse::<u32>().unwrap_or(0);
        match row.key.as_str() {
            "batch_size" if parsed > 0 => batch_size = parsed,
            "max_reference_generation_uses" if parsed > 0 => max_reference_generation_uses = parsed,
            _ => {}
        }
    }

    Ok((batch_size, max_reference_generation_uses))
}

async fn ensure_batch_generation_enqueued(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    provider_soul_id: &str,
    batch: &ExistingBatchRow,
) -> WorkerResult<()> {
    let counts = load_batch_generation_counts(db, &batch.id).await?;
    if counts.job_count > 0 || counts.output_count > 0 {
        return Ok(());
    }
    if !matches!(batch.status.as_str(), "pending" | "generating" | "ready") {
        return Ok(());
    }

    let (_, max_reference_generation_uses) = load_blitz_selection_config(db).await?;
    let (influence, selected) = select_references_for_batch(
        db,
        clone_id,
        batch.batch_number,
        batch.batch_size as usize,
        max_reference_generation_uses,
    )
    .await?;

    if selected.is_empty() {
        enqueue_refresh_pool(env, user_id, clone_id, "pool_depleted").await?;
        return Ok(());
    }

    let selected_ids = selected
        .iter()
        .map(|reference| reference.id.clone())
        .collect::<Vec<_>>();
    let stored_batch_size =
        stored_batch_size_for_selected_refs(batch.batch_size, selected_ids.len());
    db::exec(
        db,
        r#"
        UPDATE blitz_batches
        SET status = 'generating',
            batch_size = ?,
            influence_json = ?
        WHERE id = ?
          AND status IN ('pending', 'generating', 'ready')
        "#,
        vec![
            json!(stored_batch_size),
            json!(serde_json::to_string(&influence).unwrap_or_else(|_| "{}".to_string())),
            json!(&batch.id),
        ],
    )
    .await?;

    enqueue_generation_batch(
        env,
        user_id,
        clone_id,
        &batch.id,
        provider_soul_id,
        selected_ids,
    )
    .await
}

async fn select_references_for_batch(
    db: &D1Database,
    clone_id: &str,
    batch_number: u32,
    batch_size: usize,
    max_reference_generation_uses: u32,
) -> WorkerResult<(
    crate::domain::blitz::Influence,
    Vec<VisualReferenceForSelection>,
)> {
    let influence = load_influence(db, clone_id, batch_number).await?;
    let references = load_visual_references_for_selection(db, clone_id).await?;
    let selected = select_visual_references(
        &references,
        &influence,
        batch_size,
        max_reference_generation_uses,
        &now_iso_string(),
    );

    Ok((influence, selected))
}

async fn enqueue_generation_batch(
    env: &Env,
    user_id: &str,
    clone_id: &str,
    batch_id: &str,
    provider_soul_id: &str,
    visual_reference_ids: Vec<String>,
) -> WorkerResult<()> {
    env.queue("GENERATION_QUEUE")?
        .send(GenerationMessage::GenerateBlitzBatch {
            batch_id: batch_id.to_string(),
            clone_id: clone_id.to_string(),
            user_id: user_id.to_string(),
            idempotency_key: format!("blitz_gen:{batch_id}"),
            visual_reference_ids,
            provider_soul_id: provider_soul_id.to_string(),
        })
        .await
}

async fn enqueue_refresh_pool(
    env: &Env,
    user_id: &str,
    clone_id: &str,
    reason: &str,
) -> WorkerResult<()> {
    env.queue("NICHE_RESEARCH_QUEUE")?
        .send(NicheResearchMessage::RefreshPool {
            user_id: user_id.to_string(),
            clone_id: clone_id.to_string(),
            reason: reason.to_string(),
        })
        .await
}

async fn load_batch_by_number(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    batch_number: u32,
) -> WorkerResult<Option<ExistingBatchRow>> {
    db::first::<ExistingBatchRow>(
        db,
        r#"
        SELECT id, status, batch_number, batch_size
        FROM blitz_batches
        WHERE user_id = ?
          AND clone_id = ?
          AND batch_number = ?
        "#,
        vec![json!(user_id), json!(clone_id), json!(batch_number)],
    )
    .await
}

async fn load_batch_generation_counts(
    db: &D1Database,
    batch_id: &str,
) -> WorkerResult<BatchGenerationCountsRow> {
    let row = db::first::<BatchGenerationCountsRow>(
        db,
        r#"
        SELECT
          COUNT(DISTINCT gj.id) AS job_count,
          COUNT(DISTINCT go.id) AS output_count
        FROM generation_jobs gj
        LEFT JOIN generation_outputs go
          ON go.job_id = gj.id
        WHERE gj.blitz_batch_id = ?
        "#,
        vec![json!(batch_id)],
    )
    .await?;

    Ok(row.unwrap_or(BatchGenerationCountsRow {
        job_count: 0,
        output_count: 0,
    }))
}

async fn load_influence(
    db: &D1Database,
    clone_id: &str,
    next_batch_number: u32,
) -> WorkerResult<crate::domain::blitz::Influence> {
    let cutoff = trigger_influence_cutoff_batch_number(next_batch_number);
    let rows = db::all::<SwipeInfluenceRow>(
        db,
        r#"
        SELECT bs.action, bs.output_metadata_json
        FROM blitz_swipes bs
        INNER JOIN blitz_batches bb
          ON bb.id = bs.batch_id
        WHERE bb.clone_id = ?
          AND bb.status = 'completed'
          AND bb.batch_number <= ?
        ORDER BY bs.created_at ASC
        "#,
        vec![json!(clone_id), json!(cutoff)],
    )
    .await?;

    let swipes = rows
        .into_iter()
        .map(|row| {
            let snapshot =
                serde_json::from_str::<SwipeMetadataSnapshot>(&row.output_metadata_json).ok();
            SwipeMetadata {
                action: row.action,
                aesthetic_tags: snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.aesthetic_tags.clone())
                    .unwrap_or_default(),
                niche_cluster: snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.niche_cluster.clone()),
                source_platform: snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.source_platform.clone())
                    .unwrap_or_default(),
                visual_reference_id: snapshot.and_then(|snapshot| snapshot.visual_reference_id),
            }
        })
        .collect::<Vec<_>>();

    Ok(accumulate_influence(&swipes))
}

async fn load_visual_references_for_selection(
    db: &D1Database,
    clone_id: &str,
) -> WorkerResult<Vec<VisualReferenceForSelection>> {
    let rows = db::all::<VisualReferenceRow>(
        db,
        r#"
        SELECT
          id,
          source_platform,
          source_published_at,
          niche_cluster,
          aesthetic_tags_json,
          human_presence_score,
          organic_photo_score,
          freshness_visual_score,
          generation_use_count,
          last_liked_at
        FROM visual_references
        WHERE clone_id = ?
          AND status = 'active'
        "#,
        vec![json!(clone_id)],
    )
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| VisualReferenceForSelection {
            id: row.id,
            source_platform: row.source_platform,
            source_published_at: row.source_published_at,
            niche_cluster: row.niche_cluster,
            aesthetic_tags: parse_string_array(&row.aesthetic_tags_json),
            human_presence_score: row.human_presence_score,
            organic_photo_score: row.organic_photo_score,
            freshness_visual_score: row.freshness_visual_score,
            generation_use_count: row.generation_use_count,
            last_liked_at: row.last_liked_at,
        })
        .collect())
}

async fn load_user_clone(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
) -> WorkerResult<Option<CloneRow>> {
    db::first::<CloneRow>(
        db,
        r#"
        SELECT provider_config_json
        FROM clone_profiles
        WHERE user_id = ?
          AND id = ?
          AND deleted_at IS NULL
        "#,
        vec![json!(user_id), json!(clone_id)],
    )
    .await
}

async fn load_latest_unavailable_batch(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
) -> WorkerResult<Option<LatestBatchStateRow>> {
    db::first::<LatestBatchStateRow>(
        db,
        r#"
        SELECT status, error_message
        FROM blitz_batches
        WHERE user_id = ?
          AND clone_id = ?
          AND status IN ('pending', 'generating', 'failed')
        ORDER BY batch_number DESC, created_at DESC
        LIMIT 1
        "#,
        vec![json!(user_id), json!(clone_id)],
    )
    .await
}

fn progress_for_unavailable_batch(batch: &LatestBatchStateRow) -> BlitzProgressResponse {
    match batch.status.as_str() {
        "failed" => BlitzProgressResponse {
            phase: "generation_failed".to_string(),
            detail: batch
                .error_message
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "Image generation failed.".to_string()),
        },
        "pending" => BlitzProgressResponse {
            phase: "generation".to_string(),
            detail: "Preparing image generation...".to_string(),
        },
        _ => BlitzProgressResponse {
            phase: "generation".to_string(),
            detail: "Generating images...".to_string(),
        },
    }
}

fn progress_for_provider_status(status: &str) -> BlitzProgressResponse {
    let detail = match status {
        "refresh_requested" => "Refreshing visual references...",
        "failed" => "Visual reference refresh failed.",
        _ => "Scraping visual references...",
    };

    BlitzProgressResponse {
        phase: "niche_research".to_string(),
        detail: detail.to_string(),
    }
}

async fn mark_batch_active(db: &D1Database, batch_id: &str) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE blitz_batches
        SET status = 'active',
            served_at = COALESCE(served_at, ?)
        WHERE id = ?
          AND status = 'ready'
        "#,
        vec![json!(now), json!(batch_id)],
    )
    .await
}

async fn load_batch_images(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    batch_id: &str,
) -> WorkerResult<Vec<BlitzImageResponse>> {
    let rows = db::all::<OutputResponseRow>(
        db,
        r#"
        SELECT
          go.id AS output_id,
          COALESCE('/api/media/' || go.media_asset_id, go.raw_url, '') AS media_url,
          gj.input_visual_reference_id AS visual_reference_id,
          CAST(ROW_NUMBER() OVER (ORDER BY go.output_index ASC, go.created_at ASC) - 1 AS INTEGER) AS swipe_index,
          CASE WHEN bs.id IS NULL THEN 0 ELSE 1 END AS swiped
        FROM generation_outputs go
        INNER JOIN generation_jobs gj
          ON gj.id = go.job_id
        LEFT JOIN blitz_swipes bs
          ON bs.generation_output_id = go.id
         AND bs.batch_id = gj.blitz_batch_id
        WHERE gj.blitz_batch_id = ?
          AND go.user_id = ?
          AND go.clone_id = ?
        ORDER BY go.output_index ASC, go.created_at ASC
        "#,
        vec![json!(batch_id), json!(user_id), json!(clone_id)],
    )
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| BlitzImageResponse {
            output_id: row.output_id,
            media_url: row.media_url,
            visual_reference_id: row.visual_reference_id,
            swipe_index: row.swipe_index,
            swiped: row.swiped != 0,
        })
        .collect())
}

async fn load_next_batch_status(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    batch_number: u32,
) -> WorkerResult<Option<String>> {
    let row = db::first::<StatusRow>(
        db,
        r#"
        SELECT status
        FROM blitz_batches
        WHERE user_id = ?
          AND clone_id = ?
          AND batch_number > ?
          AND status IN ('pending', 'generating', 'ready')
        ORDER BY batch_number ASC
        LIMIT 1
        "#,
        vec![json!(user_id), json!(clone_id), json!(batch_number)],
    )
    .await?;

    Ok(row.map(|row| row.status))
}

async fn load_batch_for_swipe(
    db: &D1Database,
    user_id: &str,
    batch_id: &str,
) -> WorkerResult<Option<BatchRow>> {
    db::first::<BatchRow>(
        db,
        r#"
        SELECT
          bb.id,
          bb.clone_id,
          bb.batch_number,
          bb.status,
          bb.batch_size,
          cp.provider_soul_id
        FROM blitz_batches bb
        INNER JOIN clone_profiles cp
          ON cp.id = bb.clone_id
        WHERE bb.id = ?
          AND bb.user_id = ?
          AND bb.status IN ('ready', 'active')
          AND cp.deleted_at IS NULL
        "#,
        vec![json!(batch_id), json!(user_id)],
    )
    .await
}

async fn load_output_for_swipe(
    db: &D1Database,
    user_id: &str,
    batch_id: &str,
    output_id: &str,
) -> WorkerResult<Option<OutputSwipeRow>> {
    db::first::<OutputSwipeRow>(
        db,
        r#"
        SELECT
          gj.input_visual_reference_id AS visual_reference_id,
          vr.source_platform,
          vr.niche_cluster,
          vr.aesthetic_tags_json
        FROM generation_outputs go
        INNER JOIN generation_jobs gj
          ON gj.id = go.job_id
        LEFT JOIN visual_references vr
          ON vr.id = gj.input_visual_reference_id
        WHERE go.id = ?
          AND go.user_id = ?
          AND gj.blitz_batch_id = ?
        "#,
        vec![json!(output_id), json!(user_id), json!(batch_id)],
    )
    .await
}

async fn load_swipe_index(db: &D1Database, batch_id: &str, output_id: &str) -> WorkerResult<u32> {
    let row = db::first::<SwipeIndexRow>(
        db,
        r#"
        SELECT swipe_index
        FROM (
          SELECT
            go.id,
            CAST(ROW_NUMBER() OVER (ORDER BY go.output_index ASC, go.created_at ASC) - 1 AS INTEGER) AS swipe_index
          FROM generation_outputs go
          INNER JOIN generation_jobs gj
            ON gj.id = go.job_id
          WHERE gj.blitz_batch_id = ?
        )
        WHERE id = ?
        "#,
        vec![json!(batch_id), json!(output_id)],
    )
    .await?;

    row.map(|row| row.swipe_index)
        .ok_or_else(|| Error::RustError("generation_output_not_found".to_string()))
}

async fn count_swipes(db: &D1Database, batch_id: &str) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM blitz_swipes
        WHERE batch_id = ?
        "#,
        vec![json!(batch_id)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn count_swipeable_outputs(db: &D1Database, batch_id: &str) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM generation_outputs go
        INNER JOIN generation_jobs gj
          ON gj.id = go.job_id
        WHERE gj.blitz_batch_id = ?
        "#,
        vec![json!(batch_id)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn sync_batch_size_to_output_count(
    db: &D1Database,
    batch_id: &str,
    output_count: u32,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE blitz_batches
        SET batch_size = ?
        WHERE id = ?
          AND batch_size != ?
        "#,
        vec![json!(output_count), json!(batch_id), json!(output_count)],
    )
    .await
}

async fn repair_batch_swipe_counts(db: &D1Database, batch_id: &str) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE blitz_batches
        SET like_count = (
              SELECT COUNT(*)
              FROM blitz_swipes
              WHERE batch_id = ?
                AND action = 'like'
            ),
            dislike_count = (
              SELECT COUNT(*)
              FROM blitz_swipes
              WHERE batch_id = ?
                AND action = 'dislike'
            )
        WHERE id = ?
        "#,
        vec![json!(batch_id), json!(batch_id), json!(batch_id)],
    )
    .await
}

async fn repair_liked_reference_for_output(
    db: &D1Database,
    batch_id: &str,
    output_id: &str,
    now: &str,
) -> WorkerResult<()> {
    let Some(swipe) = db::first::<ExistingSwipeRow>(
        db,
        r#"
        SELECT action, visual_reference_id
        FROM blitz_swipes
        WHERE batch_id = ?
          AND generation_output_id = ?
        LIMIT 1
        "#,
        vec![json!(batch_id), json!(output_id)],
    )
    .await?
    else {
        return Ok(());
    };

    if swipe.action == "like" {
        if let Some(visual_reference_id) = swipe
            .visual_reference_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            mark_visual_reference_liked(db, visual_reference_id, now).await?;
        }
    }

    Ok(())
}

async fn mark_visual_reference_liked(
    db: &D1Database,
    visual_reference_id: &str,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE visual_references
        SET last_liked_at = ?
        WHERE id = ?
        "#,
        vec![json!(now), json!(visual_reference_id)],
    )
    .await
}

async fn mark_batch_completed(db: &D1Database, batch_id: &str, now: &str) -> WorkerResult<()> {
    db::exec(
        db,
        r#"
        UPDATE blitz_batches
        SET status = 'completed',
            completed_at = COALESCE(completed_at, ?)
        WHERE id = ?
          AND status != 'completed'
        "#,
        vec![json!(now), json!(batch_id)],
    )
    .await
}

fn provider_niche_research_status(provider_config_json: &str) -> Option<String> {
    serde_json::from_str::<Value>(provider_config_json)
        .ok()
        .and_then(|value| {
            value
                .get("nicheResearchStatus")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

fn parse_string_array(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
}

fn changed_rows(result: &worker::D1Result) -> WorkerResult<usize> {
    Ok(result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or_default())
}

fn prefixed_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

fn parse_stale_minutes_config(value: Option<String>) -> i64 {
    value
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(45)
}

fn stale_cutoff_iso(minutes: i64) -> String {
    let millis = js_sys::Date::now() - (minutes as f64 * 60_000.0);
    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(millis))
        .to_iso_string()
        .into()
}

#[cfg(test)]
mod tests {
    use super::parse_stale_minutes_config;

    #[test]
    fn parse_stale_minutes_config_uses_positive_override() {
        assert_eq!(parse_stale_minutes_config(Some("30".to_string())), 30);
        assert_eq!(parse_stale_minutes_config(Some(" 30 ".to_string())), 30);
    }

    #[test]
    fn parse_stale_minutes_config_defaults_for_missing_or_invalid_values() {
        assert_eq!(parse_stale_minutes_config(None), 45);
        assert_eq!(
            parse_stale_minutes_config(Some("not-a-number".to_string())),
            45
        );
        assert_eq!(parse_stale_minutes_config(Some("0".to_string())), 45);
        assert_eq!(parse_stale_minutes_config(Some("-1".to_string())), 45);
    }
}
