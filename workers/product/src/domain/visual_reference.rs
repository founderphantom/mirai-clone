use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fmt;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoodboardBrief {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub vibe_summary: String,
    pub search_queries: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualReferenceReview {
    pub decision: String,
    #[serde(default)]
    pub best_moodboard_slug: String,
    #[serde(deserialize_with = "deserialize_human_count")]
    pub human_count: u32,
    pub adult_likely: bool,
    pub age_unclear: bool,
    pub minor_likely: bool,
    pub youth_coded: bool,
    pub revealing_fashion: bool,
    pub explicit: bool,
    #[serde(rename = "unsafe")]
    pub unsafe_content: bool,
    pub is_moodboard: bool,
    pub is_screenshot: bool,
    pub is_product_shot: bool,
    pub is_tutorial: bool,
    pub is_generic: bool,
    pub instagram_post_worthy: bool,
    pub visual_fit_score: f64,
    #[serde(default)]
    pub pose: String,
    #[serde(default)]
    pub scene: String,
    #[serde(default)]
    pub lighting: String,
    #[serde(default)]
    pub framing: String,
    #[serde(default)]
    pub camera_feel: String,
    #[serde(default)]
    pub styling_direction: String,
    pub rejection_reason: Option<String>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedVisualReview {
    pub moodboard_id: String,
    pub moodboard_slug: String,
    pub niche_cluster: String,
}

pub fn accept_visual_review(
    review: &VisualReferenceReview,
    selected_moodboards: &[MoodboardBrief],
) -> Result<AcceptedVisualReview, &'static str> {
    if review.human_count == 0 {
        return Err("no_human");
    }
    if review.human_count > 1 {
        return Err("multiple_humans");
    }
    if review.minor_likely {
        return Err("minor_likely");
    }
    if review.youth_coded {
        return Err("youth_coded");
    }
    if review.age_unclear {
        return Err("age_unclear");
    }
    if !review.adult_likely {
        return Err("adult_not_likely");
    }
    if review.explicit {
        return Err("explicit");
    }
    if review.unsafe_content {
        return Err("unsafe");
    }
    if review.is_moodboard {
        return Err("moodboard");
    }
    if review.is_screenshot {
        return Err("screenshot");
    }
    if review.is_product_shot {
        return Err("product_shot");
    }
    if review.is_tutorial {
        return Err("tutorial");
    }
    if review.is_generic {
        return Err("generic");
    }
    if !review.instagram_post_worthy {
        return Err("not_instagram_post_worthy");
    }
    if !unit_score(review.visual_fit_score) || review.visual_fit_score < 0.72 {
        return Err("weak_visual_fit");
    }
    if review.decision.trim().to_ascii_lowercase() != "approved" {
        return Err("not_approved");
    }

    let selected_slug = review.best_moodboard_slug.trim().to_ascii_lowercase();
    let Some(moodboard) = selected_moodboards
        .iter()
        .find(|moodboard| moodboard.slug.trim().to_ascii_lowercase() == selected_slug)
    else {
        return Err("unselected_moodboard");
    };

    Ok(AcceptedVisualReview {
        moodboard_id: moodboard.id.clone(),
        moodboard_slug: moodboard.slug.clone(),
        niche_cluster: moodboard.slug.clone(),
    })
}

pub fn visual_review_tags(review: &VisualReferenceReview) -> Vec<String> {
    let mut tags = Vec::new();

    push_tag(&mut tags, &review.pose);
    push_tag(&mut tags, &review.scene);
    push_tag(&mut tags, &review.lighting);
    push_tag(&mut tags, &review.framing);
    push_tag(&mut tags, &review.camera_feel);
    push_tag(&mut tags, &review.styling_direction);

    tags
}

pub fn selected_moodboard_count_is_valid(count: usize) -> bool {
    (1..=10).contains(&count)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualCandidateForRanking {
    pub id: String,
    pub discovered_via: String,
    pub moodboard_slug: String,
    pub source_handle: String,
    pub media_type: u8,
    pub like_count: Option<u64>,
    pub comment_count: Option<u64>,
    pub source_published_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateDiversityCaps {
    pub review_limit: usize,
    pub per_handle_review_cap: usize,
    pub per_moodboard_review_cap: usize,
}

pub fn rank_candidates_for_review(
    mut candidates: Vec<VisualCandidateForRanking>,
    caps: &CandidateDiversityCaps,
) -> Vec<VisualCandidateForRanking> {
    candidates.sort_by(|left, right| {
        candidate_score(right)
            .cmp(&candidate_score(left))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut ranked = Vec::new();
    let mut handle_counts: HashMap<String, usize> = HashMap::new();
    let mut moodboard_counts: HashMap<String, usize> = HashMap::new();

    for candidate in candidates {
        if ranked.len() >= caps.review_limit {
            break;
        }

        let handle_key = ranking_key(&candidate.source_handle);
        let moodboard_key = ranking_key(&candidate.moodboard_slug);
        let handle_count = handle_counts.get(&handle_key).copied().unwrap_or_default();
        let moodboard_count = moodboard_counts
            .get(&moodboard_key)
            .copied()
            .unwrap_or_default();

        if handle_count >= caps.per_handle_review_cap {
            continue;
        }
        if moodboard_count >= caps.per_moodboard_review_cap {
            continue;
        }

        *handle_counts.entry(handle_key).or_default() += 1;
        *moodboard_counts.entry(moodboard_key).or_default() += 1;
        ranked.push(candidate);
    }

    ranked
}

fn candidate_score(candidate: &VisualCandidateForRanking) -> (u16, u16, u64, i64) {
    (
        media_type_score(candidate.media_type),
        discovered_via_score(&candidate.discovered_via),
        engagement_score(candidate.like_count, candidate.comment_count),
        recency_score(candidate.source_published_at.as_deref()),
    )
}

fn media_type_score(media_type: u8) -> u16 {
    match media_type {
        1 => 300,
        8 => 200,
        2 => 100,
        _ => 0,
    }
}

fn discovered_via_score(discovered_via: &str) -> u16 {
    match discovered_via.trim().to_ascii_lowercase().as_str() {
        "reels_owner" => 220,
        "learned_related" => 210,
        "accepted_handle" => 200,
        "configured_handle" => 150,
        "related_profile" => 50,
        _ => 0,
    }
}

fn engagement_score(like_count: Option<u64>, comment_count: Option<u64>) -> u64 {
    like_count
        .unwrap_or_default()
        .saturating_add(comment_count.unwrap_or_default().saturating_mul(2))
}

fn recency_score(source_published_at: Option<&str>) -> i64 {
    let Some(value) = source_published_at.map(str::trim) else {
        return 0;
    };

    OffsetDateTime::parse(value, &Rfc3339)
        .map(|timestamp| timestamp.unix_timestamp())
        .unwrap_or_default()
}

fn ranking_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn push_tag(tags: &mut Vec<String>, value: &str) {
    let tag = value.trim();
    let key = tag.to_ascii_lowercase();
    if !tag.is_empty()
        && !tags
            .iter()
            .any(|existing| existing.to_ascii_lowercase() == key)
    {
        tags.push(tag.to_string());
    }
}

fn unit_score(score: f64) -> bool {
    score.is_finite() && (0.0..=1.0).contains(&score)
}

fn deserialize_human_count<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    struct HumanCountVisitor;

    impl Visitor<'_> for HumanCountVisitor {
        type Value = u32;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a non-negative integral human count within u32 range")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            u32::try_from(value).map_err(|_| E::custom("human count out of range"))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value < 0 {
                return Err(E::custom("human count must be non-negative"));
            }
            u32::try_from(value).map_err(|_| E::custom("human count out of range"))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > u32::MAX as f64
            {
                return Err(E::custom(
                    "human count must be an integral value within u32 range",
                ));
            }

            Ok(value as u32)
        }
    }

    deserializer.deserialize_any(HumanCountVisitor)
}
