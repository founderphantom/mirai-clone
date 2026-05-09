#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiTask {
    PhotoQualityReview,
    HumanPresenceDetection,
    BubbleGeneration,
    NicheSeedExtraction,
    NicheClusterExpansion,
    VisualReferenceSelection,
    Moderation,
}

impl AiTask {
    pub fn requires_vision(self) -> bool {
        matches!(
            self,
            AiTask::PhotoQualityReview | AiTask::HumanPresenceDetection
        )
    }
}
