use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

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
