use serde::{Deserialize, Serialize};
use serde_json::Value;
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
}

pub async fn handle_batch(batch: MessageBatch<Value>, _env: Env) -> WorkerResult<()> {
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
        }

        raw_message.ack();
    }

    Ok(())
}
