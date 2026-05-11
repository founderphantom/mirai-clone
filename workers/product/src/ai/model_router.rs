use super::tasks::AiTask;

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
                && model.model == "@cf/moonshotai/kimi-k2.6"
                && model.supports_structured_json
                && (!task.requires_vision() || model.supports_vision)
        })
        .cloned()
}

pub fn clamp_moderation_level(value: i32) -> u8 {
    value.clamp(0, 10) as u8
}
