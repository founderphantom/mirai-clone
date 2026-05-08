use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ProviderAccountCandidate {
    pub id: String,
    pub health_state: String,
    pub active_leases: u32,
    pub max_leases: u32,
}

pub fn choose_provider_account(
    candidates: &[ProviderAccountCandidate],
) -> Option<&ProviderAccountCandidate> {
    candidates
        .iter()
        .filter(|candidate| {
            candidate.health_state == "healthy" && candidate.active_leases < candidate.max_leases
        })
        .min_by_key(|candidate| candidate.active_leases)
}
