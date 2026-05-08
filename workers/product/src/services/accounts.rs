use crate::db;
use serde::Serialize;
use serde_json::json;
use worker::{js_sys, D1Database, Result as WorkerResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    pub user_id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub plan: String,
    pub max_active_clones: u32,
    pub generation_priority: String,
    pub watermark_exports: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageLimits {
    pub active_clones: u32,
    pub max_active_clones: u32,
    pub plan: String,
}

pub fn account_usage_limits(identity: &VerifiedIdentity, active_clones: u32) -> UsageLimits {
    UsageLimits {
        active_clones,
        max_active_clones: identity.max_active_clones,
        plan: identity.plan.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntitlementSnapshot {
    pub max_active_clones: u32,
    pub generation_priority: String,
    pub watermark_exports: bool,
}

pub fn account_entitlement_snapshot(identity: &VerifiedIdentity) -> EntitlementSnapshot {
    EntitlementSnapshot {
        max_active_clones: identity.max_active_clones,
        generation_priority: identity.generation_priority.clone(),
        watermark_exports: identity.watermark_exports,
    }
}

pub async fn upsert_account_from_identity(
    db: &D1Database,
    identity: &VerifiedIdentity,
) -> WorkerResult<()> {
    let now = now_iso_string();
    let snapshot = account_entitlement_snapshot(identity);

    db::exec(
        db,
        r#"
        INSERT INTO accounts (
          user_id,
          email,
          display_name,
          plan,
          max_active_clones,
          generation_priority,
          watermark_exports,
          preferences_json,
          created_at,
          updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, '{}', ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
          email = COALESCE(excluded.email, accounts.email),
          display_name = COALESCE(excluded.display_name, accounts.display_name),
          plan = excluded.plan,
          max_active_clones = excluded.max_active_clones,
          generation_priority = excluded.generation_priority,
          watermark_exports = excluded.watermark_exports,
          updated_at = excluded.updated_at
        "#,
        vec![
            json!(identity.user_id),
            json!(identity.email),
            json!(identity.name),
            json!(identity.plan),
            json!(identity.max_active_clones),
            json!(snapshot.generation_priority),
            json!(snapshot.watermark_exports),
            json!(now),
            json!(now),
        ],
    )
    .await
}

fn now_iso_string() -> String {
    js_sys::Date::new_0().to_iso_string().into()
}
