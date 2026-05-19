use crate::db;
use crate::domain::moodboards::default_moodboards;
use crate::queues::messages::ReferencePipelineMessage;
use crate::services::queue_reservations::{
    add_minutes_iso, now_iso_string, reserve_and_send_reference_pipeline_message, QueueReservation,
    ReservationTtl,
};
use serde::Deserialize;
use serde_json::json;
use worker::{D1Database, Env, Result as WorkerResult};

#[derive(Debug, Deserialize)]
struct DueGlobalMoodboardRow {
    moodboard_slug: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct ConfigRow {
    value: String,
}

fn scheduler_due_global_moodboard_libraries_sql() -> &'static str {
    r#"
    SELECT
      gmd.slug AS moodboard_slug,
      CASE
        WHEN gmrs.moodboard_slug IS NULL THEN 'scheduler_missing_state'
        WHEN gmrs.active_reference_count < gmrs.target_reference_count THEN 'scheduler_under_target'
        WHEN gmrs.last_successful_refresh_at IS NULL THEN 'scheduler_never_refreshed'
        ELSE 'scheduler_stale_library'
      END AS reason
    FROM global_moodboard_definitions gmd
    LEFT JOIN global_moodboard_reference_state gmrs
      ON gmrs.moodboard_slug = gmd.slug
    WHERE gmd.status = 'active'
      AND (
        gmrs.moodboard_slug IS NULL
        OR gmrs.active_reference_count < gmrs.target_reference_count
        OR gmrs.last_successful_refresh_at IS NULL
        OR gmrs.last_successful_refresh_at <= ?
      )
      AND (
        gmrs.next_retry_at IS NULL
        OR gmrs.next_retry_at <= ?
      )
    ORDER BY gmd.sort_order ASC, gmd.slug ASC
    "#
}

fn upsert_global_moodboard_definition_sql() -> &'static str {
    r#"
    INSERT INTO global_moodboard_definitions (
      slug, title, vibe_summary, search_queries_json, sort_order,
      status, created_at, updated_at
    )
    VALUES (?, ?, ?, ?, ?, 'active', ?, ?)
    ON CONFLICT(slug) DO UPDATE SET
      title = excluded.title,
      vibe_summary = excluded.vibe_summary,
      search_queries_json = excluded.search_queries_json,
      sort_order = excluded.sort_order,
      updated_at = excluded.updated_at
    WHERE global_moodboard_definitions.status = 'active'
    "#
}

async fn sync_global_moodboard_definitions_for_scheduler(
    db: &D1Database,
    now: &str,
) -> WorkerResult<()> {
    let statements = default_moodboards()
        .into_iter()
        .enumerate()
        .map(|(index, seed)| {
            let search_queries_json =
                serde_json::to_string(&seed.search_queries).unwrap_or_else(|_| "[]".to_string());
            (
                upsert_global_moodboard_definition_sql(),
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

pub async fn enqueue_due_global_moodboard_libraries(env: &Env) -> WorkerResult<()> {
    let db = env.d1("DB")?;
    let now = now_iso_string();
    sync_global_moodboard_definitions_for_scheduler(&db, &now).await?;

    let stale_after_hours = load_config_i64(&db, "global_library_stale_after_hours", 168).await?;
    let stale_cutoff = add_minutes_iso(&now, -(stale_after_hours * 60));
    let due = db::all::<DueGlobalMoodboardRow>(
        &db,
        scheduler_due_global_moodboard_libraries_sql(),
        vec![json!(stale_cutoff), json!(now)],
    )
    .await?;

    for row in due {
        let reservation = QueueReservation::new(
            "ensure_global_moodboard_library",
            format!("global:ensure:{}", row.moodboard_slug),
            None,
            None,
            ReservationTtl::FiveMinutes,
        );
        reserve_and_send_reference_pipeline_message(
            &db,
            env,
            reservation,
            ReferencePipelineMessage::EnsureGlobalMoodboardLibrary {
                moodboard_slug: row.moodboard_slug,
                reason: row.reason,
            },
            &now,
        )
        .await?;
    }

    Ok(())
}

async fn load_config_i64(db: &D1Database, key: &str, fallback: i64) -> WorkerResult<i64> {
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
