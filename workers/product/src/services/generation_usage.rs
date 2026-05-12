use crate::db;
use crate::domain::blitz::daily_generation_limit;
use serde::{Deserialize, Serialize};
use serde_json::json;
use worker::{D1Database, Result as WorkerResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationUsageSnapshot {
    pub images_today: u32,
    pub daily_limit: u32,
    pub remaining: u32,
    pub limit_resets_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct UsageRow {
    images_generated: u32,
}

pub async fn usage_snapshot(
    db: &D1Database,
    user_id: &str,
    plan: &str,
    free_daily_limit: u32,
    pro_daily_limit: u32,
) -> WorkerResult<GenerationUsageSnapshot> {
    let usage_date = current_utc_date();
    let daily_limit = daily_generation_limit(plan, free_daily_limit, pro_daily_limit);
    let row = db::first::<UsageRow>(
        db,
        r#"
        SELECT images_generated
        FROM generation_daily_usage
        WHERE user_id = ?
          AND usage_date = ?
        "#,
        vec![json!(user_id), json!(usage_date)],
    )
    .await?;
    let images_today = row.map(|row| row.images_generated).unwrap_or(0);

    Ok(GenerationUsageSnapshot {
        images_today,
        daily_limit,
        remaining: daily_limit.saturating_sub(images_today),
        limit_resets_at: next_midnight_utc_iso(),
    })
}

pub async fn reserve_image(
    db: &D1Database,
    user_id: &str,
    plan: &str,
    free_daily_limit: u32,
    pro_daily_limit: u32,
) -> WorkerResult<bool> {
    let usage_date = current_utc_date();
    reserve_image_for_date(
        db,
        user_id,
        plan,
        free_daily_limit,
        pro_daily_limit,
        &usage_date,
    )
    .await
}

pub async fn reserve_image_for_date(
    db: &D1Database,
    user_id: &str,
    plan: &str,
    free_daily_limit: u32,
    pro_daily_limit: u32,
    usage_date: &str,
) -> WorkerResult<bool> {
    let daily_limit = daily_generation_limit(plan, free_daily_limit, pro_daily_limit);
    let now = now_iso_string();

    db::exec(
        db,
        r#"
        INSERT OR IGNORE INTO generation_daily_usage (
          user_id,
          usage_date,
          images_generated,
          images_limit,
          created_at,
          updated_at
        )
        VALUES (?, ?, 0, ?, ?, ?)
        "#,
        vec![
            json!(user_id),
            json!(usage_date),
            json!(daily_limit),
            json!(now),
            json!(now),
        ],
    )
    .await?;

    let result = db::run(
        db,
        r#"
        UPDATE generation_daily_usage
        SET images_generated = images_generated + 1,
            images_limit = ?,
            updated_at = ?
        WHERE user_id = ?
          AND usage_date = ?
          AND images_generated < ?
        "#,
        vec![
            json!(daily_limit),
            json!(now),
            json!(user_id),
            json!(usage_date),
            json!(daily_limit),
        ],
    )
    .await?;

    Ok(changed_rows(&result)?)
}

pub async fn refund_image(db: &D1Database, user_id: &str) -> WorkerResult<()> {
    let usage_date = current_utc_date();
    refund_image_for_date(db, user_id, &usage_date).await
}

pub async fn refund_image_for_date(
    db: &D1Database,
    user_id: &str,
    usage_date: &str,
) -> WorkerResult<()> {
    let now = now_iso_string();
    db::exec(
        db,
        r#"
        UPDATE generation_daily_usage
        SET images_generated = CASE
              WHEN images_generated > 0 THEN images_generated - 1
              ELSE 0
            END,
            updated_at = ?
        WHERE user_id = ?
          AND usage_date = ?
        "#,
        vec![json!(now), json!(user_id), json!(usage_date)],
    )
    .await
}

pub fn current_utc_date() -> String {
    now_iso_string()
        .split('T')
        .next()
        .unwrap_or_default()
        .to_string()
}

pub fn next_midnight_utc_iso() -> String {
    let current_date = current_utc_date();
    let midnight = js_sys::Date::parse(&format!("{current_date}T00:00:00.000Z"));
    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(midnight + 86_400_000.0))
        .to_iso_string()
        .into()
}

pub fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}

fn changed_rows(result: &worker::D1Result) -> WorkerResult<bool> {
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}
