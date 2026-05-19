use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessDecision {
    Recent,
    TooOld,
    UnknownAllowed,
    UnknownRejected,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanPresenceReview {
    pub has_human: bool,
    pub human_count: u32,
    pub human_type: String,
    pub confidence: f64,
    pub organic_photo_score: f64,
    pub freshness_visual_score: f64,
    pub capture_style: String,
    pub aesthetic_tags: Vec<String>,
    pub rejection_reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Influence {
    pub liked_tags: HashMap<String, u32>,
    pub disliked_tags: HashMap<String, u32>,
    pub liked_clusters: HashMap<String, u32>,
    pub disliked_clusters: HashMap<String, u32>,
    pub liked_moodboards: HashMap<String, u32>,
    pub disliked_moodboards: HashMap<String, u32>,
    pub liked_handles: HashMap<String, u32>,
    pub disliked_handles: HashMap<String, u32>,
    pub liked_platforms: HashMap<String, u32>,
    pub disliked_platforms: HashMap<String, u32>,
    pub liked_visual_reference_ids: HashMap<String, u32>,
    pub disliked_visual_reference_ids: HashMap<String, u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwipeMetadata {
    pub action: String,
    pub aesthetic_tags: Vec<String>,
    pub niche_cluster: Option<String>,
    pub moodboard_id: Option<String>,
    pub moodboard_slug: Option<String>,
    pub source_handle: Option<String>,
    pub source_platform: String,
    pub visual_reference_id: Option<String>,
    pub global_reference_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualReferenceForSelection {
    pub id: String,
    pub source_platform: String,
    pub source_published_at: Option<String>,
    pub niche_cluster: Option<String>,
    pub moodboard_id: Option<String>,
    pub moodboard_slug: Option<String>,
    pub source_handle: Option<String>,
    pub aesthetic_tags: Vec<String>,
    pub human_presence_score: f64,
    pub organic_photo_score: f64,
    pub freshness_visual_score: f64,
    pub generation_use_count: u32,
    pub last_liked_at: Option<String>,
}

pub fn filter_synthetic_terms(query: &str) -> Result<(), &'static str> {
    let normalized = normalize_words(query);
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    let blocked_terms = [
        "ai",
        "midjourney",
        "dalle",
        "dall e",
        "stable diffusion",
        "synthetic",
        "generative",
        "generated avatar",
        "ai generated",
        "ai avatar",
    ];

    for term in blocked_terms {
        if term.split_whitespace().count() == 1 {
            if tokens.iter().any(|token| *token == term) {
                return Err("synthetic_generation_term");
            }
        } else if normalized.contains(term) {
            return Err("synthetic_generation_term");
        }
    }

    Ok(())
}

pub fn classify_freshness(
    published_at: Option<&str>,
    allow_unknown: bool,
    now: &str,
    years: i64,
) -> FreshnessDecision {
    let Some(published_at) = published_at else {
        return if allow_unknown {
            FreshnessDecision::UnknownAllowed
        } else {
            FreshnessDecision::UnknownRejected
        };
    };

    let Ok(published_at) = OffsetDateTime::parse(published_at, &Rfc3339) else {
        return if allow_unknown {
            FreshnessDecision::UnknownAllowed
        } else {
            FreshnessDecision::UnknownRejected
        };
    };
    let Ok(now) = OffsetDateTime::parse(now, &Rfc3339) else {
        return FreshnessDecision::TooOld;
    };

    if published_at > now {
        return FreshnessDecision::TooOld;
    }

    if published_at >= now - Duration::days(365 * years) {
        FreshnessDecision::Recent
    } else {
        FreshnessDecision::TooOld
    }
}

pub fn can_accept_human_presence(review: &HumanPresenceReview) -> Result<(), &'static str> {
    if !review.has_human || review.human_count == 0 {
        return Err("no_human");
    }
    if review.human_count > 1 {
        return Err("multiple_humans");
    }
    if !is_supported_human_type(&review.human_type) {
        return Err("unsupported_human_type");
    }
    if !is_unit_score(review.confidence)
        || !is_unit_score(review.organic_photo_score)
        || !is_unit_score(review.freshness_visual_score)
    {
        return Err("invalid_score");
    }
    if review.confidence < 0.7 {
        return Err("low_confidence");
    }
    if review.organic_photo_score < 0.7 {
        return Err("not_organic");
    }
    if review.freshness_visual_score < 0.7 {
        return Err("not_recent_visual_style");
    }

    let capture_style = normalize_words(&review.capture_style);
    if capture_style.contains("render") {
        return Err("too_synthetic");
    }

    Ok(())
}

pub fn daily_generation_limit(plan: &str, free_limit: u32, paid_limit: u32) -> u32 {
    match plan.trim().to_ascii_lowercase().as_str() {
        "paid" | "studio" | "pro" => paid_limit,
        _ => free_limit,
    }
}

pub fn accumulate_influence(swipes: &[SwipeMetadata]) -> Influence {
    let mut influence = Influence::default();

    for swipe in swipes {
        match swipe.action.to_ascii_lowercase().as_str() {
            "like" => {
                increment_all(&mut influence.liked_tags, &swipe.aesthetic_tags);
                increment_option(
                    &mut influence.liked_clusters,
                    swipe.niche_cluster.as_deref(),
                );
                increment_option(
                    &mut influence.liked_moodboards,
                    swipe.moodboard_slug.as_deref(),
                );
                increment_option(&mut influence.liked_handles, swipe.source_handle.as_deref());
                increment(&mut influence.liked_platforms, &swipe.source_platform);
                increment_reference_option(
                    &mut influence.liked_visual_reference_ids,
                    swipe.visual_reference_id.as_deref(),
                );
            }
            "dislike" => {
                increment_all(&mut influence.disliked_tags, &swipe.aesthetic_tags);
                increment_option(
                    &mut influence.disliked_clusters,
                    swipe.niche_cluster.as_deref(),
                );
                increment_option(
                    &mut influence.disliked_moodboards,
                    swipe.moodboard_slug.as_deref(),
                );
                increment_option(
                    &mut influence.disliked_handles,
                    swipe.source_handle.as_deref(),
                );
                increment(&mut influence.disliked_platforms, &swipe.source_platform);
                increment_reference_option(
                    &mut influence.disliked_visual_reference_ids,
                    swipe.visual_reference_id.as_deref(),
                );
            }
            _ => {}
        }
    }

    influence
}

pub fn select_visual_references(
    references: &[VisualReferenceForSelection],
    influence: &Influence,
    limit: usize,
    reuse_cap: u32,
    now: &str,
) -> Vec<VisualReferenceForSelection> {
    let mut scored = references
        .iter()
        .filter(|reference| is_selectable_reference(reference, now))
        .filter(|reference| reference.generation_use_count < reuse_cap)
        .filter(|reference| {
            reference.generation_use_count == 0
                || reference.last_liked_at.is_some()
                || contains_reference_id(&influence.liked_visual_reference_ids, &reference.id)
        })
        .map(|reference| {
            (
                score_visual_reference(reference, influence, now),
                reference.id.clone(),
                reference,
            )
        })
        .collect::<Vec<_>>();

    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut selected = Vec::new();
    let mut cluster_counts: HashMap<String, u32> = HashMap::new();
    let mut handle_counts: HashMap<String, u32> = HashMap::new();
    let mut moodboard_counts: HashMap<String, u32> = HashMap::new();
    let mut platform_counts: HashMap<String, u32> = HashMap::new();

    while selected.len() < limit {
        let Some((index, reference)) = next_reference_for_selection(
            &scored,
            &platform_counts,
            &cluster_counts,
            &handle_counts,
            &moodboard_counts,
        ) else {
            break;
        };

        let reference = reference.clone();
        scored.remove(index);

        let platform_key = normalize_key(&reference.source_platform);
        let cluster_key = reference.niche_cluster.as_deref().and_then(normalize_key);
        let handle_key = reference.source_handle.as_deref().and_then(normalize_key);
        let moodboard_key = selection_moodboard_key(&reference);

        if let Some(platform) = platform_key {
            *platform_counts.entry(platform).or_insert(0) += 1;
        }
        if let Some(cluster) = cluster_key {
            *cluster_counts.entry(cluster).or_insert(0) += 1;
        }
        if let Some(handle) = handle_key {
            *handle_counts.entry(handle).or_insert(0) += 1;
        }
        if let Some(moodboard) = moodboard_key {
            *moodboard_counts.entry(moodboard).or_insert(0) += 1;
        }
        selected.push(reference);
    }

    selected
}

fn next_reference_for_selection<'a>(
    scored: &'a [(f64, String, &'a VisualReferenceForSelection)],
    platform_counts: &HashMap<String, u32>,
    cluster_counts: &HashMap<String, u32>,
    handle_counts: &HashMap<String, u32>,
    moodboard_counts: &HashMap<String, u32>,
) -> Option<(usize, &'a VisualReferenceForSelection)> {
    let mut best_selectable = None;
    let mut best_unrepresented_moodboard = None;

    for (index, (_, _, reference)) in scored.iter().enumerate() {
        if !passes_selection_caps(
            reference,
            platform_counts,
            cluster_counts,
            handle_counts,
            moodboard_counts,
        ) {
            continue;
        }

        best_selectable.get_or_insert((index, *reference));

        let moodboard_key = selection_moodboard_key(reference);
        let is_unrepresented = moodboard_key
            .as_deref()
            .map(|moodboard| !moodboard_counts.contains_key(moodboard))
            .unwrap_or(false);

        if is_unrepresented {
            best_unrepresented_moodboard.get_or_insert((index, *reference));
        }
    }

    if let Some((_, best_reference)) = best_selectable {
        let best_moodboard = selection_moodboard_key(best_reference);
        let best_repeats_moodboard = best_moodboard
            .as_deref()
            .map(|moodboard| moodboard_counts.contains_key(moodboard))
            .unwrap_or(false);
        if best_repeats_moodboard {
            return best_unrepresented_moodboard.or(best_selectable);
        }
    }

    best_selectable
}

fn passes_selection_caps(
    reference: &VisualReferenceForSelection,
    platform_counts: &HashMap<String, u32>,
    cluster_counts: &HashMap<String, u32>,
    handle_counts: &HashMap<String, u32>,
    moodboard_counts: &HashMap<String, u32>,
) -> bool {
    let platform_key = normalize_key(&reference.source_platform);
    let platform_count = platform_counts
        .get(platform_key.as_deref().unwrap_or(""))
        .copied()
        .unwrap_or(0);
    if platform_count >= 3 {
        return false;
    }

    let cluster_key = reference.niche_cluster.as_deref().and_then(normalize_key);
    if let Some(cluster) = cluster_key.as_deref() {
        if cluster_counts.get(cluster).copied().unwrap_or(0) >= 2 {
            return false;
        }
    }

    let handle_key = reference.source_handle.as_deref().and_then(normalize_key);
    if let Some(handle) = handle_key.as_deref() {
        if handle_counts.get(handle).copied().unwrap_or(0) >= 2 {
            return false;
        }
    }

    let moodboard_key = selection_moodboard_key(reference);
    if let Some(moodboard) = moodboard_key.as_deref() {
        if moodboard_counts.get(moodboard).copied().unwrap_or(0) >= 2 {
            return false;
        }
    }

    true
}

fn score_visual_reference(
    reference: &VisualReferenceForSelection,
    influence: &Influence,
    now: &str,
) -> f64 {
    let mut score = reference.human_presence_score * 3.0
        + reference.organic_photo_score * 2.0
        + reference.freshness_visual_score * 2.0
        + source_recency_score(reference.source_published_at.as_deref(), now);

    for tag in &reference.aesthetic_tags {
        score += normalized_count(&influence.liked_tags, tag) as f64 * 0.4;
        score -= normalized_count(&influence.disliked_tags, tag) as f64 * 0.6;
    }

    if let Some(cluster) = reference.niche_cluster.as_deref() {
        score += normalized_count(&influence.liked_clusters, cluster) as f64 * 0.6;
        score -= normalized_count(&influence.disliked_clusters, cluster) as f64 * 0.8;
    }

    if let Some(moodboard) = reference
        .moodboard_slug
        .as_deref()
        .or(reference.niche_cluster.as_deref())
    {
        score += normalized_count(&influence.liked_moodboards, moodboard) as f64 * 0.6;
        score -= normalized_count(&influence.disliked_moodboards, moodboard) as f64 * 0.8;
    }

    if let Some(handle) = reference.source_handle.as_deref() {
        score += normalized_count(&influence.liked_handles, handle) as f64 * 0.4;
        score -= normalized_count(&influence.disliked_handles, handle) as f64 * 0.6;
    }

    score += normalized_count(&influence.liked_platforms, &reference.source_platform) as f64 * 0.25;
    score -=
        normalized_count(&influence.disliked_platforms, &reference.source_platform) as f64 * 0.35;
    score += reference_id_count(&influence.liked_visual_reference_ids, &reference.id) as f64 * 2.0;
    score -=
        reference_id_count(&influence.disliked_visual_reference_ids, &reference.id) as f64 * 2.0;

    if reference.generation_use_count > 0 {
        score -= reference.generation_use_count as f64 * 0.25;
    }

    score
}

fn selection_moodboard_key(reference: &VisualReferenceForSelection) -> Option<String> {
    reference
        .moodboard_slug
        .as_deref()
        .or(reference.niche_cluster.as_deref())
        .and_then(normalize_key)
}

fn is_selectable_reference(reference: &VisualReferenceForSelection, now: &str) -> bool {
    is_unit_score(reference.human_presence_score)
        && is_unit_score(reference.organic_photo_score)
        && is_unit_score(reference.freshness_visual_score)
        && reference
            .source_published_at
            .as_deref()
            .map(|published_at| {
                classify_freshness(Some(published_at), true, now, 5) != FreshnessDecision::TooOld
            })
            .unwrap_or(true)
}

fn source_recency_score(published_at: Option<&str>, now: &str) -> f64 {
    match classify_freshness(published_at, true, now, 5) {
        FreshnessDecision::Recent => {
            let Some(published_at) = published_at else {
                return 0.0;
            };
            let Ok(published_at) = OffsetDateTime::parse(published_at, &Rfc3339) else {
                return 0.0;
            };
            let Ok(now) = OffsetDateTime::parse(now, &Rfc3339) else {
                return 0.0;
            };
            let age_days = (now - published_at).whole_days().max(0) as f64;
            (1.0 - (age_days / (365.0 * 5.0))).max(0.0)
        }
        FreshnessDecision::UnknownAllowed => 0.1,
        FreshnessDecision::TooOld | FreshnessDecision::UnknownRejected => -2.0,
    }
}

fn increment_all(counts: &mut HashMap<String, u32>, values: &[String]) {
    for value in values {
        increment(counts, value);
    }
}

fn increment_option(counts: &mut HashMap<String, u32>, value: Option<&str>) {
    if let Some(value) = value {
        increment(counts, value);
    }
}

fn increment(counts: &mut HashMap<String, u32>, value: &str) {
    if let Some(value) = normalize_key(value) {
        let count = counts.entry(value).or_insert(0);
        *count = count.saturating_add(1);
    }
}

fn increment_reference_option(counts: &mut HashMap<String, u32>, value: Option<&str>) {
    if let Some(value) = value {
        if let Some(value) = normalize_reference_id(value) {
            let count = counts.entry(value).or_insert(0);
            *count = count.saturating_add(1);
        }
    }
}

fn normalized_count(counts: &HashMap<String, u32>, value: &str) -> u32 {
    normalize_key(value)
        .and_then(|value| counts.get(&value).copied())
        .unwrap_or(0)
}

fn reference_id_count(counts: &HashMap<String, u32>, value: &str) -> u32 {
    normalize_reference_id(value)
        .and_then(|value| counts.get(&value).copied())
        .unwrap_or(0)
}

fn contains_reference_id(counts: &HashMap<String, u32>, value: &str) -> bool {
    reference_id_count(counts, value) > 0
}

fn normalize_key(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn normalize_reference_id(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn is_unit_score(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn is_supported_human_type(value: &str) -> bool {
    matches!(
        normalize_key(value).as_deref(),
        Some("full_body" | "upper_body" | "portrait" | "person" | "human")
    )
}

fn normalize_words(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
