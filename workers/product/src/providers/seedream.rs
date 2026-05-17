use serde_json::{json, Value};

pub const SEEDREAM_CLEANUP_MODEL: &str = "seedream_5_lite";

pub fn cleanup_prompt() -> &'static str {
    "Remove only the visible text from this image. Keep every non-text part of the image exactly the same."
}

pub fn seedream_cleanup_arguments(uploaded_reference_value: &str) -> Value {
    json!({
        "params": {
            "model": SEEDREAM_CLEANUP_MODEL,
            "prompt": cleanup_prompt(),
            "medias": [{
                "value": uploaded_reference_value,
                "role": "image"
            }],
            "count": 1
        }
    })
}

pub fn seedream_cleanup_arguments_with_model(uploaded_reference_value: &str, model: &str) -> Value {
    let mut arguments = seedream_cleanup_arguments(uploaded_reference_value);
    if let Some(params) = arguments.get_mut("params").and_then(Value::as_object_mut) {
        params.insert("model".to_string(), json!(model.trim()));
    }
    arguments
}

pub fn extract_seedream_cleaned_image_url(raw_json: &Value) -> Option<String> {
    provider_payloads(raw_json)
        .iter()
        .find_map(cleaned_image_url_from_payload)
}

fn cleaned_image_url_from_payload(payload: &Value) -> Option<String> {
    for path in [
        "/result/url",
        "/result/image_url",
        "/result/imageUrl",
        "/result/output_url",
        "/result/outputUrl",
        "/result/images/0/url",
        "/result/images/0/image_url",
        "/result/images/0/imageUrl",
        "/url",
        "/image_url",
        "/imageUrl",
        "/output_url",
        "/outputUrl",
        "/images/0/url",
    ] {
        if let Some(url) = json_string_at(payload, path).filter(|url| url.starts_with("http")) {
            return Some(url);
        }
    }
    None
}

fn provider_payloads(raw_json: &Value) -> Vec<Value> {
    let mut payloads = vec![raw_json.clone()];
    collect_text_payloads(raw_json, &mut payloads);
    payloads
}

fn collect_text_payloads(value: &Value, payloads: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                    collect_text_payloads(&parsed, payloads);
                    payloads.push(parsed);
                }
            }
            for child in map.values() {
                collect_text_payloads(child, payloads);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text_payloads(item, payloads);
            }
        }
        _ => {}
    }
}

fn json_string_at(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
