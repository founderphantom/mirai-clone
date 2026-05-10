use crate::auth_client::{verify_session, AuthVerifyResponse};
use crate::db;
use crate::http::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;
use worker::{Request, Response, Result as WorkerResult, RouteContext};

#[derive(Debug, Deserialize)]
struct FlagOverrideRow {
    key: String,
    value: String,
}

#[derive(Debug, Serialize)]
struct TelemetryConfigResponse {
    flags: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct EventsRequest {
    events: Vec<EventInput>,
}

#[derive(Debug, Clone, Deserialize)]
struct EventInput {
    event: String,
    #[serde(default)]
    props: Map<String, Value>,
}

#[derive(Debug, Serialize)]
struct EventsResponse {
    ok: bool,
}

pub async fn telemetry_config(req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = optional_auth(&req, &ctx).await;
    let mut flags = default_flags();

    if let Some(auth) = auth {
        if let Ok(db) = ctx.env.d1("DB") {
            if let Ok(overrides) = load_flag_overrides(&db, &auth.user_id).await {
                for override_row in overrides {
                    flags.insert(override_row.key, parse_flag_value(&override_row.value));
                }
            }
        }
    }

    Response::from_json(&TelemetryConfigResponse { flags })
}

pub async fn telemetry_events(mut req: Request, ctx: RouteContext<()>) -> WorkerResult<Response> {
    let auth = optional_auth(&req, &ctx).await;
    // Read the raw body first so the cap is enforced even when Content-Length
    // is absent or spoofed (clients can omit the header entirely).
    const MAX_BODY_BYTES: usize = 16 * 1024;
    let body_bytes = match req.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => {
            return ApiError::bad_request("invalid_telemetry", "Could not read request body.")
                .to_response()
        }
    };
    if body_bytes.len() > MAX_BODY_BYTES {
        return ApiError::bad_request("payload_too_large", "Telemetry batch exceeds 16 KB.")
            .to_response();
    }
    let input = match serde_json::from_slice::<EventsRequest>(&body_bytes) {
        Ok(input) => input,
        Err(_) => {
            return ApiError::bad_request("invalid_telemetry", "Expected 1 to 20 telemetry events.")
                .to_response()
        }
    };
    if !events_are_valid(&input.events) {
        return ApiError::bad_request(
            "invalid_telemetry",
            "Expected 1 to 20 telemetry events with names up to 120 characters.",
        )
        .to_response();
    }

    if let Ok(db) = ctx.env.d1("DB") {
        let user_id = auth.as_ref().map(|auth| auth.user_id.as_str());
        let created_at = now_iso_string();
        for event in input.events {
            let props_json =
                serde_json::to_string(&event.props).unwrap_or_else(|_| "{}".to_string());
            let _ = db::exec(
                &db,
                r#"
                INSERT INTO app_events (id, user_id, event, props_json, created_at)
                VALUES (?, ?, ?, ?, ?)
                "#,
                vec![
                    json!(prefixed_id("evt")),
                    json!(user_id),
                    json!(event.event.trim()),
                    json!(props_json),
                    json!(created_at),
                ],
            )
            .await;
        }
    }

    Response::from_json(&EventsResponse { ok: true })
}

async fn optional_auth(req: &Request, ctx: &RouteContext<()>) -> Option<AuthVerifyResponse> {
    verify_session(ctx, req.headers()).await.ok().flatten()
}

async fn load_flag_overrides(
    db: &worker::D1Database,
    user_id: &str,
) -> WorkerResult<Vec<FlagOverrideRow>> {
    db::all(
        db,
        r#"
        SELECT key, value
        FROM feature_flag_overrides
        WHERE user_id = ?
        "#,
        vec![json!(user_id)],
    )
    .await
}

fn default_flags() -> Map<String, Value> {
    let mut flags = Map::new();
    flags.insert("mobileShell".to_string(), json!(true));
    flags.insert("onboardingInstagram".to_string(), json!(true));
    flags.insert("onboardingStarterSouls".to_string(), json!(true));
    flags.insert("blitzPreview".to_string(), json!(true));
    flags.insert("contextualPaywalls".to_string(), json!(false));
    flags
}

fn parse_flag_value(value: &str) -> Value {
    match value {
        "true" => json!(true),
        "false" => json!(false),
        _ => serde_json::from_str::<Value>(value).unwrap_or_else(|_| json!(value)),
    }
}

fn events_are_valid(events: &[EventInput]) -> bool {
    if events.is_empty() || events.len() > 20 {
        return false;
    }
    events.iter().all(|event| {
        // Validate trimmed name so padding spaces can't extend it past 120 chars.
        let name = event.event.trim();
        if name.is_empty() || name.chars().count() > 120 {
            return false;
        }
        // Cap individual prop keys to 64 chars to prevent wide-column abuse.
        if event.props.keys().any(|k| k.chars().count() > 64) {
            return false;
        }
        // Cap each props payload to 2 KB serialized to limit per-row storage.
        let props_size = serde_json::to_string(&event.props)
            .map(|json| json.len())
            .unwrap_or(usize::MAX);
        props_size <= 2048
    })
}

fn prefixed_id(prefix: &str) -> String {
    format!("{}_{}", prefix, Uuid::new_v4().simple())
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

#[cfg(test)]
mod tests {
    use super::{default_flags, events_are_valid, parse_flag_value, EventInput};
    use pretty_assertions::assert_eq;
    use serde_json::{json, Map};

    #[test]
    fn config_defaults_match_frontend_flags() {
        let flags = default_flags();

        assert_eq!(flags["mobileShell"], json!(true));
        assert_eq!(flags["onboardingInstagram"], json!(true));
        assert_eq!(flags["onboardingStarterSouls"], json!(true));
        assert_eq!(flags["blitzPreview"], json!(true));
        assert_eq!(flags["contextualPaywalls"], json!(false));
    }

    #[test]
    fn flag_overrides_parse_json_values() {
        assert_eq!(parse_flag_value("true"), json!(true));
        assert_eq!(parse_flag_value("false"), json!(false));
        assert_eq!(parse_flag_value("7"), json!(7));
        assert_eq!(parse_flag_value("custom"), json!("custom"));
    }

    #[test]
    fn event_batches_are_bounded() {
        let valid = EventInput {
            event: "screen_view".to_string(),
            props: Map::new(),
        };

        assert!(events_are_valid(&[valid]));
        assert!(!events_are_valid(&[]));

        // spaces-only name rejects even when len > 1
        assert!(!events_are_valid(&[EventInput {
            event: "   ".to_string(),
            props: Map::new(),
        }]));

        // exactly 120 trimmed chars passes
        assert!(events_are_valid(&[EventInput {
            event: "a".repeat(120),
            props: Map::new(),
        }]));
        // 121 trimmed chars fails
        assert!(!events_are_valid(&[EventInput {
            event: "a".repeat(121),
            props: Map::new(),
        }]));

        // batch limit: 20 passes, 21 fails
        let batch_ok: Vec<EventInput> = (0..20)
            .map(|i| EventInput {
                event: format!("event_{i}"),
                props: Map::new(),
            })
            .collect();
        assert!(events_are_valid(&batch_ok));
        let mut batch_too_many = batch_ok.clone();
        batch_too_many.push(EventInput {
            event: "one_more".to_string(),
            props: Map::new(),
        });
        assert!(!events_are_valid(&batch_too_many));

        // props value exceeding 2 KB fails
        let mut fat_props = Map::new();
        fat_props.insert("x".to_string(), json!("a".repeat(2049)));
        assert!(!events_are_valid(&[EventInput {
            event: "bloated".to_string(),
            props: fat_props,
        }]));

        // prop key exceeding 64 chars fails
        let mut wide_key_props = Map::new();
        wide_key_props.insert("k".repeat(65), json!("v"));
        assert!(!events_are_valid(&[EventInput {
            event: "wide_key".to_string(),
            props: wide_key_props,
        }]));

        // prop key exactly 64 chars passes
        let mut ok_key_props = Map::new();
        ok_key_props.insert("k".repeat(64), json!("v"));
        assert!(events_are_valid(&[EventInput {
            event: "ok_key".to_string(),
            props: ok_key_props,
        }]));
    }
}
