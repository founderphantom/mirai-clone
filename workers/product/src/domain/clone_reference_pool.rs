use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

#[derive(Clone, Debug, PartialEq)]
pub struct GlobalReferenceForClonePool {
    pub id: String,
    pub moodboard_slug: String,
    pub overall_reference_score: f64,
    pub generation_use_count: u32,
}

impl GlobalReferenceForClonePool {
    pub fn new(
        id: impl Into<String>,
        moodboard_slug: impl Into<String>,
        overall_reference_score: f64,
        generation_use_count: u32,
    ) -> Self {
        Self {
            id: id.into(),
            moodboard_slug: moodboard_slug.into(),
            overall_reference_score,
            generation_use_count,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatibilityAction {
    EnqueueReview,
    RepairMissingVisualReference,
    Skip,
}

pub fn clone_pool_run_is_reusable(
    status: &str,
    selected_hash_matches: bool,
    updated_at: Option<&str>,
    now: &str,
    stale_after_minutes: i64,
) -> bool {
    if !selected_hash_matches {
        return false;
    }
    if !matches!(
        status,
        "queued" | "waiting_for_global_library" | "compatibility_reviewing"
    ) {
        return false;
    }
    let Some(updated_at) = updated_at else {
        return false;
    };
    let Ok(updated_at) = OffsetDateTime::parse(updated_at, &Rfc3339) else {
        return false;
    };
    let Ok(now) = OffsetDateTime::parse(now, &Rfc3339) else {
        return false;
    };

    updated_at >= now - Duration::minutes(stale_after_minutes.max(1))
}

pub fn compatibility_action_for(
    status: Option<&str>,
    next_retry_at: Option<&str>,
    has_visual_reference: bool,
    now: &str,
) -> CompatibilityAction {
    match status {
        None => CompatibilityAction::EnqueueReview,
        Some("queued") => CompatibilityAction::EnqueueReview,
        Some("accepted") if !has_visual_reference => {
            CompatibilityAction::RepairMissingVisualReference
        }
        Some("failed") if retry_due(next_retry_at, now) => CompatibilityAction::EnqueueReview,
        _ => CompatibilityAction::Skip,
    }
}

pub fn select_balanced_compatibility_wave(
    candidates: Vec<GlobalReferenceForClonePool>,
    selected_slugs: &[String],
    limit: usize,
) -> Vec<GlobalReferenceForClonePool> {
    if limit == 0 || candidates.is_empty() || selected_slugs.is_empty() {
        return Vec::new();
    }

    let mut buckets = selected_slugs
        .iter()
        .map(|slug| {
            let mut refs = candidates
                .iter()
                .filter(|reference| reference.moodboard_slug == *slug)
                .cloned()
                .collect::<Vec<_>>();
            refs.sort_by(|left, right| {
                right
                    .overall_reference_score
                    .total_cmp(&left.overall_reference_score)
                    .then_with(|| left.generation_use_count.cmp(&right.generation_use_count))
                    .then_with(|| left.id.cmp(&right.id))
            });
            refs
        })
        .collect::<Vec<_>>();

    let mut selected = Vec::new();
    while selected.len() < limit {
        let mut progressed = false;
        for bucket in &mut buckets {
            if selected.len() >= limit {
                break;
            }
            if !bucket.is_empty() {
                selected.push(bucket.remove(0));
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }

    selected
}

pub fn clone_visual_reference_id(clone_id: &str, global_reference_id: &str) -> String {
    deterministic_id("visual_ref", &[clone_id, global_reference_id])
}

pub fn clone_inspiration_pool_id(clone_id: &str, visual_reference_id: &str) -> String {
    deterministic_id("inspiration_pool", &[clone_id, visual_reference_id])
}

fn retry_due(next_retry_at: Option<&str>, now: &str) -> bool {
    let Some(next_retry_at) = next_retry_at else {
        return false;
    };
    let Ok(next_retry_at) = OffsetDateTime::parse(next_retry_at, &Rfc3339) else {
        return false;
    };
    let Ok(now) = OffsetDateTime::parse(now, &Rfc3339) else {
        return false;
    };
    next_retry_at <= now
}

fn deterministic_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    let digest = hasher.finalize();
    format!("{prefix}_{}", hex::encode(&digest[..16]))
}
