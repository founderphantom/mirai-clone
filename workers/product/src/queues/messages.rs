use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CloneTrainingMessage {
    SubmitCloneTraining {
        job_id: String,
        clone_id: String,
        user_id: String,
        idempotency_key: String,
    },
}
