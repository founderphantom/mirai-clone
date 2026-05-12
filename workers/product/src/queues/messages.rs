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
    PollCloneTraining {
        job_id: String,
        clone_id: String,
        user_id: String,
        idempotency_key: String,
        provider_soul_id: String,
        attempt: u8,
        max_attempts: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GenerationMessage {
    GenerateBlitzBatch {
        batch_id: String,
        clone_id: String,
        user_id: String,
        idempotency_key: String,
        visual_reference_ids: Vec<String>,
        provider_soul_id: String,
    },
    PollGeneration {
        job_id: String,
        batch_id: String,
        attempt: u8,
        max_attempts: u8,
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

    #[test]
    fn poll_clone_training_serializes_fields_as_camel_case() {
        let message = CloneTrainingMessage::PollCloneTraining {
            job_id: "train_1".to_string(),
            clone_id: "clone_1".to_string(),
            user_id: "user_1".to_string(),
            idempotency_key: "clone_upload:user_1:abc".to_string(),
            provider_soul_id: "soul_1".to_string(),
            attempt: 1,
            max_attempts: 90,
        };

        assert_eq!(
            serde_json::to_value(message).unwrap(),
            json!({
                "type": "poll_clone_training",
                "jobId": "train_1",
                "cloneId": "clone_1",
                "userId": "user_1",
                "idempotencyKey": "clone_upload:user_1:abc",
                "providerSoulId": "soul_1",
                "attempt": 1,
                "maxAttempts": 90,
            })
        );
    }

    #[test]
    fn generation_messages_serialize_blitz_fields_as_camel_case() {
        let message = GenerationMessage::GenerateBlitzBatch {
            batch_id: "batch_1".to_string(),
            clone_id: "clone_1".to_string(),
            user_id: "user_1".to_string(),
            idempotency_key: "blitz_gen:batch_1".to_string(),
            visual_reference_ids: vec!["vref_1".to_string()],
            provider_soul_id: "soul_1".to_string(),
        };
        assert_eq!(
            serde_json::to_value(message).unwrap(),
            json!({
                "type": "generate_blitz_batch",
                "batchId": "batch_1",
                "cloneId": "clone_1",
                "userId": "user_1",
                "idempotencyKey": "blitz_gen:batch_1",
                "visualReferenceIds": ["vref_1"],
                "providerSoulId": "soul_1"
            })
        );

        let poll = GenerationMessage::PollGeneration {
            job_id: "gen_1".to_string(),
            batch_id: "batch_1".to_string(),
            attempt: 1,
            max_attempts: 30,
        };
        assert_eq!(
            serde_json::to_value(poll).unwrap()["type"],
            json!("poll_generation")
        );
    }
}
