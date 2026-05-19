use crate::db;
use crate::domain::clone_reference_pool::{
    clone_inspiration_pool_id, clone_pool_run_is_reusable, clone_visual_reference_id,
    compatibility_action_for, select_balanced_compatibility_wave, CompatibilityAction,
    GlobalReferenceForClonePool,
};
use crate::domain::moodboards::selected_moodboard_hash;
use crate::queues::messages::ReferencePipelineMessage;
use crate::services::queue_reservations::{QueueReservation, ReservationOutcome, ReservationTtl};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use wasm_bindgen::JsValue;
use worker::{D1Database, Env, Error, Result as WorkerResult};

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
struct PoolRunGuardRow {
    id: String,
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

#[derive(Debug, Deserialize)]
struct CurrentGlobalTopupRow {
    current_run_id: Option<String>,
    state_status: String,
    next_retry_at: Option<String>,
    run_status: Option<String>,
    run_updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VisualReferenceIdRow {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AttemptCountRow {
    attempt_count: u32,
}

#[derive(Debug)]
struct CompatibilityClaim {
    attempt_count: u32,
    claim_expires_at: String,
}

#[derive(Debug, Deserialize)]
struct GlobalReferenceCompatibilityRow {
    storage_key: String,
    content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CloneReferenceImageRow {
    storage_key: String,
    content_type: Option<String>,
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

#[derive(Debug, Default)]
struct GlobalTopupSummary {
    active_or_started_run_slugs: Vec<String>,
    blocked_or_exhausted_slugs: Vec<String>,
    underfilled_slug_count: usize,
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

fn passive_insufficient_wakeup_pool_run_sql() -> &'static str {
    r#"
    SELECT cpr.id, cpr.status, cpr.selected_moodboard_hash, cpr.updated_at
    FROM clone_reference_state crs
    INNER JOIN clone_pool_runs cpr
      ON cpr.id = crs.current_pool_run_id
     AND cpr.clone_id = crs.clone_id
    INNER JOIN clone_pool_waiting_moodboards cpwm
      ON cpwm.user_id = crs.user_id
     AND cpwm.clone_id = crs.clone_id
     AND cpwm.pool_run_id = cpr.id
    WHERE crs.user_id = ?
      AND crs.clone_id = ?
      AND crs.current_pool_run_id = cpr.id
      AND crs.status = 'insufficient_refs'
      AND crs.selected_moodboard_hash = ?
      AND cpr.selected_moodboard_hash = ?
      AND cpr.status = 'insufficient_refs'
      AND cpwm.moodboard_slug = ?
      AND cpwm.status IN ('insufficient', 'resumed')
    LIMIT 1
    "#
}

fn revive_passive_insufficient_pool_run_for_wakeup_sql() -> &'static str {
    r#"
    UPDATE clone_pool_runs AS cpr
    SET status = 'queued',
        updated_at = ?
    WHERE cpr.id = ?
      AND cpr.user_id = ?
      AND cpr.clone_id = ?
      AND cpr.status = 'insufficient_refs'
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        INNER JOIN clone_pool_waiting_moodboards cpwm
          ON cpwm.user_id = crs.user_id
         AND cpwm.clone_id = crs.clone_id
         AND cpwm.pool_run_id = cpr.id
        WHERE crs.user_id = ?
          AND crs.clone_id = ?
          AND crs.current_pool_run_id = cpr.id
          AND crs.status = 'insufficient_refs'
          AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
          AND cpwm.moodboard_slug = ?
          AND cpwm.status IN ('insufficient', 'resumed')
        LIMIT 1
      )
    "#
}

fn clear_revived_passive_insufficient_pool_run_for_wakeup_sql() -> &'static str {
    r#"
    UPDATE clone_pool_runs AS cpr
    SET waiting_moodboard_slugs_json = '[]',
        completed_at = NULL,
        updated_at = ?
    WHERE cpr.id = ?
      AND cpr.user_id = ?
      AND cpr.clone_id = ?
      AND cpr.status = 'queued'
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        WHERE crs.user_id = ?
          AND crs.clone_id = ?
          AND crs.current_pool_run_id = cpr.id
          AND crs.status = 'queued'
          AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
        LIMIT 1
      )
    "#
}

fn revert_passive_insufficient_pool_run_for_wakeup_sql() -> &'static str {
    r#"
    UPDATE clone_pool_runs AS cpr
    SET status = 'insufficient_refs',
        updated_at = ?
    WHERE cpr.id = ?
      AND cpr.user_id = ?
      AND cpr.clone_id = ?
      AND cpr.status = 'queued'
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        WHERE crs.user_id = ?
          AND crs.clone_id = ?
          AND crs.current_pool_run_id = cpr.id
          AND crs.status = 'insufficient_refs'
          AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
        LIMIT 1
      )
    "#
}

fn revive_passive_insufficient_clone_reference_state_for_wakeup_sql() -> &'static str {
    r#"
    UPDATE clone_reference_state
    SET status = 'queued',
        waiting_moodboard_slugs_json = '[]',
        updated_at = ?
    WHERE user_id = ?
      AND clone_id = ?
      AND current_pool_run_id = ?
      AND status = 'insufficient_refs'
      AND EXISTS (
        SELECT 1
        FROM clone_pool_runs cpr
        INNER JOIN clone_pool_waiting_moodboards cpwm
          ON cpwm.user_id = clone_reference_state.user_id
         AND cpwm.clone_id = clone_reference_state.clone_id
         AND cpwm.pool_run_id = cpr.id
        WHERE cpr.id = clone_reference_state.current_pool_run_id
          AND cpr.user_id = clone_reference_state.user_id
          AND cpr.clone_id = clone_reference_state.clone_id
          AND cpr.status = 'queued'
          AND clone_reference_state.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
          AND cpwm.moodboard_slug = ?
          AND cpwm.status IN ('insufficient', 'resumed')
        LIMIT 1
      )
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

pub fn compatibility_actionable_global_reference_count_for_current_selection_sql() -> &'static str {
    r#"
    SELECT COUNT(*) AS count
    FROM global_moodboard_references gmr
    INNER JOIN moodboards mb
      ON mb.user_id = ?
     AND mb.slug = gmr.moodboard_slug
     AND mb.selected = 1
    INNER JOIN global_moodboard_definitions gmd
      ON gmd.slug = mb.slug
     AND gmd.status = 'active'
    LEFT JOIN clone_visual_reference_compatibility cvr
      ON cvr.clone_id = ?
     AND cvr.global_reference_id = gmr.id
    LEFT JOIN visual_references vr
      ON vr.clone_id = ?
     AND vr.global_reference_id = gmr.id
     AND vr.status = 'active'
    WHERE gmr.status = 'active'
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
    "#
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

fn update_clone_reference_state_status_sql() -> &'static str {
    r#"
    UPDATE clone_reference_state
    SET status = ?,
        waiting_moodboard_slugs_json = ?,
        updated_at = ?
    WHERE user_id = ?
      AND clone_id = ?
      AND current_pool_run_id = ?
      AND status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
      AND EXISTS (
        SELECT 1
        FROM clone_pool_runs cpr
        WHERE cpr.id = clone_reference_state.current_pool_run_id
          AND cpr.user_id = clone_reference_state.user_id
          AND cpr.clone_id = clone_reference_state.clone_id
          AND cpr.status = ?
          AND cpr.selected_moodboard_hash = clone_reference_state.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
        LIMIT 1
      )
    "#
}

fn insert_clone_pool_waiting_moodboard_sql() -> &'static str {
    r#"
    INSERT INTO clone_pool_waiting_moodboards (
      id, user_id, clone_id, pool_run_id, moodboard_slug,
      status, created_at, resolved_at
    )
    VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
    ON CONFLICT(pool_run_id, moodboard_slug) DO UPDATE SET
      status = excluded.status,
      resolved_at = NULL
    WHERE clone_pool_waiting_moodboards.status IN ('waiting', 'insufficient', 'resumed')
    -- status = 'waiting'
    -- status = 'insufficient'
    -- UNIQUE(pool_run_id, moodboard_slug)
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

fn current_clone_pool_run_guard_sql() -> &'static str {
    r#"
    SELECT cpr.id
    FROM clone_reference_state crs
    INNER JOIN clone_pool_runs cpr
      ON cpr.id = crs.current_pool_run_id
     AND cpr.clone_id = crs.clone_id
    WHERE crs.user_id = ?
      AND crs.clone_id = ?
      AND crs.current_pool_run_id = ?
      AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
      AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
      AND cpr.selected_moodboard_hash = ?
      AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
      AND cpr.updated_at > ?
    LIMIT 1
    "#
}

fn current_pool_run_allows_side_effects_sql() -> &'static str {
    r#"
    SELECT 1 AS count
    FROM clone_reference_state crs
    INNER JOIN clone_pool_runs cpr
      ON cpr.id = crs.current_pool_run_id
     AND cpr.clone_id = crs.clone_id
    WHERE crs.user_id = ?
      AND crs.clone_id = ?
      AND crs.current_pool_run_id = ?
      AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
      AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
      AND cpr.selected_moodboard_hash = ?
      AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
      AND cpr.updated_at > ?
    LIMIT 1
    "#
}

fn record_stale_clone_compatibility_attempt_sql() -> &'static str {
    r#"
    INSERT INTO clone_reference_compatibility_attempts (
      id, pool_run_id, clone_id, global_reference_id, status,
      error_code, error_message, created_at
    )
    VALUES (?, ?, ?, ?, 'stale_ignored',
      'stale_pool_run', 'stale clone pool message acknowledged without visible writes', ?)
    -- status = 'stale_ignored'
    "#
}

fn load_global_reference_for_compatibility_sql() -> &'static str {
    r#"
    SELECT gmr.id, gmr.media_asset_id, ma.storage_key, ma.content_type,
           gmr.moodboard_slug, gmr.image_width, gmr.image_height
    FROM global_moodboard_references gmr
    INNER JOIN media_assets ma
      ON ma.id = gmr.media_asset_id
     AND ma.user_id = 'global'
     AND ma.clone_id IS NULL
     AND ma.deleted_at IS NULL
     AND ma.storage_key IS NOT NULL
     AND TRIM(ma.storage_key) <> ''
    WHERE gmr.id = ?
      AND gmr.status = 'active'
    LIMIT 1
    "#
}

fn load_clone_reference_image_urls_sql() -> &'static str {
    r#"
    SELECT ma.storage_key, ma.content_type
    FROM clone_reference_assets cra
    INNER JOIN media_assets ma
      ON ma.id = cra.media_asset_id
     AND ma.deleted_at IS NULL
     AND ma.storage_key IS NOT NULL
     AND TRIM(ma.storage_key) <> ''
    WHERE cra.user_id = ?
      AND cra.clone_id = ?
      AND cra.training_selected = 1
      AND cra.eligibility_status = 'accepted'
    ORDER BY cra.sort_order ASC, cra.created_at ASC
    LIMIT ?
    "#
}

// clone_visual_reference_compatibility has UNIQUE(clone_id, global_reference_id).
fn insert_or_claim_clone_compatibility_sql() -> &'static str {
    r#"
    INSERT INTO clone_visual_reference_compatibility (
      id, clone_id, global_reference_id, status, attempt_count,
      last_attempted_at, created_at, updated_at
    )
    SELECT ?, ?, ?, 'queued', 0, NULL, ?, ?
    WHERE EXISTS (
      SELECT 1
      FROM clone_reference_state crs
      INNER JOIN clone_pool_runs cpr
        ON cpr.id = crs.current_pool_run_id
       AND cpr.clone_id = crs.clone_id
      WHERE crs.user_id = ?
        AND crs.clone_id = ?
        AND crs.current_pool_run_id = ?
        AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
        AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
        AND cpr.selected_moodboard_hash = ?
        AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
        AND cpr.updated_at > ?
      LIMIT 1
    )
    ON CONFLICT(clone_id, global_reference_id) DO UPDATE SET
      updated_at = excluded.updated_at
    WHERE (
        clone_visual_reference_compatibility.status = 'queued'
        OR (
          clone_visual_reference_compatibility.status = 'failed'
          AND clone_visual_reference_compatibility.attempt_count < ?
          AND clone_visual_reference_compatibility.next_retry_at <= ?
        )
      )
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        INNER JOIN clone_pool_runs cpr
          ON cpr.id = crs.current_pool_run_id
         AND cpr.clone_id = crs.clone_id
        WHERE crs.user_id = ?
          AND crs.clone_id = ?
          AND crs.current_pool_run_id = ?
          AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
          AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND cpr.updated_at > ?
        LIMIT 1
      )
    "#
}

fn insert_queued_clone_compatibility_sql() -> &'static str {
    r#"
    INSERT OR IGNORE INTO clone_visual_reference_compatibility (
      id, clone_id, global_reference_id, status, attempt_count,
      created_at, updated_at
    )
    SELECT ?, ?, ?, 'queued', 0, ?, ?
    WHERE EXISTS (
      SELECT 1
      FROM clone_reference_state crs
      INNER JOIN clone_pool_runs cpr
        ON cpr.id = crs.current_pool_run_id
       AND cpr.clone_id = crs.clone_id
      WHERE crs.user_id = ?
        AND crs.clone_id = ?
        AND crs.current_pool_run_id = ?
        AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
        AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
        AND cpr.selected_moodboard_hash = ?
        AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
        AND cpr.updated_at > ?
      LIMIT 1
    )
    "#
}

fn increment_clone_compatibility_attempt_sql() -> &'static str {
    r#"
    UPDATE clone_visual_reference_compatibility
    SET status = 'failed',
        attempt_count = attempt_count + 1,
        last_attempted_at = ?,
        next_retry_at = ?,
        updated_at = ?
    WHERE clone_id = ?
      AND global_reference_id = ?
      AND attempt_count < ?
      AND (
        status = 'queued'
        OR status = 'failed'
          AND next_retry_at <= ?
      )
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        INNER JOIN clone_pool_runs cpr
          ON cpr.id = crs.current_pool_run_id
         AND cpr.clone_id = crs.clone_id
        WHERE crs.user_id = ?
          AND crs.clone_id = ?
          AND crs.current_pool_run_id = ?
          AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
          AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND cpr.updated_at > ?
        LIMIT 1
      )
    "#
}

fn load_claimed_clone_compatibility_attempt_sql() -> &'static str {
    r#"
    SELECT attempt_count
    FROM clone_visual_reference_compatibility
    WHERE clone_id = ?
      AND global_reference_id = ?
      AND status = 'failed'
      AND next_retry_at = ?
    LIMIT 1
    "#
}

fn mark_clone_compatibility_accepted_sql() -> &'static str {
    r#"
    UPDATE clone_visual_reference_compatibility
    SET status = 'accepted',
        body_proportions_compatible = ?,
        hair_length_compatible = ?,
        facial_hair_compatible = ?,
        review_json = ?,
        last_error_code = NULL,
        last_error_message = NULL,
        next_retry_at = NULL,
        accepted_at = ?,
        updated_at = ?
    WHERE clone_id = ?
      AND global_reference_id = ?
      AND attempt_count = ?
      AND next_retry_at = ?
      AND status = 'failed'
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        INNER JOIN clone_pool_runs cpr
          ON cpr.id = crs.current_pool_run_id
         AND cpr.clone_id = crs.clone_id
        WHERE crs.user_id = ?
          AND crs.clone_id = clone_visual_reference_compatibility.clone_id
          AND crs.current_pool_run_id = ?
          AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
          AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND cpr.updated_at > ?
      )
    "#
}

fn mark_clone_compatibility_rejected_sql() -> &'static str {
    r#"
    UPDATE clone_visual_reference_compatibility
    SET status = 'rejected',
        body_proportions_compatible = ?,
        hair_length_compatible = ?,
        facial_hair_compatible = ?,
        review_json = ?,
        last_error_code = NULL,
        last_error_message = NULL,
        next_retry_at = NULL,
        rejected_at = ?,
        updated_at = ?
    WHERE clone_id = ?
      AND global_reference_id = ?
      AND attempt_count = ?
      AND next_retry_at = ?
      AND status = 'failed'
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        INNER JOIN clone_pool_runs cpr
          ON cpr.id = crs.current_pool_run_id
         AND cpr.clone_id = crs.clone_id
        WHERE crs.user_id = ?
          AND crs.clone_id = clone_visual_reference_compatibility.clone_id
          AND crs.current_pool_run_id = ?
          AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
          AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND cpr.updated_at > ?
      )
    "#
}

fn mark_clone_compatibility_failed_sql() -> &'static str {
    r#"
    UPDATE clone_visual_reference_compatibility
    SET status = 'failed',
        last_error_code = ?,
        last_error_message = ?,
        next_retry_at = CASE WHEN attempt_count >= ? THEN NULL ELSE ? END,
        updated_at = ?
    WHERE clone_id = ?
      AND global_reference_id = ?
      AND attempt_count = ?
      AND next_retry_at = ?
      AND status = 'failed'
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        INNER JOIN clone_pool_runs cpr
          ON cpr.id = crs.current_pool_run_id
         AND cpr.clone_id = crs.clone_id
        WHERE crs.user_id = ?
          AND crs.clone_id = clone_visual_reference_compatibility.clone_id
          AND crs.current_pool_run_id = ?
          AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
          AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND cpr.updated_at > ?
      )
    "#
}

// visual_references has UNIQUE(clone_id, global_reference_id).
fn insert_clone_visual_reference_sql() -> &'static str {
    r#"
    INSERT INTO visual_references (
      id, user_id, clone_id, global_reference_id, media_asset_id,
      source_platform, source_image_key, source_handle, source_post_id,
      source_post_code, source_url, source_published_at, image_width,
      image_height, moodboard_id, moodboard_slug, niche_cluster,
      human_presence_type, human_presence_score, organic_photo_score,
      freshness_visual_score, visual_fit_score, pose, scene, lighting,
      framing, camera_feel, styling_direction, aesthetic_tags_json,
      source_caption_removed, status, created_at, updated_at
    )
    SELECT
      ?, ?, ?, gmr.id, gmr.media_asset_id,
      gmr.source_platform, gmr.source_image_key, gmr.source_handle,
      gmr.source_post_id, gmr.source_post_code, gmr.source_url,
      gmr.source_published_at, gmr.image_width, gmr.image_height,
      mb.id, gmr.moodboard_slug, gmr.moodboard_slug,
      'person', 1, 1, 1, gmr.moodboard_fit_score,
      gmr.pose, gmr.scene, gmr.lighting, gmr.framing, gmr.camera_feel,
      gmr.styling_direction,
      json_array(
        gmr.pose,
        gmr.scene,
        gmr.lighting,
        gmr.framing,
        gmr.camera_feel,
        gmr.styling_direction
      ),
      1, 'active', ?, ?
    FROM global_moodboard_references gmr
    INNER JOIN media_assets ma
      ON ma.id = gmr.media_asset_id
     AND ma.user_id = 'global'
     AND ma.clone_id IS NULL
     AND ma.deleted_at IS NULL
    INNER JOIN clone_visual_reference_compatibility cvr
      ON cvr.clone_id = ?
     AND cvr.global_reference_id = gmr.id
     AND cvr.status = 'accepted'
    INNER JOIN moodboards mb
      ON mb.user_id = ?
     AND mb.slug = gmr.moodboard_slug
     AND mb.selected = 1
    INNER JOIN global_moodboard_definitions gmd
      ON gmd.slug = mb.slug
     AND gmd.status = 'active'
    WHERE gmr.id = ?
      AND gmr.status = 'active'
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        INNER JOIN clone_pool_runs cpr
          ON cpr.id = crs.current_pool_run_id
         AND cpr.clone_id = crs.clone_id
        WHERE crs.user_id = ?
          AND crs.clone_id = ?
          AND crs.current_pool_run_id = ?
          AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
          AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND cpr.updated_at > ?
        LIMIT 1
      )
    ON CONFLICT(clone_id, global_reference_id) DO UPDATE SET
      user_id = excluded.user_id,
      media_asset_id = excluded.media_asset_id,
      source_platform = excluded.source_platform,
      source_image_key = excluded.source_image_key,
      source_handle = excluded.source_handle,
      source_post_id = excluded.source_post_id,
      source_post_code = excluded.source_post_code,
      source_url = excluded.source_url,
      source_published_at = excluded.source_published_at,
      image_width = excluded.image_width,
      image_height = excluded.image_height,
      moodboard_id = excluded.moodboard_id,
      moodboard_slug = excluded.moodboard_slug,
      niche_cluster = excluded.niche_cluster,
      human_presence_type = excluded.human_presence_type,
      human_presence_score = excluded.human_presence_score,
      organic_photo_score = excluded.organic_photo_score,
      freshness_visual_score = excluded.freshness_visual_score,
      visual_fit_score = excluded.visual_fit_score,
      pose = excluded.pose,
      scene = excluded.scene,
      lighting = excluded.lighting,
      framing = excluded.framing,
      camera_feel = excluded.camera_feel,
      styling_direction = excluded.styling_direction,
      aesthetic_tags_json = excluded.aesthetic_tags_json,
      source_caption_removed = excluded.source_caption_removed,
      status = 'active',
      updated_at = excluded.updated_at
    "#
}

fn active_clone_visual_reference_for_accepted_global_reference_sql() -> &'static str {
    r#"
    SELECT vr.id
    FROM visual_references vr
    INNER JOIN global_moodboard_references gmr
      ON gmr.id = vr.global_reference_id
     AND gmr.status = 'active'
     AND gmr.media_asset_id = vr.media_asset_id
    INNER JOIN media_assets ma
      ON ma.id = gmr.media_asset_id
     AND ma.user_id = 'global'
     AND ma.clone_id IS NULL
     AND ma.deleted_at IS NULL
    INNER JOIN clone_visual_reference_compatibility cvr
      ON cvr.clone_id = vr.clone_id
     AND cvr.global_reference_id = gmr.id
     AND cvr.status = 'accepted'
    INNER JOIN moodboards mb
      ON mb.user_id = vr.user_id
     AND mb.slug = vr.moodboard_slug
     AND mb.slug = gmr.moodboard_slug
     AND mb.selected = 1
    INNER JOIN global_moodboard_definitions gmd
      ON gmd.slug = mb.slug
     AND gmd.status = 'active'
    WHERE vr.user_id = ?
      AND vr.clone_id = ?
      AND vr.global_reference_id = ?
      AND vr.status = 'active'
    LIMIT 1
    "#
}

fn insert_clone_inspiration_pool_sql() -> &'static str {
    r#"
    INSERT OR IGNORE INTO user_inspiration_pool (
      id, user_id, clone_id, moodboard_id, visual_reference_id,
      discovery_item_id, score, created_at
    )
    SELECT ?, vr.user_id, vr.clone_id, vr.moodboard_id, vr.id, NULL, 1, ?
    FROM visual_references vr
    WHERE vr.user_id = ?
      AND vr.clone_id = ?
      AND vr.id = ?
      AND vr.status = 'active'
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        INNER JOIN clone_pool_runs cpr
          ON cpr.id = crs.current_pool_run_id
         AND cpr.clone_id = crs.clone_id
        WHERE crs.user_id = ?
          AND crs.clone_id = ?
          AND crs.current_pool_run_id = ?
          AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND crs.selected_moodboard_hash = cpr.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
          AND cpr.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND cpr.updated_at > ?
        LIMIT 1
      )
    "#
}

fn active_clone_reference_count_for_current_selection_sql() -> &'static str {
    r#"
    SELECT COUNT(*) AS count
    FROM visual_references vr
    INNER JOIN moodboards mb
      ON mb.user_id = vr.user_id
     AND mb.slug = vr.moodboard_slug
     AND mb.selected = 1
    INNER JOIN global_moodboard_definitions gmd
      ON gmd.slug = mb.slug
     AND gmd.status = 'active'
    INNER JOIN global_moodboard_references gmr
      ON gmr.id = vr.global_reference_id
     AND gmr.status = 'active'
     AND gmr.media_asset_id = vr.media_asset_id
     AND gmr.moodboard_slug = vr.moodboard_slug
    INNER JOIN media_assets ma
      ON ma.id = gmr.media_asset_id
     AND ma.user_id = 'global'
     AND ma.clone_id IS NULL
     AND ma.deleted_at IS NULL
    INNER JOIN clone_visual_reference_compatibility cvr
      ON cvr.clone_id = vr.clone_id
     AND cvr.global_reference_id = vr.global_reference_id
     AND cvr.status = 'accepted'
    WHERE vr.user_id = ?
      AND vr.clone_id = ?
      AND vr.status = 'active'
    "#
}

fn pending_clone_compatibility_count_for_current_selection_sql() -> &'static str {
    r#"
    SELECT COUNT(*) AS count
    FROM clone_visual_reference_compatibility cvr
    INNER JOIN global_moodboard_references gmr
      ON gmr.id = cvr.global_reference_id
     AND gmr.status = 'active'
    INNER JOIN media_assets ma
      ON ma.id = gmr.media_asset_id
     AND ma.user_id = 'global'
     AND ma.clone_id IS NULL
     AND ma.deleted_at IS NULL
    INNER JOIN moodboards mb
      ON mb.user_id = ?
     AND mb.slug = gmr.moodboard_slug
     AND mb.selected = 1
    INNER JOIN global_moodboard_definitions gmd
      ON gmd.slug = mb.slug
     AND gmd.status = 'active'
    WHERE cvr.clone_id = ?
      AND (
        cvr.status = 'queued'
        OR (
          cvr.status = 'failed'
          AND cvr.next_retry_at IS NOT NULL
          AND cvr.next_retry_at <= ?
        )
      )
    "#
}

fn finalize_clone_pool_run_sql() -> &'static str {
    r#"
    UPDATE clone_pool_runs
    SET status = ?,
        reason = ?,
        waiting_moodboard_slugs_json = ?,
        updated_at = ?,
        completed_at = CASE
          WHEN ? IN ('pool_ready', 'partial_pool_ready', 'insufficient_refs') THEN ?
          ELSE completed_at
        END
    WHERE id = ?
      AND user_id = ?
      AND clone_id = ?
      AND status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        WHERE crs.user_id = clone_pool_runs.user_id
          AND crs.clone_id = clone_pool_runs.clone_id
          AND crs.current_pool_run_id = clone_pool_runs.id
          AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND crs.selected_moodboard_hash = clone_pool_runs.selected_moodboard_hash
          AND clone_pool_runs.selected_moodboard_hash = ?
      )
    "#
}

fn finalize_clone_reference_state_sql() -> &'static str {
    r#"
    UPDATE clone_reference_state
    SET status = ?,
        waiting_moodboard_slugs_json = CASE
          WHEN ? = 'insufficient_refs' THEN ?
          ELSE '[]'
        END,
        last_usable_pool_at = CASE
          WHEN ? IN ('pool_ready', 'partial_pool_ready') THEN ?
          ELSE last_usable_pool_at
        END,
        last_ready_at = CASE
          WHEN ? = 'pool_ready' THEN ?
          ELSE last_ready_at
        END,
        last_partial_ready_at = CASE
          WHEN ? = 'partial_pool_ready' THEN ?
          ELSE last_partial_ready_at
        END,
        last_insufficient_at = CASE
          WHEN ? = 'insufficient_refs' THEN ?
          ELSE last_insufficient_at
        END,
        updated_at = ?
    WHERE user_id = ?
      AND clone_id = ?
      AND current_pool_run_id = ?
      AND status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
      AND EXISTS (
        SELECT 1
        FROM clone_pool_runs cpr
        WHERE cpr.id = clone_reference_state.current_pool_run_id
          AND cpr.user_id = clone_reference_state.user_id
          AND cpr.clone_id = clone_reference_state.clone_id
          AND cpr.status = ?
          AND cpr.selected_moodboard_hash = clone_reference_state.selected_moodboard_hash
          AND cpr.selected_moodboard_hash = ?
        LIMIT 1
      )
    "#
}

fn update_clone_pool_run_status_if_current_sql() -> &'static str {
    r#"
    UPDATE clone_pool_runs
    SET status = ?,
        waiting_moodboard_slugs_json = ?,
        updated_at = ?,
        completed_at = CASE WHEN ? IN ('pool_ready', 'partial_pool_ready', 'insufficient_refs', 'pool_failed') THEN ? ELSE completed_at END
    WHERE id = ?
      AND user_id = ?
      AND clone_id = ?
      AND status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
      AND clone_pool_runs.selected_moodboard_hash = ?
      AND EXISTS (
        SELECT 1
        FROM clone_reference_state crs
        WHERE crs.user_id = clone_pool_runs.user_id
          AND crs.clone_id = clone_pool_runs.clone_id
          AND crs.current_pool_run_id = clone_pool_runs.id
          AND crs.status IN ('queued', 'waiting_for_global_library', 'compatibility_reviewing')
          AND crs.selected_moodboard_hash = clone_pool_runs.selected_moodboard_hash
      )
    "#
}

fn insert_compatibility_attempt_audit_sql() -> &'static str {
    r#"
    INSERT INTO clone_reference_compatibility_attempts (
      id, pool_run_id, clone_id, global_reference_id, status,
      error_code, error_message, created_at
    )
    VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    "#
}

fn current_global_topup_state_sql() -> &'static str {
    r#"
    SELECT
      gmrs.current_run_id,
      gmrs.status AS state_status,
      gmrs.next_retry_at,
      gsr.status AS run_status,
      gsr.updated_at AS run_updated_at
    FROM global_moodboard_reference_state gmrs
    LEFT JOIN global_moodboard_source_runs gsr
      ON gsr.id = gmrs.current_run_id
     AND gsr.moodboard_slug = gmrs.moodboard_slug
    WHERE gmrs.moodboard_slug = ?
    LIMIT 1
    "#
}

fn eligible_global_topup_work_sql() -> &'static str {
    r#"
    SELECT
      (
        SELECT COUNT(*)
        FROM global_moodboard_search_state
        WHERE moodboard_slug = ?
          AND status IN ('active', 'cooldown')
          AND (next_eligible_at IS NULL OR next_eligible_at <= ?)
      ) + (
        SELECT COUNT(*)
        FROM global_moodboard_handles
        WHERE moodboard_slug = ?
          AND status IN ('active', 'cooldown')
          AND (cooldown_until IS NULL OR cooldown_until <= ?)
      ) + (
        SELECT COUNT(*)
        FROM global_visual_reference_candidates
        WHERE (discovery_moodboard_slug = ? OR assigned_moodboard_slug = ?)
          AND candidate_status = 'active'
          AND (
            review_status = 'queued'
            OR (
              review_status = 'reviewing'
              AND review_locked_until IS NOT NULL
              AND review_locked_until <= ?
              AND review_attempt_count < ?
            )
            OR (
              review_status = 'failed'
              AND review_attempt_count < ?
              AND (review_next_retry_at IS NULL OR review_next_retry_at <= ?)
            )
            OR (
              review_status = 'approved'
              AND cleanup_status = 'queued'
            )
            OR (
              review_status = 'approved'
              AND cleanup_status = 'cleaning'
              AND (cleanup_next_retry_at IS NULL OR cleanup_next_retry_at <= ?)
            )
            OR (
              review_status = 'approved'
              AND cleanup_status = 'failed'
              AND cleanup_attempt_count < ?
              AND (cleanup_next_retry_at IS NULL OR cleanup_next_retry_at <= ?)
            )
          )
      ) AS count
    "#
}

pub async fn build_or_refresh_clone_pool(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    reason: &str,
    wakeup_moodboard_slug: Option<&str>,
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
    let _ = config.batch_size;
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
    let stale_cutoff = add_minutes_iso(&now, -config.clone_pool_run_stale_after_minutes.max(1));

    let pool_run_id =
        match reusable_pool_run(
            db,
            user_id,
            clone_id,
            &selected_hash,
            &now,
            &config,
            wakeup_moodboard_slug,
        )
        .await?
        {
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

    let topup_summary = enqueue_global_topups_for_underfilled_selected_slugs(
        db,
        env,
        &selected_slugs,
        config.global_refs_per_moodboard_target,
        "clone_pool_topup",
        &now,
    )
    .await?;
    let _ = (
        topup_summary.underfilled_slug_count,
        topup_summary.blocked_or_exhausted_slugs.len(),
    );

    let actionable = load_actionable_global_references(
        db,
        clone_id,
        &selected_slugs,
        config.clone_pool_global_reference_review_limit,
        &now,
    )
    .await?;

    if actionable.len() < config.global_refs_for_pool_min as usize
        && !topup_summary.active_or_started_run_slugs.is_empty()
    {
        if mark_pool_waiting_for_global_library(
            db,
            user_id,
            clone_id,
            &pool_run_id,
            &selected_hash,
            &topup_summary.active_or_started_run_slugs,
            &now,
        )
        .await?
        {
            write_clone_pool_waiting_rows(
                db,
                user_id,
                clone_id,
                &pool_run_id,
                &topup_summary.active_or_started_run_slugs,
                "waiting",
                &now,
            )
            .await?;
        }
        return Ok(());
    }

    if actionable.is_empty() {
        if mark_pool_status(
            db,
            user_id,
            clone_id,
            &pool_run_id,
            "insufficient_refs",
            &selected_hash,
            &selected_slugs,
            &now,
        )
        .await?
        {
            write_clone_pool_waiting_rows(
                db,
                user_id,
                clone_id,
                &pool_run_id,
                &selected_slugs,
                "insufficient",
                &now,
            )
            .await?;
        }
        return Ok(());
    }

    repair_already_accepted_references(
        db,
        user_id,
        clone_id,
        &pool_run_id,
        &selected_hash,
        &actionable,
        &now,
        &stale_cutoff,
    )
    .await?;
    schedule_compatibility_wave(
        db,
        env,
        user_id,
        clone_id,
        &pool_run_id,
        &selected_hash,
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

pub async fn validate_clone_compatibility(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    global_reference_id: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let stale_after_minutes =
        config_value_i64(db, "clone_pool_run_stale_after_minutes", 30).await?;
    let selected = db::all::<SelectedMoodboardRow>(
        db,
        load_current_selected_moodboard_snapshot_sql(),
        vec![json!(user_id)],
    )
    .await?;
    if selected.is_empty() {
        return Ok(());
    }
    let current_selected_slugs = selected
        .iter()
        .map(|row| row.slug.clone())
        .collect::<Vec<_>>();
    let current_selected_hash = selected_moodboard_hash(&current_selected_slugs);
    if !current_clone_pool_run_or_record_stale(
        db,
        user_id,
        clone_id,
        pool_run_id,
        &current_selected_hash,
        Some(global_reference_id),
        &now,
        stale_after_minutes,
    )
    .await?
    {
        must_not_mutate_clone_visible_state_from_stale_pool();
        return Ok(());
    }
    let stale_cutoff =
        crate::services::queue_reservations::add_minutes_iso(&now, -stale_after_minutes.max(1));
    if !current_pool_run_allows_side_effects(
        db,
        user_id,
        clone_id,
        pool_run_id,
        &current_selected_hash,
        &stale_cutoff,
    )
    .await?
    {
        insert_compatibility_attempt_audit(
            db,
            pool_run_id,
            clone_id,
            global_reference_id,
            "stale_pool_message",
            Some("stale_pool_message"),
            Some("Pool run is no longer current for this clone."),
            &now,
        )
        .await?;
        return Ok(());
    }

    let retry_limit = config_value_u32(db, "visual_reference_compatibility_retry_limit", 2).await?;
    let clone_reference_limit =
        config_value_u32(db, "clone_compatibility_reference_limit", 4).await?;
    let Some(compatibility_claim) = claim_clone_compatibility(
        db,
        user_id,
        clone_id,
        pool_run_id,
        &current_selected_hash,
        global_reference_id,
        retry_limit,
        &now,
        &stale_cutoff,
    )
    .await?
    else {
        if !current_clone_pool_run_or_record_stale(
            db,
            user_id,
            clone_id,
            pool_run_id,
            &current_selected_hash,
            Some(global_reference_id),
            &now,
            stale_after_minutes,
        )
        .await?
        {
            must_not_mutate_clone_visible_state_from_stale_pool();
            return Ok(());
        }
        return enqueue_finalize_clone_pool(
            db,
            env,
            user_id,
            clone_id,
            pool_run_id,
            "clone_compatibility_claim_skipped",
            &now,
        )
        .await;
    };

    if !current_pool_run_allows_side_effects(
        db,
        user_id,
        clone_id,
        pool_run_id,
        &current_selected_hash,
        &stale_cutoff,
    )
    .await?
    {
        return Ok(());
    }

    let Some(global_reference) =
        load_global_reference_for_compatibility(db, global_reference_id).await?
    else {
        if !current_pool_run_allows_side_effects(
            db,
            user_id,
            clone_id,
            pool_run_id,
            &current_selected_hash,
            &stale_cutoff,
        )
        .await?
        {
            return Ok(());
        }
        if mark_clone_compatibility_failed(
            db,
            user_id,
            clone_id,
            pool_run_id,
            &current_selected_hash,
            global_reference_id,
            &compatibility_claim,
            retry_limit,
            "global_reference_unavailable",
            "Global reference was not active or had no global media asset.",
            &now,
            &stale_cutoff,
        )
        .await?
        {
            insert_compatibility_attempt_audit(
                db,
                pool_run_id,
                clone_id,
                global_reference_id,
                "failed",
                Some("global_reference_unavailable"),
                Some("Global reference was not active or had no global media asset."),
                &now,
            )
            .await?;
        }
        return enqueue_finalize_clone_pool(
            db,
            env,
            user_id,
            clone_id,
            pool_run_id,
            "global_reference_unavailable",
            &now,
        )
        .await;
    };

    let image_urls = match compatibility_image_urls(
        db,
        env,
        user_id,
        clone_id,
        &global_reference.storage_key,
        global_reference.content_type.as_deref(),
        clone_reference_limit,
    )
    .await
    {
        Ok(image_urls) => image_urls,
        Err(error) => {
            let detail = compact_error_detail(&error.to_string());
            let code = compatibility_image_error_code(&detail);
            if !current_pool_run_allows_side_effects(
                db,
                user_id,
                clone_id,
                pool_run_id,
                &current_selected_hash,
                &stale_cutoff,
            )
            .await?
            {
                return Ok(());
            }
            if mark_clone_compatibility_failed(
                db,
                user_id,
                clone_id,
                pool_run_id,
                &current_selected_hash,
                global_reference_id,
                &compatibility_claim,
                retry_limit,
                code,
                &detail,
                &now,
                &stale_cutoff,
            )
            .await?
            {
                insert_compatibility_attempt_audit(
                    db,
                    pool_run_id,
                    clone_id,
                    global_reference_id,
                    "failed",
                    Some(code),
                    Some(&detail),
                    &now,
                )
                .await?;
            }
            return enqueue_finalize_clone_pool(
                db,
                env,
                user_id,
                clone_id,
                pool_run_id,
                "compatibility_image_load_failed",
                &now,
            )
            .await;
        }
    };
    if image_urls.len() <= 1 {
        if !current_pool_run_allows_side_effects(
            db,
            user_id,
            clone_id,
            pool_run_id,
            &current_selected_hash,
            &stale_cutoff,
        )
        .await?
        {
            return Ok(());
        }
        if mark_clone_compatibility_failed(
            db,
            user_id,
            clone_id,
            pool_run_id,
            &current_selected_hash,
            global_reference_id,
            &compatibility_claim,
            retry_limit,
            "clone_compatibility_reference_missing",
            "No clone reference images were available.",
            &now,
            &stale_cutoff,
        )
        .await?
        {
            insert_compatibility_attempt_audit(
                db,
                pool_run_id,
                clone_id,
                global_reference_id,
                "failed",
                Some("clone_compatibility_reference_missing"),
                Some("No clone reference images were available."),
                &now,
            )
            .await?;
        }
        return enqueue_finalize_clone_pool(
            db,
            env,
            user_id,
            clone_id,
            pool_run_id,
            "clone_reference_missing",
            &now,
        )
        .await;
    }

    let prompt =
        crate::ai::workers_ai::clone_compatibility_prompt(image_urls.len().saturating_sub(1));
    let review = match env.ai("AI") {
        Ok(ai) => {
            crate::ai::workers_ai::run_multi_vision_json::<
                crate::ai::workers_ai::CloneCompatibilityReview,
            >(&ai, &prompt, &image_urls)
            .await
        }
        Err(error) => Err(error),
    };

    let write_now = now_iso_string();
    let write_stale_cutoff = crate::services::queue_reservations::add_minutes_iso(
        &write_now,
        -stale_after_minutes.max(1),
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
    let current_selected_slugs = selected
        .iter()
        .map(|row| row.slug.clone())
        .collect::<Vec<_>>();
    let current_selected_hash = selected_moodboard_hash(&current_selected_slugs);
    if !current_pool_run_allows_side_effects(
        db,
        user_id,
        clone_id,
        pool_run_id,
        &current_selected_hash,
        &write_stale_cutoff,
    )
    .await?
    {
        insert_compatibility_attempt_audit(
            db,
            pool_run_id,
            clone_id,
            global_reference_id,
            "stale_pool_message",
            Some("stale_pool_message"),
            Some("Pool run became stale before compatibility write."),
            &write_now,
        )
        .await?;
        return Ok(());
    }

    match review {
        Ok(review) => {
            let review_json = serde_json::to_string(&review).unwrap_or_else(|_| "{}".to_string());
            match crate::domain::visual_reference::accept_clone_compatibility(&review) {
                Ok(()) => {
                    if mark_clone_compatibility_accepted(
                        db,
                        user_id,
                        clone_id,
                        pool_run_id,
                        &current_selected_hash,
                        global_reference_id,
                        &compatibility_claim,
                        &review,
                        &review_json,
                        &write_now,
                        &write_stale_cutoff,
                    )
                    .await?
                    {
                        let _ = insert_clone_visual_reference_for_accepted_global_reference(
                            db,
                            user_id,
                            clone_id,
                            pool_run_id,
                            &current_selected_hash,
                            global_reference_id,
                            &write_now,
                            &write_stale_cutoff,
                        )
                        .await?;
                        insert_compatibility_attempt_audit(
                            db,
                            pool_run_id,
                            clone_id,
                            global_reference_id,
                            "accepted",
                            None,
                            None,
                            &write_now,
                        )
                        .await?;
                    }
                }
                Err(reason) => {
                    if mark_clone_compatibility_rejected(
                        db,
                        user_id,
                        clone_id,
                        pool_run_id,
                        &current_selected_hash,
                        global_reference_id,
                        &compatibility_claim,
                        &review,
                        &review_json,
                        &write_now,
                        &write_stale_cutoff,
                    )
                    .await?
                    {
                        insert_compatibility_attempt_audit(
                            db,
                            pool_run_id,
                            clone_id,
                            global_reference_id,
                            "rejected",
                            Some(reason),
                            Some(reason),
                            &write_now,
                        )
                        .await?;
                    }
                }
            }
        }
        Err(error) => {
            let detail = compact_error_detail(&error.to_string());
            if mark_clone_compatibility_failed(
                db,
                user_id,
                clone_id,
                pool_run_id,
                &current_selected_hash,
                global_reference_id,
                &compatibility_claim,
                retry_limit,
                "provider_error",
                &detail,
                &write_now,
                &write_stale_cutoff,
            )
            .await?
            {
                insert_compatibility_attempt_audit(
                    db,
                    pool_run_id,
                    clone_id,
                    global_reference_id,
                    "failed",
                    Some("provider_error"),
                    Some(&detail),
                    &write_now,
                )
                .await?;
            }
        }
    }

    enqueue_finalize_clone_pool(
        db,
        env,
        user_id,
        clone_id,
        pool_run_id,
        "compatibility_result",
        &write_now,
    )
    .await
}

pub async fn insert_clone_visual_reference_for_accepted_global_reference(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    current_selected_hash: &str,
    global_reference_id: &str,
    now: &str,
    stale_cutoff: &str,
) -> WorkerResult<Option<String>> {
    if !current_pool_run_allows_side_effects(
        db,
        user_id,
        clone_id,
        pool_run_id,
        current_selected_hash,
        stale_cutoff,
    )
    .await?
    {
        return Ok(None);
    }

    let deterministic_visual_reference_id =
        clone_visual_reference_id(clone_id, global_reference_id);
    db::run(
        db,
        insert_clone_visual_reference_sql(),
        vec![
            json!(deterministic_visual_reference_id),
            json!(user_id),
            json!(clone_id),
            json!(now),
            json!(now),
            json!(clone_id),
            json!(user_id),
            json!(global_reference_id),
            json!(user_id),
            json!(clone_id),
            json!(pool_run_id),
            json!(current_selected_hash),
            json!(stale_cutoff),
        ],
    )
    .await?;

    let Some(visual_reference) = db::first::<VisualReferenceIdRow>(
        db,
        active_clone_visual_reference_for_accepted_global_reference_sql(),
        vec![json!(user_id), json!(clone_id), json!(global_reference_id)],
    )
    .await?
    else {
        return Ok(None);
    };

    let pool_id = clone_inspiration_pool_id(clone_id, &visual_reference.id);
    db::run(
        db,
        insert_clone_inspiration_pool_sql(),
        vec![
            json!(pool_id),
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(visual_reference.id.clone()),
            json!(user_id),
            json!(clone_id),
            json!(pool_run_id),
            json!(current_selected_hash),
            json!(stale_cutoff),
        ],
    )
    .await?;

    Ok(Some(visual_reference.id))
}

pub async fn finalize_clone_reference_pool(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    reason: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let config = load_pool_config(db).await?;
    let stale_cutoff = add_minutes_iso(&now, -config.clone_pool_run_stale_after_minutes.max(1));
    let selected = db::all::<SelectedMoodboardRow>(
        db,
        load_current_selected_moodboard_snapshot_sql(),
        vec![json!(user_id)],
    )
    .await?;
    if selected.is_empty() {
        return Ok(());
    }

    let selected_slugs = selected
        .iter()
        .map(|row| row.slug.clone())
        .collect::<Vec<_>>();
    let selected_hash = selected_moodboard_hash(&selected_slugs);
    if !current_clone_pool_run_or_record_stale(
        db,
        user_id,
        clone_id,
        pool_run_id,
        &selected_hash,
        None,
        &now,
        config.clone_pool_run_stale_after_minutes,
    )
    .await?
    {
        must_not_mutate_clone_visible_state_from_stale_pool();
        return Ok(());
    }
    let Some(run) = db::first::<PoolRunRow>(
        db,
        load_current_pool_run_sql(),
        vec![json!(user_id), json!(clone_id)],
    )
    .await?
    else {
        return Ok(());
    };
    if run.id != pool_run_id {
        return Ok(());
    }

    if !clone_pool_run_is_reusable(
        &run.status,
        run.selected_moodboard_hash == selected_hash,
        run.updated_at.as_deref(),
        &now,
        config.clone_pool_run_stale_after_minutes,
    ) {
        return Ok(());
    }

    let active_selected_count =
        active_clone_reference_count_for_current_selection(db, user_id, clone_id).await?;
    let pending_compatibility_count =
        pending_clone_compatibility_count_for_current_selection(db, user_id, clone_id, &now)
            .await?;

    if active_selected_count < config.batch_size && pending_compatibility_count == 0 {
        let actionable = load_actionable_global_references(
            db,
            clone_id,
            &selected_slugs,
            config.clone_pool_global_reference_review_limit,
            &now,
        )
        .await?;
        if !actionable.is_empty() {
            repair_already_accepted_references(
                db,
                user_id,
                clone_id,
                pool_run_id,
                &selected_hash,
                &actionable,
                &now,
                &stale_cutoff,
            )
            .await?;
            schedule_compatibility_wave(
                db,
                env,
                user_id,
                clone_id,
                pool_run_id,
                &selected_hash,
                &selected_slugs,
                actionable,
                &config,
                &now,
            )
            .await?;
            return enqueue_finalize_clone_pool(
                db,
                env,
                user_id,
                clone_id,
                pool_run_id,
                "wave_scheduled",
                &now,
            )
            .await;
        }
    }

    let final_status = if active_selected_count >= config.batch_size {
        crate::services::queue_reservations::cancel_unstarted_pool_reservations(
            db,
            pool_run_id,
            &now,
        )
        .await?;
        "pool_ready"
    } else if pending_compatibility_count > 0 {
        "compatibility_reviewing"
    } else if active_selected_count > 0 {
        "partial_pool_ready"
    } else {
        "insufficient_refs"
    };

    let final_waiting_slugs: &[String] = if final_status == "insufficient_refs" {
        selected_slugs.as_slice()
    } else {
        &[]
    };
    let final_waiting_slugs_json = json!(final_waiting_slugs).to_string();

    let result = db::run(
        db,
        finalize_clone_pool_run_sql(),
        vec![
            json!(final_status),
            json!(reason),
            json!(final_waiting_slugs_json.clone()),
            json!(now),
            json!(final_status),
            json!(now),
            json!(pool_run_id),
            json!(user_id),
            json!(clone_id),
            json!(selected_hash),
        ],
    )
    .await?;
    if changed_rows(&result)? == 0 {
        return Ok(());
    }

    let state_result = db::run(
        db,
        finalize_clone_reference_state_sql(),
        vec![
            json!(final_status),
            json!(final_status),
            json!(final_waiting_slugs_json),
            json!(final_status),
            json!(now),
            json!(final_status),
            json!(now),
            json!(final_status),
            json!(now),
            json!(final_status),
            json!(now),
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(pool_run_id),
            json!(final_status),
            json!(selected_hash),
        ],
    )
    .await?;
    if changed_rows(&state_result)? == 0 {
        return Ok(());
    }

    if final_status == "insufficient_refs" {
        write_clone_pool_waiting_rows(
            db,
            user_id,
            clone_id,
            pool_run_id,
            &selected_slugs,
            "insufficient",
            &now,
        )
        .await?;
    }

    if final_status == "pool_ready" || final_status == "partial_pool_ready" {
        if let Some(provider_soul_id) = db::first::<CloneForPoolRow>(
            db,
            load_clone_for_pool_sql(),
            vec![json!(user_id), json!(clone_id)],
        )
        .await?
        .and_then(|clone| {
            if clone.soul_status.as_deref() == Some("ready") {
                clone
                    .provider_soul_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            } else {
                None
            }
        }) {
            crate::services::blitz::create_next_batch(
                db,
                env,
                user_id,
                clone_id,
                &provider_soul_id,
            )
            .await?;
        }
    }

    Ok(())
}

async fn reusable_pool_run(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    selected_hash: &str,
    now: &str,
    config: &PoolConfig,
    wakeup_moodboard_slug: Option<&str>,
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
    } else if let Some(wakeup_moodboard_slug) = wakeup_moodboard_slug {
        passive_insufficient_wakeup_pool_run(
            db,
            user_id,
            clone_id,
            selected_hash,
            wakeup_moodboard_slug,
            now,
        )
        .await
    } else {
        Ok(None)
    }
}

async fn passive_insufficient_wakeup_pool_run(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    selected_hash: &str,
    wakeup_moodboard_slug: &str,
    now: &str,
) -> WorkerResult<Option<String>> {
    let Some(row) = db::first::<PoolRunRow>(
        db,
        passive_insufficient_wakeup_pool_run_sql(),
        vec![
            json!(user_id),
            json!(clone_id),
            json!(selected_hash),
            json!(selected_hash),
            json!(wakeup_moodboard_slug),
        ],
    )
    .await?
    else {
        return Ok(None);
    };

    let result = db::run(
        db,
        revive_passive_insufficient_pool_run_for_wakeup_sql(),
        vec![
            json!(now),
            json!(row.id.clone()),
            json!(user_id),
            json!(clone_id),
            json!(user_id),
            json!(clone_id),
            json!(selected_hash),
            json!(wakeup_moodboard_slug),
        ],
    )
    .await?;
    if changed_rows(&result)? == 0 {
        return Ok(None);
    }

    let state_result = db::run(
        db,
        revive_passive_insufficient_clone_reference_state_for_wakeup_sql(),
        vec![
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(row.id.clone()),
            json!(selected_hash),
            json!(wakeup_moodboard_slug),
        ],
    )
    .await?;
    if changed_rows(&state_result)? == 0 {
        let _ = db::run(
            db,
            revert_passive_insufficient_pool_run_for_wakeup_sql(),
            vec![
                json!(now),
                json!(row.id.clone()),
                json!(user_id),
                json!(clone_id),
                json!(user_id),
                json!(clone_id),
                json!(selected_hash),
            ],
        )
        .await?;
        return Ok(None);
    }

    let clear_result = db::run(
        db,
        clear_revived_passive_insufficient_pool_run_for_wakeup_sql(),
        vec![
            json!(now),
            json!(row.id.clone()),
            json!(user_id),
            json!(clone_id),
            json!(user_id),
            json!(clone_id),
            json!(selected_hash),
        ],
    )
    .await?;
    if changed_rows(&clear_result)? == 0 {
        return Ok(None);
    }

    Ok(Some(row.id))
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
    current_selected_hash: &str,
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
    let stale_cutoff = add_minutes_iso(now, -config.clone_pool_run_stale_after_minutes.max(1));

    for reference in selected {
        db::exec(
            db,
            insert_queued_clone_compatibility_sql(),
            vec![
                json!(format!("clone_compatibility_{}", Uuid::new_v4().simple())),
                json!(clone_id),
                json!(reference.id.clone()),
                json!(now),
                json!(now),
                json!(user_id),
                json!(clone_id),
                json!(pool_run_id),
                json!(current_selected_hash),
                json!(stale_cutoff),
            ],
        )
        .await?;
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

    let _ = mark_pool_status(
        db,
        user_id,
        clone_id,
        pool_run_id,
        "compatibility_reviewing",
        current_selected_hash,
        &[],
        now,
    )
    .await?;
    Ok(())
}

async fn repair_already_accepted_references(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    current_selected_hash: &str,
    rows: &[GlobalReferenceActionableRow],
    now: &str,
    stale_cutoff: &str,
) -> WorkerResult<()> {
    for row in rows {
        if compatibility_action_for(
            row.compatibility_status.as_deref(),
            row.next_retry_at.as_deref(),
            row.visual_reference_id.is_some(),
            now,
        ) == CompatibilityAction::RepairMissingVisualReference
        {
            let _ = insert_clone_visual_reference_for_accepted_global_reference(
                db,
                user_id,
                clone_id,
                pool_run_id,
                current_selected_hash,
                &row.id,
                now,
                stale_cutoff,
            )
            .await?;
        }
    }

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

async fn current_pool_run_allows_side_effects(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    current_selected_hash: &str,
    stale_cutoff: &str,
) -> WorkerResult<bool> {
    let row = db::first::<CountRow>(
        db,
        current_pool_run_allows_side_effects_sql(),
        vec![
            json!(user_id),
            json!(clone_id),
            json!(pool_run_id),
            json!(current_selected_hash),
            json!(stale_cutoff),
        ],
    )
    .await?;
    Ok(row.map(|row| row.count > 0).unwrap_or_default())
}

async fn current_clone_pool_run_or_record_stale(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    current_selected_hash: &str,
    global_reference_id: Option<&str>,
    now: &str,
    stale_after_minutes: i64,
) -> WorkerResult<bool> {
    let stale_cutoff =
        crate::services::queue_reservations::add_minutes_iso(now, -stale_after_minutes.max(1));
    let row = db::first::<PoolRunGuardRow>(
        db,
        current_clone_pool_run_guard_sql(),
        vec![
            json!(user_id),
            json!(clone_id),
            json!(pool_run_id),
            json!(current_selected_hash),
            json!(stale_cutoff),
        ],
    )
    .await?;
    if let Some(row) = row {
        let _ = row.id;
        return Ok(true);
    }

    if let Some(global_reference_id) = global_reference_id {
        db::exec(
            db,
            record_stale_clone_compatibility_attempt_sql(),
            vec![
                json!(format!("clone_compat_attempt_{}", Uuid::new_v4().simple())),
                json!(pool_run_id),
                json!(clone_id),
                json!(global_reference_id),
                json!(now),
            ],
        )
        .await?;
    }

    Ok(false)
}

fn must_not_mutate_clone_visible_state_from_stale_pool() {}

async fn load_global_reference_for_compatibility(
    db: &D1Database,
    global_reference_id: &str,
) -> WorkerResult<Option<GlobalReferenceCompatibilityRow>> {
    db::first(
        db,
        load_global_reference_for_compatibility_sql(),
        vec![json!(global_reference_id)],
    )
    .await
}

async fn claim_clone_compatibility(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    current_selected_hash: &str,
    global_reference_id: &str,
    retry_limit: u32,
    now: &str,
    stale_cutoff: &str,
) -> WorkerResult<Option<CompatibilityClaim>> {
    let compatibility_id = format!("clone_compatibility_{}", Uuid::new_v4().simple());
    let claim_expires_at = add_minutes_iso(now, 10);
    db::exec(
        db,
        insert_or_claim_clone_compatibility_sql(),
        vec![
            json!(compatibility_id),
            json!(clone_id),
            json!(global_reference_id),
            json!(now),
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(pool_run_id),
            json!(current_selected_hash),
            json!(stale_cutoff),
            json!(retry_limit.max(1)),
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(pool_run_id),
            json!(current_selected_hash),
            json!(stale_cutoff),
        ],
    )
    .await?;
    let result = db::run(
        db,
        increment_clone_compatibility_attempt_sql(),
        vec![
            json!(now),
            json!(claim_expires_at),
            json!(now),
            json!(clone_id),
            json!(global_reference_id),
            json!(retry_limit.max(1)),
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(pool_run_id),
            json!(current_selected_hash),
            json!(stale_cutoff),
        ],
    )
    .await?;
    if changed_rows(&result)? == 0 {
        return Ok(None);
    }

    Ok(db::first::<AttemptCountRow>(
        db,
        load_claimed_clone_compatibility_attempt_sql(),
        vec![
            json!(clone_id),
            json!(global_reference_id),
            json!(claim_expires_at.clone()),
        ],
    )
    .await?
    .map(|row| CompatibilityClaim {
        attempt_count: row.attempt_count,
        claim_expires_at,
    }))
}

async fn compatibility_image_urls(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    global_storage_key: &str,
    global_content_type: Option<&str>,
    clone_reference_limit: u32,
) -> WorkerResult<Vec<String>> {
    let mut image_urls =
        vec![media_storage_data_url(env, global_storage_key, global_content_type).await?];

    let rows = db::all::<CloneReferenceImageRow>(
        db,
        load_clone_reference_image_urls_sql(),
        vec![
            json!(user_id),
            json!(clone_id),
            json!(clone_reference_limit),
        ],
    )
    .await?;
    for row in rows {
        image_urls.push(
            media_storage_data_url(env, &row.storage_key, row.content_type.as_deref()).await?,
        );
    }

    Ok(image_urls)
}

async fn media_storage_data_url(
    env: &Env,
    storage_key: &str,
    content_type: Option<&str>,
) -> WorkerResult<String> {
    let object = env
        .bucket("MEDIA")?
        .get(storage_key.to_string())
        .execute()
        .await?
        .ok_or_else(|| Error::RustError("compatibility_media_missing".to_string()))?;
    let body = object
        .body()
        .ok_or_else(|| Error::RustError("compatibility_media_body_missing".to_string()))?;
    let bytes = body.bytes().await?;
    let content_type = normalized_image_content_type(content_type);
    Ok(format!(
        "data:{};base64,{}",
        content_type,
        BASE64_STANDARD.encode(bytes)
    ))
}

fn normalized_image_content_type(content_type: Option<&str>) -> String {
    content_type
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .trim()
        .to_string()
}

fn compatibility_image_error_code(error: &str) -> &'static str {
    if error.contains("compatibility_media_missing")
        || error.contains("compatibility_media_body_missing")
    {
        "compatibility_media_missing"
    } else {
        "compatibility_image_load_failed"
    }
}

async fn mark_clone_compatibility_accepted(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    current_selected_hash: &str,
    global_reference_id: &str,
    claim: &CompatibilityClaim,
    review: &crate::ai::workers_ai::CloneCompatibilityReview,
    review_json: &str,
    now: &str,
    stale_cutoff: &str,
) -> WorkerResult<bool> {
    let result = db::run(
        db,
        mark_clone_compatibility_accepted_sql(),
        vec![
            json!(review.body_proportions_compatible),
            json!(review.hair_length_compatible),
            json!(review.facial_hair_compatible),
            json!(review_json),
            json!(now),
            json!(now),
            json!(clone_id),
            json!(global_reference_id),
            json!(claim.attempt_count),
            json!(claim.claim_expires_at),
            json!(user_id),
            json!(pool_run_id),
            json!(current_selected_hash),
            json!(stale_cutoff),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn mark_clone_compatibility_rejected(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    current_selected_hash: &str,
    global_reference_id: &str,
    claim: &CompatibilityClaim,
    review: &crate::ai::workers_ai::CloneCompatibilityReview,
    review_json: &str,
    now: &str,
    stale_cutoff: &str,
) -> WorkerResult<bool> {
    let result = db::run(
        db,
        mark_clone_compatibility_rejected_sql(),
        vec![
            json!(review.body_proportions_compatible),
            json!(review.hair_length_compatible),
            json!(review.facial_hair_compatible),
            json!(review_json),
            json!(now),
            json!(now),
            json!(clone_id),
            json!(global_reference_id),
            json!(claim.attempt_count),
            json!(claim.claim_expires_at),
            json!(user_id),
            json!(pool_run_id),
            json!(current_selected_hash),
            json!(stale_cutoff),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn mark_clone_compatibility_failed(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    current_selected_hash: &str,
    global_reference_id: &str,
    claim: &CompatibilityClaim,
    retry_limit: u32,
    error_code: &str,
    error_message: &str,
    now: &str,
    stale_cutoff: &str,
) -> WorkerResult<bool> {
    let result = db::run(
        db,
        mark_clone_compatibility_failed_sql(),
        vec![
            json!(error_code),
            json!(compact_error_detail(error_message)),
            json!(retry_limit.max(1)),
            json!(add_minutes_iso(now, 10)),
            json!(now),
            json!(clone_id),
            json!(global_reference_id),
            json!(claim.attempt_count),
            json!(claim.claim_expires_at),
            json!(user_id),
            json!(pool_run_id),
            json!(current_selected_hash),
            json!(stale_cutoff),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

async fn insert_compatibility_attempt_audit(
    db: &D1Database,
    pool_run_id: &str,
    clone_id: &str,
    global_reference_id: &str,
    status: &str,
    error_code: Option<&str>,
    error_message: Option<&str>,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        insert_compatibility_attempt_audit_sql(),
        vec![
            json!(format!("clone_compat_attempt_{}", Uuid::new_v4().simple())),
            json!(pool_run_id),
            json!(clone_id),
            json!(global_reference_id),
            json!(status),
            json!(error_code),
            json!(error_message.map(compact_error_detail)),
            json!(now),
        ],
    )
    .await
}

async fn enqueue_global_topups_for_underfilled_selected_slugs(
    db: &D1Database,
    env: &Env,
    selected_slugs: &[String],
    target: u32,
    reason: &str,
    now: &str,
) -> WorkerResult<GlobalTopupSummary> {
    let mut summary = GlobalTopupSummary::default();
    let stale_after_minutes = config_value_i64(
        db,
        crate::services::global_reference_discovery::GLOBAL_DISCOVERY_RUN_STALE_AFTER_MINUTES_CONFIG_KEY,
        60,
    )
    .await?
    .max(1);
    let stale_cutoff =
        crate::services::queue_reservations::add_minutes_iso(now, -stale_after_minutes);
    let review_retry_limit = config_value_u32(db, "visual_reference_review_retry_limit", 2)
        .await?
        .max(1);
    let cleanup_retry_limit = config_value_u32(db, "visual_reference_cleanup_retry_limit", 3)
        .await?
        .max(1);

    for slug in selected_slugs {
        if active_global_reference_count(db, slug).await? >= target {
            continue;
        }
        summary.underfilled_slug_count += 1;

        if current_global_topup_run_is_active(db, slug, &stale_cutoff).await? {
            summary.active_or_started_run_slugs.push(slug.clone());
            continue;
        }

        let eligible_global_work_exists = eligible_global_topup_work_exists(
            db,
            slug,
            now,
            review_retry_limit,
            cleanup_retry_limit,
        )
        .await?;

        if !eligible_global_work_exists && global_topup_next_retry_is_blocked(db, slug, now).await?
        {
            summary.blocked_or_exhausted_slugs.push(slug.clone());
            continue;
        }

        if current_global_topup_state_exists(db, slug).await? && !eligible_global_work_exists
        {
            summary.blocked_or_exhausted_slugs.push(slug.clone());
            continue;
        }

        let reservation = QueueReservation::new(
            "ensure_global_moodboard_library",
            format!("global:ensure:{slug}"),
            None,
            None,
            ReservationTtl::FiveMinutes,
        );
        let outcome =
            crate::services::queue_reservations::reserve_and_send_reference_pipeline_message(
                db,
                env,
                reservation,
                ReferencePipelineMessage::EnsureGlobalMoodboardLibrary {
                    moodboard_slug: slug.clone(),
                    reason: reason.to_string(),
                },
                now,
            )
            .await?;
        if matches!(
            outcome,
            ReservationOutcome::Reserved | ReservationOutcome::SuppressedActive
        ) {
            summary.active_or_started_run_slugs.push(slug.clone());
        }
    }
    Ok(summary)
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

async fn current_global_topup_state(
    db: &D1Database,
    moodboard_slug: &str,
) -> WorkerResult<Option<CurrentGlobalTopupRow>> {
    db::first(
        db,
        current_global_topup_state_sql(),
        vec![json!(moodboard_slug)],
    )
    .await
}

async fn current_global_topup_state_exists(
    db: &D1Database,
    moodboard_slug: &str,
) -> WorkerResult<bool> {
    Ok(current_global_topup_state(db, moodboard_slug)
        .await?
        .is_some())
}

async fn current_global_topup_run_is_active(
    db: &D1Database,
    moodboard_slug: &str,
    stale_cutoff: &str,
) -> WorkerResult<bool> {
    let Some(row) = current_global_topup_state(db, moodboard_slug).await? else {
        return Ok(false);
    };
    let _ = row.current_run_id.as_deref();
    let run_status_is_active = row
        .run_status
        .as_deref()
        .map(|status| matches!(status, "queued" | "refreshing" | "scraping" | "reviewing" | "cleaning"))
        .unwrap_or(false);
    let fresh = row
        .run_updated_at
        .as_deref()
        .map(|updated_at| updated_at > stale_cutoff)
        .unwrap_or(false);
    Ok(run_status_is_active && fresh)
}

async fn global_topup_next_retry_is_blocked(
    db: &D1Database,
    moodboard_slug: &str,
    now: &str,
) -> WorkerResult<bool> {
    let Some(row) = current_global_topup_state(db, moodboard_slug).await? else {
        return Ok(false);
    };
    let terminal_underfill = matches!(
        row.state_status.as_str(),
        "insufficient_refs" | "underfilled_exhausted" | "discovery_failed"
    );
    Ok(terminal_underfill
        && row
            .next_retry_at
            .as_deref()
            .map(|next_retry_at| next_retry_at > now)
            .unwrap_or(false))
}

async fn eligible_global_topup_work_exists(
    db: &D1Database,
    moodboard_slug: &str,
    now: &str,
    review_retry_limit: u32,
    cleanup_retry_limit: u32,
) -> WorkerResult<bool> {
    let row = db::first::<CountRow>(
        db,
        eligible_global_topup_work_sql(),
        vec![
            json!(moodboard_slug),
            json!(now),
            json!(moodboard_slug),
            json!(now),
            json!(moodboard_slug),
            json!(moodboard_slug),
            json!(now),
            json!(review_retry_limit),
            json!(review_retry_limit),
            json!(now),
            json!(now),
            json!(cleanup_retry_limit),
            json!(now),
        ],
    )
    .await?;
    Ok(row.map(|row| row.count > 0).unwrap_or(false))
}

async fn active_clone_reference_count_for_current_selection(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        active_clone_reference_count_for_current_selection_sql(),
        vec![json!(user_id), json!(clone_id)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn pending_clone_compatibility_count_for_current_selection(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    now: &str,
) -> WorkerResult<u32> {
    let row = db::first::<CountRow>(
        db,
        pending_clone_compatibility_count_for_current_selection_sql(),
        vec![json!(user_id), json!(clone_id), json!(now)],
    )
    .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

async fn write_clone_pool_waiting_rows(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    selected_slugs: &[String],
    status: &str,
    now: &str,
) -> WorkerResult<()> {
    for slug in selected_slugs {
        db::exec(
            db,
            insert_clone_pool_waiting_moodboard_sql(),
            vec![
                json!(format!("clone_pool_waiting_{}", Uuid::new_v4().simple())),
                json!(user_id),
                json!(clone_id),
                json!(pool_run_id),
                json!(slug),
                json!(status),
                json!(now),
            ],
        )
        .await?;
    }
    Ok(())
}

async fn mark_pool_waiting_for_global_library(
    db: &D1Database,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    current_selected_hash: &str,
    selected_slugs: &[String],
    now: &str,
) -> WorkerResult<bool> {
    mark_pool_status(
        db,
        user_id,
        clone_id,
        pool_run_id,
        "waiting_for_global_library",
        current_selected_hash,
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
    current_selected_hash: &str,
    waiting_slugs: &[String],
    now: &str,
) -> WorkerResult<bool> {
    let waiting_slugs_json = json!(waiting_slugs).to_string();
    let result = db::run(
        db,
        update_clone_pool_run_status_if_current_sql(),
        vec![
            json!(status),
            json!(waiting_slugs_json),
            json!(now),
            json!(status),
            json!(now),
            json!(pool_run_id),
            json!(user_id),
            json!(clone_id),
            json!(current_selected_hash),
        ],
    )
    .await?;
    if changed_rows(&result)? == 0 {
        return Ok(false);
    }
    let state_result = db::run(
        db,
        update_clone_reference_state_status_sql(),
        vec![
            json!(status),
            json!(waiting_slugs_json),
            json!(now),
            json!(user_id),
            json!(clone_id),
            json!(pool_run_id),
            json!(status),
            json!(current_selected_hash),
        ],
    )
    .await?;
    if changed_rows(&state_result)? == 0 {
        return Ok(false);
    }
    Ok(true)
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

fn compact_error_detail(error: &str) -> String {
    const MAX_ERROR_DETAIL_CHARS: usize = 500;
    let compact = error.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(MAX_ERROR_DETAIL_CHARS).collect()
}

async fn enqueue_finalize_clone_pool(
    _db: &D1Database,
    env: &Env,
    user_id: &str,
    clone_id: &str,
    pool_run_id: &str,
    reason: &str,
    _now: &str,
) -> WorkerResult<()> {
    env.queue(REFERENCE_QUEUE_NAME)?
        .send(ReferencePipelineMessage::FinalizeCloneReferencePool {
            user_id: user_id.to_string(),
            clone_id: clone_id.to_string(),
            pool_run_id: pool_run_id.to_string(),
            reason: reason.to_string(),
        })
        .await
}

fn changed_rows(result: &worker::D1Result) -> WorkerResult<usize> {
    Ok(result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or_default())
}
