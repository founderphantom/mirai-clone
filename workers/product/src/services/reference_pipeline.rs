use crate::db;
use crate::queues::messages::ReferencePipelineMessage;
use crate::services::queue_reservations::{
    now_iso_string, reservation_key_for_reference_message,
    reserve_and_send_reference_pipeline_message,
};
use serde::Deserialize;
use serde_json::json;
use worker::{D1Database, Env, Result as WorkerResult};

#[derive(Debug, Deserialize)]
struct ConfigRow {
    value: String,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u32,
}

#[derive(Debug, Deserialize)]
struct ReadyCloneRow {
    id: String,
}

pub async fn enqueue_after_moodboard_save(
    db: &D1Database,
    env: &Env,
    user_id: &str,
    selected_slugs: &[String],
) -> WorkerResult<()> {
    if let Some(clone) = load_ready_active_clone(db, user_id).await? {
        reserve_and_send_reference_message(
            db,
            env,
            ReferencePipelineMessage::BuildCloneReferencePool {
                user_id: user_id.to_string(),
                clone_id: clone.id,
                reason: "moodboard_selection_changed".to_string(),
                wakeup_moodboard_slug: None,
            },
        )
        .await?;
    }

    enqueue_global_topups_for_underfilled_slugs(db, env, selected_slugs, "onboarding_selection")
        .await?;

    Ok(())
}

pub async fn enqueue_global_topups_for_underfilled_slugs(
    db: &D1Database,
    env: &Env,
    selected_slugs: &[String],
    reason: &str,
) -> WorkerResult<()> {
    let target = global_refs_per_moodboard_target(db).await?;
    for slug in selected_slugs {
        if active_global_reference_count(db, slug).await? < target {
            reserve_and_send_reference_message(
                db,
                env,
                ReferencePipelineMessage::EnsureGlobalMoodboardLibrary {
                    moodboard_slug: slug.clone(),
                    reason: reason.to_string(),
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn reserve_and_send_reference_message(
    db: &D1Database,
    env: &Env,
    message: ReferencePipelineMessage,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let reservation = reservation_key_for_reference_message(&message, 0, 0);
    reserve_and_send_reference_pipeline_message(db, env, reservation, message, &now).await?;
    Ok(())
}

async fn global_refs_per_moodboard_target(db: &D1Database) -> WorkerResult<u32> {
    let row = db::first::<ConfigRow>(
        db,
        "SELECT value FROM blitz_config WHERE key = 'global_refs_per_moodboard_target'",
        vec![],
    )
    .await?;
    Ok(row
        .and_then(|row| row.value.trim().parse::<u32>().ok())
        .unwrap_or(25))
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

async fn load_ready_active_clone(
    db: &D1Database,
    user_id: &str,
) -> WorkerResult<Option<ReadyCloneRow>> {
    db::first(
        db,
        r#"
        SELECT id
        FROM clone_profiles
        WHERE user_id = ?
          AND deleted_at IS NULL
          AND status = 'active'
          AND soul_status IN ('ready', 'completed')
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
        vec![json!(user_id)],
    )
    .await
}
