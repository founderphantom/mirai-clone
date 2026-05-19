use crate::db;
use crate::queues::messages::ReferencePipelineMessage;
use serde::Deserialize;
use serde_json::json;
use worker::{D1Database, Env, Result as WorkerResult};

const REFERENCE_QUEUE_NAME: &str = "REFERENCE_PIPELINE_QUEUE";

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
    enqueue_global_topups_for_underfilled_slugs(db, env, selected_slugs, "onboarding_selection")
        .await?;

    if let Some(clone) = load_ready_active_clone(db, user_id).await? {
        env.queue(REFERENCE_QUEUE_NAME)?
            .send(ReferencePipelineMessage::BuildCloneReferencePool {
                user_id: user_id.to_string(),
                clone_id: clone.id,
                reason: "moodboard_selection_changed".to_string(),
                wakeup_moodboard_slug: None,
            })
            .await?;
    }

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
