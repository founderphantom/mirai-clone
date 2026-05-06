CREATE TABLE IF NOT EXISTS feature_flag_overrides (
  user_id TEXT NOT NULL,
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (user_id, key)
);

CREATE TABLE IF NOT EXISTS app_events (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  event TEXT NOT NULL,
  props_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_app_events_user_created
ON app_events(user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_app_events_event_created
ON app_events(event, created_at DESC);
