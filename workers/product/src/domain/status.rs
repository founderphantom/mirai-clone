#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoulStatus {
    Draft,
    Queued,
    Training,
    Ready,
    Failed,
    ProviderActionRequired,
}

pub fn can_transition_soul_status(from: SoulStatus, to: SoulStatus) -> bool {
    matches!(
        (from, to),
        (SoulStatus::Draft, SoulStatus::Queued)
            | (SoulStatus::Queued, SoulStatus::Training)
            | (SoulStatus::Training, SoulStatus::Ready)
            | (SoulStatus::Training, SoulStatus::Failed)
            | (SoulStatus::Training, SoulStatus::ProviderActionRequired)
            | (SoulStatus::ProviderActionRequired, SoulStatus::Queued)
    )
}
