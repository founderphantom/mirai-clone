use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use worker::{Ai, Error, Result as WorkerResult};

pub const KIMI_K2_6_MODEL: &str = "@cf/moonshotai/kimi-k2.6";

#[derive(Debug, Serialize)]
pub struct WorkersAiInput<'a> {
    pub messages: Vec<WorkersAiMessage<'a>>,
    pub response_format: WorkersAiResponseFormat,
    pub temperature: f32,
}

#[derive(Debug, Serialize)]
pub struct WorkersAiMessage<'a> {
    pub role: &'static str,
    pub content: WorkersAiMessageContent<'a>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum WorkersAiMessageContent<'a> {
    Text(&'a str),
    Parts(Vec<WorkersAiContentPart<'a>>),
}

#[derive(Debug, Serialize)]
pub struct WorkersAiContentPart<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<WorkersAiImageUrl<'a>>,
}

#[derive(Debug, Serialize)]
pub struct WorkersAiImageUrl<'a> {
    pub url: &'a str,
}

#[derive(Debug, Serialize)]
pub struct WorkersAiResponseFormat {
    #[serde(rename = "type")]
    pub kind: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct WorkersAiTextResponse {
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub response: Option<String>,
}

pub async fn run_text_json<T: DeserializeOwned>(ai: &Ai, prompt: &str) -> WorkerResult<T> {
    let input = WorkersAiInput {
        messages: vec![WorkersAiMessage {
            role: "user",
            content: WorkersAiMessageContent::Text(prompt),
        }],
        response_format: WorkersAiResponseFormat {
            kind: "json_object",
        },
        temperature: 0.2,
    };

    let response = ai
        .run::<_, WorkersAiTextResponse>(KIMI_K2_6_MODEL, input)
        .await?;
    decode_structured_response(response)
}

pub async fn run_vision_json<T: DeserializeOwned>(
    ai: &Ai,
    prompt: &str,
    image_url: &str,
) -> WorkerResult<T> {
    let input = WorkersAiInput {
        messages: vec![WorkersAiMessage {
            role: "user",
            content: WorkersAiMessageContent::Parts(vec![
                WorkersAiContentPart {
                    kind: "text",
                    text: Some(prompt),
                    image_url: None,
                },
                WorkersAiContentPart {
                    kind: "image_url",
                    text: None,
                    image_url: Some(WorkersAiImageUrl { url: image_url }),
                },
            ]),
        }],
        response_format: WorkersAiResponseFormat {
            kind: "json_object",
        },
        temperature: 0.1,
    };

    let response = ai
        .run::<_, WorkersAiTextResponse>(KIMI_K2_6_MODEL, input)
        .await?;
    decode_structured_response(response)
}

pub fn decode_structured_response<T: DeserializeOwned>(
    response: WorkersAiTextResponse,
) -> WorkerResult<T> {
    if let Some(result) = response.result {
        return serde_json::from_value(result).map_err(|error| {
            Error::RustError(format!("failed to decode workers ai result: {error}"))
        });
    }

    if let Some(response_text) = response.response {
        return serde_json::from_str(&response_text).map_err(|error| {
            Error::RustError(format!("failed to decode workers ai response: {error}"))
        });
    }

    Err(Error::RustError(
        "workers ai response did not include result or response".to_string(),
    ))
}

pub fn seed_extraction_prompt(niche: &str, excluded_terms: &[String]) -> String {
    format!(
        r#"Extract fresh discovery seeds for the niche "{niche}".

Research current TikTok and Instagram creator behavior. Return strict JSON with a "seeds" array of concise search terms, each with "term", "platform", and "reason".

Guardrails:
- Prefer organic creator content, outfit posts, routines, location cues, and visible real-world aesthetics.
- Do not include synthetic/generation topics, AI art prompts, render packs, CGI references, face-swap terms, or model-generation language.
- Avoid duplicates, brand-only terms, and terms that are too broad to search.
- Exclude these terms if they appear or are close variants: {excluded_terms}.
"#,
        niche = niche.trim(),
        excluded_terms = excluded_terms.join(", ")
    )
}

pub fn knowledge_extraction_prompt(niche: &str) -> String {
    format!(
        r#"Extract durable research knowledge for the niche "{niche}".

Return strict JSON with "signals", "avoid", and "source_notes" arrays. Focus on current TikTok and Instagram evidence from organic creator content and visual behavior.

Guardrails:
- Do not extract from known-stale source items, repost farms, synthetic/generation showcases, prompt galleries, render_like examples, or content without a real creator context.
- Preserve only observations that help select real photos, human presence, styling, setting, pose, lighting, and composition.
- Flag uncertainty instead of inventing facts.
"#,
        niche = niche.trim()
    )
}

pub fn clustering_prompt(niche: &str, seeds_json: &str) -> String {
    format!(
        r#"Cluster discovery seeds for the niche "{niche}".

Input seeds JSON:
{seeds_json}

Return strict JSON with a "clusters" array. Each cluster must include "label", "terms", "intent", and "visual_criteria".

Guardrails:
- Keep TikTok and Instagram organic-photo discovery separate from synthetic/generation topics.
- Reject clusters centered on AI renders, generated people, CGI, prompt packs, or render_like material.
- Favor clusters that can produce real creator references with one clear aesthetic signal.
"#,
        niche = niche.trim(),
        seeds_json = seeds_json.trim()
    )
}

pub fn human_presence_prompt() -> String {
    r#"Review the image for human-presence suitability.

Return strict JSON with:
{
  "accepted": boolean,
  "reason": string,
  "human_count": number,
  "render_like": boolean,
  "organic_photo": boolean
}

Accept only organic creator content with exactly one human person clearly visible. Reject images with zero people, multiple people, mannequins, illustrations, synthetic/generation artifacts, render_like lighting or skin, CGI, heavy face distortion, or product-only compositions.
"#
    .to_string()
}
