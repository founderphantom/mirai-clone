pub mod clone_training;
pub mod generation;
pub mod messages;
pub mod niche_research;

use serde_json::Value;
use worker::{Env, Error, MessageBatch, Result as WorkerResult};

const CLONE_TRAINING_QUEUE_NAME: &str = "mirai-clone-training";
const GENERATION_QUEUE_NAME: &str = "mirai-generation";
const NICHE_RESEARCH_QUEUE_NAME: &str = "mirai-niche-research";

pub async fn handle_batch(batch: MessageBatch<Value>, env: Env) -> WorkerResult<()> {
    let queue_name = batch.queue();
    match queue_name.as_str() {
        CLONE_TRAINING_QUEUE_NAME => clone_training::handle_batch(batch, env).await,
        GENERATION_QUEUE_NAME => generation::handle_batch(batch, env).await,
        NICHE_RESEARCH_QUEUE_NAME => niche_research::handle_batch(batch, env).await,
        _ => {
            web_sys::console::error_1(
                &format!("unhandled product queue batch from queue: {queue_name}").into(),
            );
            batch.retry_all();
            Err(Error::RustError(format!(
                "unhandled_product_queue:{queue_name}"
            )))
        }
    }
}
