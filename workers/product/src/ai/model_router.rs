use super::tasks::AiTask;
use super::workers_ai::KIMI_K2_6_MODEL;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub supports_vision: bool,
    pub supports_structured_json: bool,
}

pub fn choose_model(task: AiTask, models: &[ModelConfig]) -> Option<ModelConfig> {
    models
        .iter()
        .find(|model| {
            model.provider == "workers_ai"
                && model.model == KIMI_K2_6_MODEL
                && model.supports_structured_json
                && (!task.requires_vision() || model.supports_vision)
        })
        .cloned()
}

pub fn clamp_moderation_level(value: i32) -> u8 {
    value.clamp(0, 10) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kimi_is_the_only_analysis_model_for_app_analysis_tasks() {
        let models = vec![
            ModelConfig {
                provider: "deepseek".to_string(),
                model: "deepseek-v4-pro".to_string(),
                supports_vision: false,
                supports_structured_json: true,
            },
            ModelConfig {
                provider: "workers_ai".to_string(),
                model: KIMI_K2_6_MODEL.to_string(),
                supports_vision: true,
                supports_structured_json: true,
            },
        ];

        for task in [
            AiTask::PhotoQualityReview,
            AiTask::HumanPresenceDetection,
            AiTask::MoodboardGeneration,
            AiTask::NicheSeedExtraction,
            AiTask::NicheKnowledgeExtraction,
            AiTask::NicheClusterExpansion,
            AiTask::VisualReferenceSelection,
            AiTask::Moderation,
        ] {
            let selected = choose_model(task, &models).unwrap();

            assert_eq!(selected.provider, "workers_ai");
            assert_eq!(selected.model, KIMI_K2_6_MODEL);
        }
    }
}
