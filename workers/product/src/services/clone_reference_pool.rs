use crate::db;
use crate::domain::clone_reference_pool::{
    clone_pool_run_is_reusable, compatibility_action_for, select_balanced_compatibility_wave,
    CompatibilityAction, GlobalReferenceForClonePool,
};
use crate::domain::moodboards::selected_moodboard_hash;
use crate::queues::messages::ReferencePipelineMessage;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use wasm_bindgen::JsValue;
use worker::{D1Database, Env, Result as WorkerResult};

const REFERENCE_QUEUE_NAME: &str = "REFERENCE_PIPELINE_QUEUE";
const REFERENCE_QUEUE_STORAGE_NAME: &str = "mirai-reference-pipeline";

#[derive(Debug, Deserialize)]
struct ConfigRow {
    value: String,
}

#[derive(Debug, Deserialize)]
struct CloneForPoolRow {
    id: String,
    user_id: String,
    soul_status: Option<String>,
    provider_soul_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SelectedMoodboardRow {
    id: String,
    slug: String,
}

#[derive(Debug, Deserialize)]
struct PoolRunRow {
    id: String,
    status: String,
    selected_moodboard_hash: String,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GlobalReferenceActionableRow {
    id: String,
    moodboard_slug: String,
    overall_reference_score: f64,
    generation_use_count: u32,
    compatibility_status: Option<String>,
    next_retry_at: Option<String>,
    visual_reference_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u32,
}

#[derive(Debug, Default)]
struct PoolConfig {
    batch_size: u32,
    global_refs_per_moodboard_target: u32,
    global_refs_for_pool_min: u32,
    clone_pool_run_stale_after_minutes: i64,
    clone_pool_global_reference_review_limit: usize,
    clone_pool_compatibility_wave_size: usize,
}

fn load_clone_for_pool_sql() -> &'static str {
    r#"
    SELECT id, user_id, soul_status, provider_soul_id
    FROM clone_profiles
    WHERE user_id = ?
      AND id = ?
      AND deleted_at IS NULL
      AND status = 'active'
      AND soul_status IN ('ready', 'completed')
      AND provider_soul_id IS NOT NULL
      AND TRIM(provider_soul_id) <> ''
    LIMIT 1
    "#
}

fn load_current_selected_moodboard_snapshot_sql() -> &'static str {
    r#"
    SELECT mb.id, mb.slug
    FROM moodboards mb
    INNER JOIN global_moodboard_definitions gmd
      ON gmd.slug = mb.slug
    WHERE mb.user_id = ?
      AND mb.selected = 1
      AND gmd.status = 'active'
    ORDER BY mb.slug ASC
    "#
}

fn load_current_pool_run_sql() -> &'static str {
    r#"
    SELECT cpr.id, cpr.status, cpr.selected_moodboard_hash, cpr.updated_at
    FROM clone_reference_state crs
    INNER JOIN clone_pool_runs cpr
      ON cpr.id = crs.current_pool_run_id
     AND cpr.clone_id = crs.clone_id
    WHERE crs.user_id = ?
      AND crs.clone_id = ?
    LIMIT 1
    "#
}

fn insert_clone_pool_run_sql() -> &'static str {
    r#"
    INSERT OR IGNORE INTO clone_pool_runs (
      id, user_id, clone_id, status, reason,
      selected_moodboard_ids_snapshot_json,
      selected_moodboard_slugs_snapshot_json,
      selected_moodboard_hash,
      waiting_moodboard_slugs_json,
      created_at, updated_at, started_at
    )
    VALUES (?, ?, ?, 'queued', ?, ?, ?, ?, '[]', ?, ?, ?)
    "#
}

fn claim_clone_reference_state_for_run_sql() -> &'static str {
    r#"
    INSERT INTO clone_reference_state (
      clone_id, user_id, current_pool_run_id, selected_moodboard_hash,
      status, waiting_moodboard_slugs_json, created_at, updated_at
    )
    VALUES (?, ?, ?, ?, 'queued', '[]', ?, ?)
    ON CONFLICT(clone_id) DO UPDATE SET
      user_id = excluded.user_id,
      current_pool_run_id = excluded.current_pool_run_id,
      selected_moodboard_hash = excluded.selected_moodboard_hash,
      status = 'queued',
      waiting_moodboard_slugs_json = '[]',
      updated_at = excluded.updated_at
    WHERE clone_reference_state.current_pool_run_id IS NULL
       OR clone_reference_state.selected_moodboard_hash <> excluded.selected_moodboard_hash
       OR clone_reference_state.status NOT IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
       OR clone_reference_state.updated_at <= ?
    "#
}

fn select_actionable_global_references_sql(selected_slug_params: &str) -> String {
    format!(
        r#"
        SELECT
          gmr.id,
          gmr.moodboard_slug,
          gmr.overall_reference_score,
          COALESCE(vr.generation_use_count, 0) AS generation_use_count,
          cvr.status AS compatibility_status,
          cvr.next_retry_at,
          vr.id AS visual_reference_id
        FROM global_moodboard_references gmr
        LEFT JOIN clone_visual_reference_compatibility cvr
          ON cvr.clone_id = ?
         AND cvr.global_reference_id = gmr.id
        LEFT JOIN visual_references vr
          ON vr.clone_id = ?
         AND vr.global_reference_id = gmr.id
         AND vr.status = 'active'
        WHERE gmr.status = 'active'
          AND gmr.moodboard_slug IN ({selected_slug_params})
          AND (
            cvr.status IS NULL
            OR cvr.status = 'queued'
            OR (cvr.status = 'accepted' AND vr.id IS NULL)
            OR (
              cvr.status = 'failed'
              AND cvr.next_retry_at IS NOT NULL
              AND cvr.next_retry_at <= ?
            )
          )
        ORDER BY
          gmr.moodboard_slug ASC,
          CASE WHEN cvr.status IS NULL THEN 0 WHEN cvr.status = 'queued' THEN 1 ELSE 2 END ASC,
          gmr.overall_reference_score DESC,
          COALESCE(vr.generation_use_count, 0) ASC,
          gmr.created_at DESC
        LIMIT ?
        "#
    )
}

fn reserve_reference_pipeline_message_sql() -> &'static str {
    r#"
    INSERT INTO queue_message_reservations (
      id, queue_name, message_kind, dedupe_key, pool_run_id,
      status, created_at, updated_at, expires_at
    )
    VALUES (?, ?, ?, ?, ?, 'reserved', ?, ?, ?)
    ON CONFLICT(queue_name, message_kind, dedupe_key) DO UPDATE SET
      id = excluded.id,
      pool_run_id = excluded.pool_run_id,
      status = 'reserved',
      updated_at = excluded.updated_at,
      expires_at = excluded.expires_at
    WHERE queue_message_reservations.status = 'failed'
       OR queue_message_reservations.status = 'reserved' AND queue_message_reservations.expires_at <= excluded.created_at
    "#
}

fn mark_reserved_reference_pipeline_message_enqueued_sql() -> &'static str {
    r#"
    UPDATE queue_message_reservations
    SET status = 'enqueued',
        updated_at = ?
    WHERE queue_name = ?
      AND message_kind = ?
      AND dedupe_key = ?
      AND status = 'reserved'
    "#
}

fn mark_reserved_reference_pipeline_message_failed_sql() -> &'static str {
    r#"
    UPDATE queue_message_reservations
    SET status = 'failed',
        updated_at = ?
    WHERE queue_name = ?
      AND message_kind = ?
      AND dedupe_key = ?
      AND id = ?
      AND status = 'reserved'
    "#
}

fn update_pool_status_sql() -> &'static str {
    r#"
    UPDATE clone_pool_runs
    SET status = ?,
        waiting_moodboard_slugs_json = ?,
        updated_at = ?
    WHERE id = ?
      AND user_id = ?
      AND clone_id = ?
    "#
}

fn update_clone_reference_state_status_sql() -> &'static str {
    r#"
    UPDATE clone_reference_state
    SET status = ?,
        waiting_moodboard_slugs_json = ?,
        updated_at = ?
    WHERE user_id = ?
      AND clone_id = ?
      AND current_pool_run_id = ?
    "#
}

fn mark_unclaimed_clone_pool_run_superseded_sql() -> &'static str {
    r#"
    UPDATE clone_pool_runs
    SET status = 'superseded',
        updated_at = ?
    WHERE id = ?
      AND status = 'queued'
    "#
}

pub async fn build_or_refresh_clone_pool(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    reason: &str,
) -> WorkerResult<()> {
    let Some(clone) = db::first::<CloneForPoolRow>(
        db,
        load_clone_for_pool_sql(),
        vec![json!(user_id), json!(clone_id)],
    )
    .await?
    else {
        return Ok(());
    };
    let _ = (
        &clone.id,
        &clone.user_id,
        &clone.soul_status,
        &clone.provider_soul_id,
    );

    let selected = db::all::<SelectedMoodboardRow>(
        db,
        load_current_selected_moodboard_snapshot_sql(),
        vec![json!(user_id)],
    )
    .await?;
    if selected.is_empty() {
        return Ok(());
    }

    let config = load_pool_config(db).await?;
    let _ = (config.batch_size, config.global_refs_for_pool_min);
    let selected_ids = selected
        .iter()
        .map(|row| row.id.clone())
        .collect::<Vec<_>>();
    let selected_slugs = selected
        .iter()
        .map(|row| row.slug.clone())
        .collect::<Vec<_>>();
    let selected_hash = selected_moodboard_hash(&selected_slugs);
    let now = now_iso_string();

    let pool_run_id =
        match reusable_pool_run(db, user_id, clone_id, &selected_hash, &now, &config).await? {
            Some(run_id) => run_id,
            None => {
                create_clone_pool_run(
                    db,
                    user_id,
                    clone_id,
                    reason,
                    &selected_ids,
                    &selected_slugs,
                    &selected_hash,
                    &now,
                    config.clone_pool_run_stale_after_minutes,
                )
                .await?
            }
        };

    enqueue_global_topups_for_underfilled_selected_slugs(
        db,
        env,
        &selected_slugs,
        config.global_refs_per_moodboard_target,
        "clone_pool_topup",
    )
    .await?;

    let actionable = load_actionable_global_references(
        db,
        clone_id,
        &selected_slugs,
        config.clone_pool_global_reference_review_limit,
        &now,
    )
    .await?;

    if actionable.is_empty() {
        mark_pool_waiting_for_global_library(
            db,
            user_id,
            clone_id,
            &pool_run_id,
            &selected_slugs,
            &now,
        )
        .await?;
        return Ok(());
    }

    repair_already_accepted_references(db, user_id, clone_id, &pool_run_id, &actionable, &now)
        .await?;
    schedule_compatibility_wave(
        db,
        env,
        user_id,
        clone_id,
        &pool_run_id,
        &selected_slugs,
        actionable,
        &config,
        &now,
    )
    .await?;
    enqueue_finalize_clone_pool(
        db,
        env,
        user_id,
        clone_id,
        &pool_run_id,
        "wave_scheduled",
        &now,
    )
    .await
}

async fn reusable_pool_run(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    selected_hash: &str,
    now: &str,
    config: &PoolConfig,
) -> WorkerResult<Option<String>> {
    let Some(run) = db::first::<PoolRunRow>(
        db,
        load_current_pool_run_sql(),
        vec![json!(user_id), json!(clone_id)],
    )
    .await?
    else {
        return Ok(None);
    };

    let selected_hash_matches = run.selected_moodboard_hash == selected_hash;
    if clone_pool_run_is_reusable(
        &run.status,
        selected_hash_matches,
        run.updated_at.as_deref(),
        now,
        config.clone_pool_run_stale_after_minutes,
    ) {
        Ok(Some(run.id))
    } else {
        Ok(None)
    }
}

async fn create_clone_pool_run(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    reason: &str,
    selected_ids: &[String],
    selected_slugs: &[String],
    selected_hash: &str,
    now: &str,
    stale_after_minutes: i64,
) -> WorkerResult<String> {
    let pool_run_id = format!("clone_pool_run_{}", Uuid::new_v4().simple());
    db::exec(
        db,
        insert_clone_pool_run_sql(),
        vec![
            json!(pool_run_id),
            json!(user_id),
            json!(clone_id),
            json!(reason),
            json!(selected_ids),
            json!(selected_slugs),
            json!(selected_hash),
            json!(now),
            json!(now),
            json!(now),
        ],
    )
    .await?;
    let stale_cutoff = add_minutes_iso(now, -stale_after_minutes.max(1));
    db::exec(
        db,
        claim_clone_reference_state_for_run_sql(),
        vec![
            json!(clone_id),
            json!(user_id),
            json!(pool_run_id),
            json!(selected_hash),
            json!(now),
            json!(now),
            json!(stale_cutoff),
        ],
    )
    .await?;

    let Some(claimed_run) = db::first::<PoolRunRow>(
        db,
        load_current_pool_run_sql(),
        vec![json!(user_id), json!(clone_id)],
    )
    .await?
    else {
        return Ok(pool_run_id);
    };

    let claimed_pool_run_id = claimed_run.id;
    if claimed_pool_run_id != pool_run_id {
        mark_unclaimed_clone_pool_run_superseded(db, &pool_run_id, now).await?;
    }

    Ok(claimed_pool_run_id)
}

async fn load_actionable_global_references(
    db: &D1Database,
    clone_id: &str,
    selected_slugs: &[String],
    limit: usize,
    now: &str,
) -> WorkerResult<Vec<GlobalReferenceActionableRow>> {
    if selected_slugs.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let selected_slug_params = std::iter::repeat("?")
        .take(selected_slugs.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut params = vec![json!(clone_id), json!(clone_id)];
    params.extend(selected_slugs.iter().map(|slug| json!(slug)));
    params.push(json!(now));
    params.push(json!(limit as u32));

    db::all(
        db,
        &select_actionable_global_references_sql(&selected_slug_params),
        params,
    )
    .await
}

async fn schedule_compatibility_wave(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    selected_slugs: &[String],
    rows: Vec<GlobalReferenceActionableRow>,
    config: &PoolConfig,
    now: &str,
) -> WorkerResult<()> {
    let reviewable = rows
        .into_iter()
        .filter(|row| {
            compatibility_action_for(
                row.compatibility_status.as_deref(),
                row.next_retry_at.as_deref(),
                row.visual_reference_id.is_some(),
                now,
            ) == CompatibilityAction::EnqueueReview
        })
        .map(|row| {
            GlobalReferenceForClonePool::new(
                row.id,
                row.moodboard_slug,
                row.overall_reference_score,
                row.generation_use_count,
            )
        })
        .collect::<Vec<_>>();

    let selected = select_balanced_compatibility_wave(
        reviewable,
        selected_slugs,
        config.clone_pool_compatibility_wave_size,
    );

    for reference in selected {
        reserve_and_send_clone_message(
            db,
            env,
            "validate_clone_compatibility",
            &format!("{pool_run_id}:{}", reference.id),
            Some(pool_run_id),
            ReferencePipelineMessage::ValidateCloneCompatibility {
                user_id: user_id.to_string(),
                clone_id: clone_id.to_string(),
                pool_run_id: pool_run_id.to_string(),
                global_reference_id: reference.id,
            },
            now,
        )
        .await?;
    }

    mark_pool_status(
        db,
        user_id,
        clone_id,
        pool_run_id,
        "compatibility_reviewing",
        &[],
        now,
    )
    .await
}

async fn repair_already_accepted_references(
    _db: &D1Database,
    _user_id: &str,
    _clone_id: &str,
    _pool_run_id: &str,
    _rows: &[GlobalReferenceActionableRow],
    _now: &str,
) -> WorkerResult<()> {
    Ok(())
}

async fn reserve_and_send_clone_message(
    db: &D1Database,
    env: &Env,
    message_kind: &str,
    dedupe_key: &str,
    pool_run_id: Option<&str>,
    message: ReferencePipelineMessage,
    now: &str,
) -> WorkerResult<()> {
    let reservation_id = format!("queue_reservation_{}", Uuid::new_v4().simple());
    let result = db::run(
        db,
        reserve_reference_pipeline_message_sql(),
        vec![
            json!(reservation_id),
            json!(REFERENCE_QUEUE_STORAGE_NAME),
            json!(message_kind),
            json!(dedupe_key),
            json!(pool_run_id),
            json!(now),
            json!(now),
            json!(add_minutes_iso(now, 45)),
        ],
    )
    .await?;

    if changed_rows(&result)? == 0 {
        return Ok(());
    }

    if let Err(error) = env.queue("REFERENCE_PIPELINE_QUEUE")?.send(message).await {
        mark_reserved_reference_pipeline_message_failed(
            db,
            message_kind,
            dedupe_key,
            &reservation_id,
            now,
        )
        .await?;
        return Err(error);
    }

    db::exec(
        db,
        mark_reserved_reference_pipeline_message_enqueued_sql(),
        vec![
            json!(now),
            json!(REFERENCE_QUEUE_STORAGE_NAME),
            json!(message_kind),
            json!(dedupe_key),
        ],
    )
    .await
}

async fn mark_reserved_reference_pipeline_message_failed(
    db: &D1Database,
    message_kind: &str,
    dedupe_key: &str,
    reservation_id: &str,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        mark_reserved_reference_pipeline_message_failed_sql(),
        vec![
            json!(now),
            json!(REFERENCE_QUEUE_STORAGE_NAME),
            json!(message_kind),
            json!(dedupe_key),
            json!(reservation_id),
        ],
    )
    .await
}

async fn mark_unclaimed_clone_pool_run_superseded(
    db: &D1Database,
    pool_run_id: &str,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        mark_unclaimed_clone_pool_run_superseded_sql(),
        vec![json!(now), json!(pool_run_id)],
    )
    .await
}

async fn enqueue_global_topups_for_underfilled_selected_slugs(
    db: &D1Database,
    env: &Env,
    selected_slugs: &[String],
    target: u32,
    reason: &str,
) -> WorkerResult<()> {
    for slug in selected_slugs {
        if active_global_reference_count(db, slug).await? < target {
            env.queue(REFERENCE_QUEUE_NAME)?
                .send(ReferencePipelineMessage::EnsureGlobalMoodboardLibrary {
                    moodboard_slug: slug.clone(),
                    reason: reason.to_string(),
                })
                .await?;
        }
    }
    Ok(())
}

async fn active_global_reference_count(db: &D1Database, moodboard_slug: &str) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        r#"
        SELECT COUNT(*) AS count
        FROM global_moodboard_references
        WHERE moodboard_slug = ?
          AND status = 'active'
        "#,
        vec![json!(moodboard_slug)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn mark_pool_waiting_for_global_library(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    selected_slugs: &[String],
    now: &str,
) -> WorkerResult<()> {
    mark_pool_status(
        db,
        user_id,
        clone_id,
        pool_run_id,
        "waiting_for_global_library",
        selected_slugs,
        now,
    )
    .await
}

async fn mark_pool_status(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    status: &str,
    waiting_slugs: &[String],
    now: &str,
) -> WorkerResult<()> {
    let waiting_slugs_json = json!(waiting_slugs).to_string();
    db::exec(
        db,
        update_pool_status_sql(),
        vec![
            json!(status),
            json!(waiting_slugs_json),
            json!(now),
            json!(pool_run_id),
            json!(user_id),
            json!(clone_id),
        ],
    )
    .await?;
    db::exec(
        db,
        update_clone_reference_state_status_sql(),
        vec![
            json!(status),
            json!(waiting_slugs_json),
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(pool_run_id),
        ],
    )
    .await
}

async fn load_pool_config(db: &D1Database) -> WorkerResult<PoolConfig> {
    Ok(PoolConfig {
        batch_size: config_value_u32(db, "batch_size", 5).await?,
        global_refs_per_moodboard_target: config_value_u32(
            db,
            "global_refs_per_moodboard_target",
            25,
        )
        .await?,
        global_refs_for_pool_min: config_value_u32(db, "global_refs_for_pool_min", 5).await?,
        clone_pool_run_stale_after_minutes: config_value_i64(
            db,
            "clone_pool_run_stale_after_minutes",
            30,
        )
        .await?,
        clone_pool_global_reference_review_limit: config_value_u32(
            db,
            "clone_pool_global_reference_review_limit",
            40,
        )
        .await? as usize,
        clone_pool_compatibility_wave_size: config_value_u32(
            db,
            "clone_pool_compatibility_wave_size",
            10,
        )
        .await? as usize,
    })
}

async fn config_value_u32(db: &D1Database, key: &str, fallback: u32) -> WorkerResult<u32> {
    let row = db::first::<ConfigRow>(
        db,
        "SELECT value FROM blitz_config WHERE key = ? LIMIT 1",
        vec![json!(key)],
    )
    .await?;
    Ok(row
        .and_then(|row| row.value.trim().parse::<u32>().ok())
        .unwrap_or(fallback))
}

async fn config_value_i64(db: &D1Database, key: &str, fallback: i64) -> WorkerResult<i64> {
    let row = db::first::<ConfigRow>(
        db,
        "SELECT value FROM blitz_config WHERE key = ? LIMIT 1",
        vec![json!(key)],
    )
    .await?;
    Ok(row
        .and_then(|row| row.value.trim().parse::<i64>().ok())
        .unwrap_or(fallback))
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

fn add_minutes_iso(now: &str, minutes: i64) -> String {
    let timestamp = js_sys::Date::parse(now);
    let date = js_sys::Date::new(&JsValue::from_f64(timestamp));
    date.set_time(date.get_time() + (minutes as f64 * 60_000.0));
    date.to_iso_string().into()
}

async fn enqueue_finalize_clone_pool(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    reason: &str,
    now: &str,
) -> WorkerResult<()> {
    reserve_and_send_clone_message(
        db,
        env,
        "finalize_clone_reference_pool",
        pool_run_id,
        Some(pool_run_id),
        ReferencePipelineMessage::FinalizeCloneReferencePool {
            user_id: user_id.to_string(),
            clone_id: clone_id.to_string(),
            pool_run_id: pool_run_id.to_string(),
            reason: reason.to_string(),
        },
        now,
    )
    .await
}

fn changed_rows(result: &worker::D1Result) -> WorkerResult<usize> {
    Ok(result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or_default())
}
