use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    pub source_platform: String,
    pub visual_reference_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualReferenceForSelection {
    pub id: String,
    pub source_platform: String,
    pub source_published_at: Option<String>,
    pub niche_cluster: Option<String>,
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
    if capture_style.contains("professional studio")
        || capture_style.contains("studio")
        || capture_style.contains("editorial")
        || capture_style.contains("render")
    {
        return Err("too_professional");
    }

    Ok(())
}

pub fn daily_generation_limit(plan: &str, free_limit: u32, paid_limit: u32) -> u32 {
    match plan.to_ascii_lowercase().as_str() {
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
                increment(&mut influence.liked_platforms, &swipe.source_platform);
                increment_option(
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
                increment(&mut influence.disliked_platforms, &swipe.source_platform);
                increment_option(
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
        .filter(|reference| reference.generation_use_count < reuse_cap)
        .filter(|reference| {
            reference.generation_use_count == 0
                || reference.last_liked_at.is_some()
                || influence
                    .liked_visual_reference_ids
                    .contains_key(&reference.id)
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
    let mut platform_counts: HashMap<String, u32> = HashMap::new();

    for (_, _, reference) in scored {
        if selected.len() >= limit {
            break;
        }

        let platform_count = platform_counts
            .get(&reference.source_platform)
            .copied()
            .unwrap_or(0);
        if platform_count >= 3 {
            continue;
        }

        if let Some(cluster) = reference.niche_cluster.as_deref() {
            if cluster_counts.get(cluster).copied().unwrap_or(0) >= 2 {
                continue;
            }
        }

        *platform_counts
            .entry(reference.source_platform.clone())
            .or_insert(0) += 1;
        if let Some(cluster) = reference.niche_cluster.as_deref() {
            *cluster_counts.entry(cluster.to_string()).or_insert(0) += 1;
        }
        selected.push(reference.clone());
    }

    selected
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
        score += *influence.liked_tags.get(tag).unwrap_or(&0) as f64 * 0.4;
        score -= *influence.disliked_tags.get(tag).unwrap_or(&0) as f64 * 0.6;
    }

    if let Some(cluster) = reference.niche_cluster.as_deref() {
        score += *influence.liked_clusters.get(cluster).unwrap_or(&0) as f64 * 0.6;
        score -= *influence.disliked_clusters.get(cluster).unwrap_or(&0) as f64 * 0.8;
    }

    score += *influence
        .liked_platforms
        .get(&reference.source_platform)
        .unwrap_or(&0) as f64
        * 0.25;
    score -= *influence
        .disliked_platforms
        .get(&reference.source_platform)
        .unwrap_or(&0) as f64
        * 0.35;
    score += *influence
        .liked_visual_reference_ids
        .get(&reference.id)
        .unwrap_or(&0) as f64
        * 2.0;
    score -= *influence
        .disliked_visual_reference_ids
        .get(&reference.id)
        .unwrap_or(&0) as f64
        * 2.0;

    if reference.generation_use_count > 0 {
        score -= reference.generation_use_count as f64 * 0.25;
    }

    score
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
    if !value.is_empty() {
        *counts.entry(value.to_string()).or_insert(0) += 1;
    }
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
