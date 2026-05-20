use crate::db;
use crate::queues::messages::ReferencePipelineMessage;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use wasm_bindgen::JsValue;
use worker::{D1Database, Env, Error, Result as WorkerResult};

const REFERENCE_PIPELINE_QUEUE_STORAGE_NAME: &str = "mirai-reference-pipeline";
const REFERENCE_PIPELINE_QUEUE_BINDING_NAME: &str = "REFERENCE_PIPELINE_QUEUE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservationOutcome {
    Reserved,
    SuppressedActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueHandlingOutcome {
    Claimed,
    AlreadyHandled,
    Terminal,
    Suppressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservationTtl {
    FiveMinutes,
    QueueDelivery,
    ReviewBatch,
    GlobalRun { stale_after_minutes: i64 },
    ClonePool { stale_after_minutes: i64 },
}

#[derive(Debug, Clone)]
pub struct QueueReservation {
    pub queue_name: String,
    pub message_kind: String,
    pub dedupe_key: String,
    pub run_id: Option<String>,
    pub pool_run_id: Option<String>,
    pub ttl: ReservationTtl,
}

impl QueueReservation {
    pub fn new(
        message_kind: impl Into<String>,
        dedupe_key: impl Into<String>,
        run_id: Option<String>,
        pool_run_id: Option<String>,
        ttl: ReservationTtl,
    ) -> Self {
        Self {
            queue_name: REFERENCE_PIPELINE_QUEUE_STORAGE_NAME.to_string(),
            message_kind: message_kind.into(),
            dedupe_key: dedupe_key.into(),
            run_id,
            pool_run_id,
            ttl,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ActiveReservationRow {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ReservationStatusRow {
    status: String,
}

fn expire_active_reservation_sql() -> &'static str {
    r#"
    UPDATE queue_message_reservations
    SET status = 'expired',
        updated_at = ?
    WHERE queue_name = ?
      AND message_kind = ?
      AND dedupe_key = ?
      AND status IN ('reserved', 'enqueued', 'handling', 'retrying')
      AND expires_at <= ?
    "#
}

fn reserve_queue_message_sql() -> &'static str {
    r#"
    INSERT INTO queue_message_reservations (
      id, queue_name, message_kind, dedupe_key, run_id, pool_run_id,
      status, created_at, updated_at, expires_at
    )
    VALUES (?, ?, ?, ?, ?, ?, 'reserved', ?, ?, ?)
    ON CONFLICT(queue_name, message_kind, dedupe_key) DO UPDATE SET
      id = excluded.id,
      run_id = excluded.run_id,
      pool_run_id = excluded.pool_run_id,
      status = 'reserved',
      updated_at = excluded.updated_at,
      expires_at = excluded.expires_at
    WHERE queue_message_reservations.status IN ('handled', 'failed', 'expired', 'cancelled')
       OR queue_message_reservations.expires_at <= excluded.created_at
    "#
}

fn load_active_reservation_sql() -> &'static str {
    r#"
    SELECT id
    FROM queue_message_reservations
    WHERE queue_name = ?
      AND message_kind = ?
      AND dedupe_key = ?
      AND status IN ('reserved', 'enqueued', 'handling', 'retrying')
      AND expires_at > ?
    LIMIT 1
    "#
}

fn claim_queue_message_handling_sql() -> &'static str {
    r#"
    UPDATE queue_message_reservations
    SET status = 'handling',
        updated_at = ?,
        expires_at = ?
    WHERE queue_name = ?
      AND message_kind = ?
      AND dedupe_key = ?
      AND status IN ('reserved', 'enqueued', 'retrying')
      AND expires_at > ?
    "#
}

fn load_reservation_status_sql() -> &'static str {
    r#"
    SELECT status
    FROM queue_message_reservations
    WHERE queue_name = ?
      AND message_kind = ?
      AND dedupe_key = ?
    LIMIT 1
    "#
}

fn insert_direct_handling_reservation_sql() -> &'static str {
    r#"
    INSERT INTO queue_message_reservations (
      id, queue_name, message_kind, dedupe_key, run_id, pool_run_id,
      status, created_at, updated_at, expires_at
    )
    VALUES (?, ?, ?, ?, ?, ?, 'handling', ?, ?, ?)
    ON CONFLICT(queue_name, message_kind, dedupe_key) DO UPDATE SET
      id = excluded.id,
      run_id = excluded.run_id,
      pool_run_id = excluded.pool_run_id,
      status = 'handling',
      updated_at = excluded.updated_at,
      expires_at = excluded.expires_at
    WHERE queue_message_reservations.status IN ('handled', 'failed', 'expired', 'cancelled')
       OR queue_message_reservations.expires_at <= excluded.created_at
    "#
}

fn mark_queue_message_enqueued_sql() -> &'static str {
    r#"
    UPDATE queue_message_reservations
    SET status = 'enqueued',
        updated_at = ?
    WHERE queue_name = ?
      AND message_kind = ?
      AND dedupe_key = ?
      AND status = 'reserved'
      AND expires_at > ?
    "#
}

fn mark_queue_message_handled_sql() -> &'static str {
    r#"
    UPDATE queue_message_reservations
    SET status = 'handled',
        updated_at = ?
    WHERE queue_name = ?
      AND message_kind = ?
      AND dedupe_key = ?
      AND status = 'handling'
      AND expires_at > ?
    "#
}

fn mark_queue_message_retrying_sql() -> &'static str {
    r#"
    UPDATE queue_message_reservations
    SET status = 'retrying',
        updated_at = ?
    WHERE queue_name = ?
      AND message_kind = ?
      AND dedupe_key = ?
      AND status = 'handling'
      AND expires_at > ?
    "#
}

fn mark_queue_message_failed_sql() -> &'static str {
    r#"
    UPDATE queue_message_reservations
    SET status = 'failed',
        updated_at = ?
    WHERE queue_name = ?
      AND message_kind = ?
      AND dedupe_key = ?
      AND status IN ('reserved', 'enqueued', 'handling', 'retrying')
      AND expires_at > ?
    "#
}

fn cancel_pool_reservations_sql() -> &'static str {
    r#"
    UPDATE queue_message_reservations
    SET status = 'cancelled',
        updated_at = ?
    WHERE queue_name = ?
      AND pool_run_id = ?
      AND status IN ('reserved', 'enqueued')
      AND expires_at > ?
    "#
}

pub fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

pub fn add_minutes_iso(now: &str, minutes: i64) -> String {
    let timestamp = js_sys::Date::parse(now);
    let date = js_sys::Date::new(&JsValue::from_f64(timestamp));
    date.set_time(date.get_time() + (minutes as f64 * 60_000.0));
    date.to_iso_string().into()
}

pub fn expires_at_for_ttl(now: &str, ttl: ReservationTtl) -> String {
    let minutes = match ttl {
        ReservationTtl::FiveMinutes => 5,
        ReservationTtl::QueueDelivery => 15,
        ReservationTtl::ReviewBatch => 60,
        ReservationTtl::GlobalRun {
            stale_after_minutes,
        } => stale_after_minutes + 15,
        ReservationTtl::ClonePool {
            stale_after_minutes,
        } => stale_after_minutes + 15,
    };
    add_minutes_iso(now, minutes)
}

pub async fn claim_queue_message_handling(
    db: &D1Database,
    reservation: &QueueReservation,
    now: &str,
) -> WorkerResult<QueueHandlingOutcome> {
    db::exec(
        db,
        expire_active_reservation_sql(),
        vec![
            json!(now),
            json!(reservation.queue_name),
            json!(reservation.message_kind),
            json!(reservation.dedupe_key),
            json!(now),
        ],
    )
    .await?;

    let claim_result = db::run(
        db,
        claim_queue_message_handling_sql(),
        vec![
            json!(now),
            json!(expires_at_for_ttl(now, reservation.ttl)),
            json!(reservation.queue_name),
            json!(reservation.message_kind),
            json!(reservation.dedupe_key),
            json!(now),
        ],
    )
    .await?;
    if changed_rows(&claim_result)? > 0 {
        return Ok(QueueHandlingOutcome::Claimed);
    }

    // Direct-sent messages from older producers have no reservation row. Create
    // or reclaim a handling row here so success/failure transitions are still
    // guarded and expired dedupe windows can be re-driven.
    let insert_result = db::run(
        db,
        insert_direct_handling_reservation_sql(),
        vec![
            json!(format!("queue_reservation_{}", Uuid::new_v4())),
            json!(reservation.queue_name),
            json!(reservation.message_kind),
            json!(reservation.dedupe_key),
            json!(reservation.run_id),
            json!(reservation.pool_run_id),
            json!(now),
            json!(now),
            json!(expires_at_for_ttl(now, reservation.ttl)),
        ],
    )
    .await?;
    if changed_rows(&insert_result)? > 0 {
        return Ok(QueueHandlingOutcome::Claimed);
    }

    Ok(load_reservation_status(db, reservation)
        .await?
        .map(|status| handling_outcome_for_status(&status))
        .unwrap_or(QueueHandlingOutcome::Suppressed))
}

async fn load_reservation_status(
    db: &D1Database,
    reservation: &QueueReservation,
) -> WorkerResult<Option<String>> {
    Ok(db::first::<ReservationStatusRow>(
        db,
        load_reservation_status_sql(),
        vec![
            json!(reservation.queue_name),
            json!(reservation.message_kind),
            json!(reservation.dedupe_key),
        ],
    )
    .await?
    .map(|row| row.status))
}

fn handling_outcome_for_status(status: &str) -> QueueHandlingOutcome {
    match status {
        "handled" => QueueHandlingOutcome::AlreadyHandled,
        "failed" | "expired" | "cancelled" => QueueHandlingOutcome::Terminal,
        _ => QueueHandlingOutcome::Suppressed,
    }
}

pub async fn reserve_queue_message(
    db: &D1Database,
    reservation: &QueueReservation,
    now: &str,
) -> WorkerResult<ReservationOutcome> {
    db::exec(
        db,
        expire_active_reservation_sql(),
        vec![
            json!(now),
            json!(reservation.queue_name),
            json!(reservation.message_kind),
            json!(reservation.dedupe_key),
            json!(now),
        ],
    )
    .await?;

    let reservation_id = format!("queue_reservation_{}", Uuid::new_v4());
    let expires_at = expires_at_for_ttl(now, reservation.ttl);
    db::exec(
        db,
        reserve_queue_message_sql(),
        vec![
            json!(reservation_id),
            json!(reservation.queue_name),
            json!(reservation.message_kind),
            json!(reservation.dedupe_key),
            json!(reservation.run_id),
            json!(reservation.pool_run_id),
            json!(now),
            json!(now),
            json!(expires_at),
        ],
    )
    .await?;

    let active = db::first::<ActiveReservationRow>(
        db,
        load_active_reservation_sql(),
        vec![
            json!(reservation.queue_name),
            json!(reservation.message_kind),
            json!(reservation.dedupe_key),
            json!(now),
        ],
    )
    .await?;

    if active.as_ref().map(|row| row.id.as_str()) == Some(reservation_id.as_str()) {
        Ok(ReservationOutcome::Reserved)
    } else {
        Ok(ReservationOutcome::SuppressedActive)
    }
}

pub async fn mark_queue_message_enqueued(
    db: &D1Database,
    reservation: &QueueReservation,
    now: &str,
) -> WorkerResult<bool> {
    mark_queue_message_status(db, reservation, mark_queue_message_enqueued_sql(), now).await
}

pub async fn mark_queue_message_handled(
    db: &D1Database,
    reservation: &QueueReservation,
    now: &str,
) -> WorkerResult<bool> {
    mark_queue_message_status(db, reservation, mark_queue_message_handled_sql(), now).await
}

pub async fn mark_queue_message_retrying(
    db: &D1Database,
    reservation: &QueueReservation,
    now: &str,
) -> WorkerResult<bool> {
    mark_queue_message_status(db, reservation, mark_queue_message_retrying_sql(), now).await
}

pub async fn mark_queue_message_failed(
    db: &D1Database,
    reservation: &QueueReservation,
    now: &str,
) -> WorkerResult<bool> {
    mark_queue_message_status(db, reservation, mark_queue_message_failed_sql(), now).await
}

async fn mark_queue_message_status(
    db: &D1Database,
    reservation: &QueueReservation,
    sql: &str,
    now: &str,
) -> WorkerResult<bool> {
    let result = db::run(
        db,
        sql,
        vec![
            json!(now),
            json!(reservation.queue_name),
            json!(reservation.message_kind),
            json!(reservation.dedupe_key),
            json!(now),
        ],
    )
    .await?;
    Ok(changed_rows(&result)? > 0)
}

pub async fn cancel_unstarted_pool_reservations(
    db: &D1Database,
    pool_run_id: &str,
    now: &str,
) -> WorkerResult<()> {
    db::exec(
        db,
        cancel_pool_reservations_sql(),
        vec![
            json!(now),
            json!(REFERENCE_PIPELINE_QUEUE_STORAGE_NAME),
            json!(pool_run_id),
            json!(now),
        ],
    )
    .await
}

pub async fn reserve_and_send_reference_pipeline_message(
    db: &D1Database,
    env: &Env,
    reservation: QueueReservation,
    message: ReferencePipelineMessage,
    now: &str,
) -> WorkerResult<ReservationOutcome> {
    let outcome = reserve_queue_message(db, &reservation, now).await?;
    if outcome == ReservationOutcome::Reserved {
        if let Err(error) = env
            .queue(REFERENCE_PIPELINE_QUEUE_BINDING_NAME)?
            .send(message)
            .await
        {
            let _ = mark_queue_message_failed(db, &reservation, now).await?;
            return Err(error);
        }
        if !mark_queue_message_enqueued(db, &reservation, now).await? {
            return Err(Error::RustError(
                "queue_reservation_enqueued_transition_failed".to_string(),
            ));
        }
    }
    Ok(outcome)
}

pub fn reservation_key_for_reference_message(
    message: &ReferencePipelineMessage,
    _global_run_stale_after_minutes: i64,
    _clone_pool_run_stale_after_minutes: i64,
) -> QueueReservation {
    match message {
        ReferencePipelineMessage::EnsureGlobalMoodboardLibrary { moodboard_slug, .. } => {
            QueueReservation::new(
                "ensure_global_moodboard_library",
                format!("global:ensure:{moodboard_slug}"),
                None,
                None,
                ReservationTtl::FiveMinutes,
            )
        }
        ReferencePipelineMessage::DiscoverGlobalInstagramHandles {
            moodboard_slug,
            run_id,
            search_term,
            date_window,
            page,
        } => QueueReservation::new(
            "discover_global_instagram_handles",
            format!(
                "global:{run_id}:discover:{}:{}:{}:{page}",
                stable_dedupe_segment(moodboard_slug),
                stable_dedupe_segment(search_term),
                stable_dedupe_segment(date_window)
            ),
            Some(run_id.clone()),
            None,
            ReservationTtl::QueueDelivery,
        ),
        ReferencePipelineMessage::FetchGlobalInstagramProfile {
            moodboard_slug,
            run_id,
            handle,
            related_depth,
            ..
        } => QueueReservation::new(
            "fetch_global_instagram_profile",
            format!(
                "global:{run_id}:profile:{}:{}:{related_depth}",
                stable_dedupe_segment(moodboard_slug),
                stable_dedupe_segment(handle)
            ),
            Some(run_id.clone()),
            None,
            ReservationTtl::QueueDelivery,
        ),
        ReferencePipelineMessage::FetchGlobalInstagramPosts {
            moodboard_slug,
            run_id,
            handle,
            next_max_id,
            page,
            ..
        } => QueueReservation::new(
            "fetch_global_instagram_posts",
            format!(
                "global:{run_id}:posts:{}:{}:{page}:{}",
                stable_dedupe_segment(moodboard_slug),
                stable_dedupe_segment(handle),
                stable_dedupe_segment(next_max_id.as_deref().unwrap_or(""))
            ),
            Some(run_id.clone()),
            None,
            ReservationTtl::QueueDelivery,
        ),
        ReferencePipelineMessage::FetchGlobalInstagramPostDetail {
            run_id, source_url, ..
        } => QueueReservation::new(
            "fetch_global_instagram_post_detail",
            format!(
                "global:{run_id}:post-detail:{}",
                stable_dedupe_segment(source_url)
            ),
            Some(run_id.clone()),
            None,
            ReservationTtl::QueueDelivery,
        ),
        ReferencePipelineMessage::ReviewGlobalVisualCandidates {
            moodboard_slug,
            run_id,
            ..
        } => QueueReservation::new(
            "review_global_visual_candidates",
            "global:<run_id>:review-batch:<moodboard_slug>"
                .replace("<run_id>", run_id)
                .replace("<moodboard_slug>", moodboard_slug),
            Some(run_id.clone()),
            None,
            ReservationTtl::ReviewBatch,
        ),
        ReferencePipelineMessage::CleanupGlobalMoodboardReference {
            run_id,
            candidate_id,
            ..
        } => QueueReservation::new(
            "cleanup_global_moodboard_reference",
            format!(
                "global:{run_id}:cleanup:{}",
                stable_dedupe_segment(candidate_id)
            ),
            Some(run_id.clone()),
            None,
            ReservationTtl::QueueDelivery,
        ),
        ReferencePipelineMessage::FinalizeGlobalMoodboardLibrary {
            moodboard_slug,
            run_id,
            ..
        } => QueueReservation::new(
            "finalize_global_moodboard_library",
            format!(
                "global:{run_id}:finalize:{}",
                stable_dedupe_segment(moodboard_slug)
            ),
            Some(run_id.clone()),
            None,
            ReservationTtl::QueueDelivery,
        ),
        ReferencePipelineMessage::BuildCloneReferencePool {
            user_id,
            clone_id,
            wakeup_moodboard_slug,
            ..
        } => {
            let dedupe_key = wakeup_moodboard_slug.as_ref().map_or_else(
                || {
                    format!(
                        "clone:kickoff:{}:{}",
                        stable_dedupe_segment(user_id),
                        stable_dedupe_segment(clone_id)
                    )
                },
                |slug| format!("clone:wakeup:{}:{}:{}", user_id, clone_id, slug),
            );
            QueueReservation::new(
                "build_clone_reference_pool",
                dedupe_key,
                None,
                None,
                ReservationTtl::FiveMinutes,
            )
        }
        ReferencePipelineMessage::RefreshPool {
            user_id, clone_id, ..
        } => QueueReservation::new(
            "refresh_clone_reference_pool",
            format!(
                "clone:refresh:{}:{}",
                stable_dedupe_segment(user_id),
                stable_dedupe_segment(clone_id)
            ),
            None,
            None,
            ReservationTtl::FiveMinutes,
        ),
        ReferencePipelineMessage::ValidateCloneCompatibility {
            pool_run_id,
            global_reference_id,
            ..
        } => QueueReservation::new(
            "validate_clone_compatibility",
            clone_compatibility_dedupe_key(pool_run_id, global_reference_id),
            None,
            Some(pool_run_id.clone()),
            ReservationTtl::QueueDelivery,
        ),
        ReferencePipelineMessage::FinalizeCloneReferencePool { pool_run_id, .. } => {
            QueueReservation::new(
                "finalize_clone_reference_pool",
                format!("clone:{pool_run_id}:finalize"),
                None,
                Some(pool_run_id.clone()),
                ReservationTtl::QueueDelivery,
            )
        }
    }
}

fn clone_compatibility_dedupe_key(pool_run_id: &str, global_reference_id: &str) -> String {
    // Existing clone pool producers reserve this as the message-kind scoped key:
    // "clone:<pool_run_id>:compat:<global_reference_id>"
    format!("{pool_run_id}:{global_reference_id}")
}

fn stable_dedupe_segment(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn changed_rows(result: &worker::D1Result) -> WorkerResult<usize> {
    Ok(result
        .meta()?
        .and_then(|meta| meta.changes)
        .unwrap_or_default())
}
