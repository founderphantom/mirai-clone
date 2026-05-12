use crate::db;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worker::{Env, MessageBatch, MessageExt, Result as WorkerResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum NicheResearchMessage {
    SeedFromBubbles {
        user_id: String,
        clone_id: String,
        bubble_ids: Vec<String>,
        moderation_level: u8,
    },
    RefreshPool {
        user_id: String,
        clone_id: String,
        reason: String,
    },
}

pub async fn handle_batch(batch: MessageBatch<Value>, env: Env) -> WorkerResult<()> {
    for raw_message in batch.raw_iter() {
        let body = match serde_wasm_bindgen::from_value::<NicheResearchMessage>(raw_message.body())
        {
            Ok(body) => body,
            Err(error) => {
                web_sys::console::error_1(
                    &format!("failed to deserialize niche research queue message: {error:?}")
                        .into(),
                );
                raw_message.ack();
                continue;
            }
        };

        match body {
            NicheResearchMessage::SeedFromBubbles {
                user_id,
                clone_id,
                bubble_ids,
                moderation_level,
            } => {
                web_sys::console::log_1(
                    &format!(
                        "ack niche research seed user={user_id} clone={clone_id} bubbles={} moderation={moderation_level}",
                        bubble_ids.len()
                    )
                    .into(),
                );
            }
            NicheResearchMessage::RefreshPool {
                user_id,
                clone_id,
                reason,
            } => {
                persist_refresh_pool_requested(&env, &user_id, &clone_id, &reason).await?;
                web_sys::console::log_1(
                    &format!(
                        "ack niche research refresh user={user_id} clone={clone_id} reason={reason}"
                    )
                    .into(),
                );
            }
        }

        raw_message.ack();
    }

    Ok(())
}

async fn persist_refresh_pool_requested(
    env: &Env,
    user_id: &str,
    clone_id: &str,
    reason: &str,
) -> WorkerResult<()> {
    let db = env.d1("DB")?;
    let now = now_iso_string();
    db::exec(
        &db,
        r#"
        UPDATE clone_profiles
        SET provider_config_json = json_set(
              CASE
                WHEN json_valid(provider_config_json) THEN provider_config_json
                ELSE '{}'
              END,
              '$.nicheResearchStatus',
              'refresh_requested',
              '$.nicheResearchReason',
              ?,
              '$.nicheResearchRequestedAt',
              ?
            ),
            updated_at = ?
        WHERE user_id = ?
          AND id = ?
          AND deleted_at IS NULL
        "#,
        vec![
            json!(reason),
            json!(now),
            json!(now),
            json!(user_id),
            json!(clone_id),
        ],
    )
    .await
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn refresh_pool_messages_serialize_as_queue_contract() {
        let message = NicheResearchMessage::RefreshPool {
            user_id: "user_1".to_string(),
            clone_id: "clone_1".to_string(),
            reason: "pool_depleted".to_string(),
        };

        assert_eq!(
            serde_json::to_value(message).unwrap(),
            json!({
                "type": "refresh_pool",
                "userId": "user_1",
                "cloneId": "clone_1",
                "reason": "pool_depleted"
            })
        );

        let parsed: NicheResearchMessage = serde_json::from_value(json!({
            "type": "refresh_pool",
            "userId": "user_1",
            "cloneId": "clone_1",
            "reason": "pool_depleted"
        }))
        .unwrap();
        assert!(matches!(
            parsed,
            NicheResearchMessage::RefreshPool {
                user_id,
                clone_id,
                reason
            } if user_id == "user_1" && clone_id == "clone_1" && reason == "pool_depleted"
        ));
    }
}
