use crate::domain::visual_reference::MoodboardBrief;
use crate::instagram_references::InstagramImageCandidate;
use serde::de::{self, Visitor};
use serde::Deserializer;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalVisualReferenceReview {
    pub decision: String,
    pub best_moodboard_slug: String,
    #[serde(deserialize_with = "deserialize_human_count")]
    pub human_count: u32,
    pub adult_likely: bool,
    pub age_unclear: bool,
    pub minor_likely: bool,
    pub youth_coded: bool,
    pub explicit: bool,
    #[serde(rename = "unsafe")]
    pub unsafe_content: bool,
    pub is_moodboard: bool,
    pub is_screenshot: bool,
    pub is_product_shot: bool,
    pub is_tutorial: bool,
    pub is_generic: bool,
    pub instagram_post_worthy: bool,
    pub editorial_composition_score: f64,
    pub real_pose_angle_score: f64,
    pub fashion_culture_cue_score: f64,
    pub lighting_color_direction_score: f64,
    pub moodboard_fit_score: f64,
    pub overall_reference_score: f64,
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
    #[serde(default)]
    pub color_palette: Vec<String>,
    #[serde(default)]
    pub fashion_culture_cues: Vec<String>,
    #[serde(default)]
    pub composition_notes: String,
    pub rejection_reason: Option<String>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcceptedGlobalVisualReview {
    pub moodboard_slug: String,
    pub editorial_composition_score: f64,
    pub real_pose_angle_score: f64,
    pub fashion_culture_cue_score: f64,
    pub lighting_color_direction_score: f64,
    pub moodboard_fit_score: f64,
    pub overall_reference_score: f64,
}

pub fn accept_global_visual_review(
    review: &GlobalVisualReferenceReview,
    active_moodboards: &[MoodboardBrief],
) -> Result<AcceptedGlobalVisualReview, &'static str> {
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
    if review.decision.trim().to_ascii_lowercase() != "approved" {
        return Err("not_approved");
    }

    for score in [
        review.editorial_composition_score,
        review.real_pose_angle_score,
        review.fashion_culture_cue_score,
        review.lighting_color_direction_score,
        review.moodboard_fit_score,
        review.overall_reference_score,
    ] {
        if !unit_score(score) {
            return Err("invalid_score");
        }
    }
    if review.moodboard_fit_score < 0.72 {
        return Err("weak_moodboard_fit");
    }
    if review.overall_reference_score < 0.70 {
        return Err("weak_overall_reference");
    }

    let high_quality_dimensions = [
        review.editorial_composition_score,
        review.real_pose_angle_score,
        review.fashion_culture_cue_score,
        review.lighting_color_direction_score,
    ]
    .into_iter()
    .filter(|score| *score >= 0.62)
    .count();
    if high_quality_dimensions < 2 {
        return Err("weak_soul2_quality");
    }

    let selected_slug = review.best_moodboard_slug.trim().to_ascii_lowercase();
    let Some(moodboard) = active_moodboards
        .iter()
        .find(|moodboard| moodboard.slug.trim().to_ascii_lowercase() == selected_slug)
    else {
        return Err("inactive_moodboard");
    };

    Ok(AcceptedGlobalVisualReview {
        moodboard_slug: moodboard.slug.clone(),
        editorial_composition_score: review.editorial_composition_score,
        real_pose_angle_score: review.real_pose_angle_score,
        fashion_culture_cue_score: review.fashion_culture_cue_score,
        lighting_color_direction_score: review.lighting_color_direction_score,
        moodboard_fit_score: review.moodboard_fit_score,
        overall_reference_score: review.overall_reference_score,
    })
}

pub fn global_visual_review_tags(review: &GlobalVisualReferenceReview) -> Vec<String> {
    let mut tags = Vec::new();

    push_tag(&mut tags, &review.pose);
    push_tag(&mut tags, &review.scene);
    push_tag(&mut tags, &review.lighting);
    push_tag(&mut tags, &review.framing);
    push_tag(&mut tags, &review.camera_feel);
    push_tag(&mut tags, &review.styling_direction);
    for tag in &review.color_palette {
        push_tag(&mut tags, tag);
    }
    for tag in &review.fashion_culture_cues {
        push_tag(&mut tags, tag);
    }

    tags
}

pub fn instagram_source_image_key(candidate: &InstagramImageCandidate) -> String {
    let post_identity = if candidate.source_post_id.trim().is_empty() {
        candidate.source_post_code.trim()
    } else {
        candidate.source_post_id.trim()
    };

    format!(
        "instagram:{}:{}",
        post_identity, candidate.source_image_index
    )
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
