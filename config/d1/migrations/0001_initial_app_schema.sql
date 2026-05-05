PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS clone_profiles (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  name TEXT NOT NULL,
  handle TEXT NOT NULL,
  persona TEXT NOT NULL DEFAULT '',
  voice TEXT NOT NULL DEFAULT '',
  style_prompt TEXT NOT NULL DEFAULT '',
  default_provider TEXT NOT NULL DEFAULT 'higgsfield',
  provider_config_json TEXT NOT NULL DEFAULT '{}',
  visibility TEXT NOT NULL DEFAULT 'private',
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_clone_profiles_user_handle
ON clone_profiles(user_id, handle);

CREATE INDEX IF NOT EXISTS idx_clone_profiles_user_status
ON clone_profiles(user_id, status);

CREATE TABLE IF NOT EXISTS media_assets (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT,
  kind TEXT NOT NULL,
  source TEXT NOT NULL,
  storage_key TEXT,
  content_type TEXT,
  bytes INTEGER,
  width INTEGER,
  height INTEGER,
  remote_url TEXT,
  sha256 TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_media_assets_user_kind
ON media_assets(user_id, kind, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_media_assets_clone
ON media_assets(clone_id, created_at DESC);

CREATE TABLE IF NOT EXISTS clone_reference_assets (
  id TEXT PRIMARY KEY,
  clone_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  media_asset_id TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'style',
  label TEXT NOT NULL DEFAULT '',
  weight REAL NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_clone_reference_assets_clone
ON clone_reference_assets(clone_id, created_at DESC);

CREATE TABLE IF NOT EXISTS discovery_sources (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  source TEXT NOT NULL,
  params_json TEXT NOT NULL DEFAULT '{}',
  refreshed_at TEXT,
  expires_at TEXT,
  status TEXT NOT NULL DEFAULT 'stale',
  UNIQUE(provider, source, params_json)
);

CREATE TABLE IF NOT EXISTS discovery_items (
  id TEXT PRIMARY KEY,
  source_id TEXT NOT NULL,
  external_id TEXT NOT NULL,
  platform TEXT NOT NULL,
  media_type TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  author_handle TEXT NOT NULL DEFAULT '',
  author_name TEXT NOT NULL DEFAULT '',
  thumbnail_url TEXT,
  image_url TEXT,
  source_url TEXT,
  metrics_json TEXT NOT NULL DEFAULT '{}',
  raw_json TEXT NOT NULL DEFAULT '{}',
  discovered_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (source_id) REFERENCES discovery_sources(id) ON DELETE CASCADE,
  UNIQUE(platform, external_id)
);

CREATE INDEX IF NOT EXISTS idx_discovery_items_source
ON discovery_items(source_id, discovered_at DESC);

CREATE INDEX IF NOT EXISTS idx_discovery_items_expires
ON discovery_items(expires_at);

CREATE TABLE IF NOT EXISTS generation_jobs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  provider TEXT NOT NULL DEFAULT 'higgsfield',
  provider_job_ids_json TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL DEFAULT 'queued',
  mode TEXT NOT NULL DEFAULT 'image',
  prompt TEXT NOT NULL DEFAULT '',
  input_asset_id TEXT,
  inspiration_discovery_item_id TEXT,
  aspect_ratio TEXT,
  quality TEXT NOT NULL DEFAULT '1080p',
  batch_size INTEGER NOT NULL DEFAULT 4,
  request_json TEXT NOT NULL DEFAULT '{}',
  provider_payload_json TEXT NOT NULL DEFAULT '{}',
  error_message TEXT,
  queued_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (input_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL,
  FOREIGN KEY (inspiration_discovery_item_id) REFERENCES discovery_items(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_generation_jobs_user
ON generation_jobs(user_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_generation_jobs_clone
ON generation_jobs(clone_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_generation_jobs_status
ON generation_jobs(status, updated_at);

CREATE TABLE IF NOT EXISTS generation_outputs (
  id TEXT PRIMARY KEY,
  job_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  provider_asset_id TEXT,
  media_asset_id TEXT,
  share_url TEXT,
  raw_url TEXT,
  output_index INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (job_id) REFERENCES generation_jobs(id) ON DELETE CASCADE,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_generation_outputs_job
ON generation_outputs(job_id, output_index);

CREATE TABLE IF NOT EXISTS billing_events (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  event_type TEXT NOT NULL,
  polar_customer_id TEXT,
  polar_subscription_id TEXT,
  polar_product_id TEXT,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_billing_events_user
ON billing_events(user_id, created_at DESC);
