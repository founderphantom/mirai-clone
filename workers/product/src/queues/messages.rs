use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CloneTrainingMessage {
    SubmitCloneTraining {
        job_id: String,
        clone_id: String,
        user_id: String,
        idempotency_key: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn submit_clone_training_serializes_fields_as_camel_case() {
        let message = CloneTrainingMessage::SubmitCloneTraining {
            job_id: "train_1".to_string(),
            clone_id: "clone_1".to_string(),
            user_id: "user_1".to_string(),
            idempotency_key: "clone_upload:user_1:abc".to_string(),
        };

        let value = serde_json::to_value(message).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "submit_clone_training",
                "jobId": "train_1",
                "cloneId": "clone_1",
                "userId": "user_1",
                "idempotencyKey": "clone_upload:user_1:abc",
            })
        );
        assert!(value.get("job_id").is_none());
        assert!(value.get("clone_id").is_none());
        assert!(value.get("user_id").is_none());
        assert!(value.get("idempotency_key").is_none());
    }
}
