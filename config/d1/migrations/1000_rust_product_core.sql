PRAGMA foreign_keys = ON;

DROP TABLE IF EXISTS user_inspiration_pool;
DROP TABLE IF EXISTS visual_references;
DROP TABLE IF EXISTS visual_reference_candidates;
DROP TABLE IF EXISTS niche_research_queries;
DROP TABLE IF EXISTS niche_knowledge;
DROP TABLE IF EXISTS inspiration_bubbles;
DROP TABLE IF EXISTS generation_outputs;
DROP TABLE IF EXISTS generation_jobs;
DROP TABLE IF EXISTS soul_training_jobs;
DROP TABLE IF EXISTS clone_reference_assets;
DROP TABLE IF EXISTS instagram_harvest_jobs;
DROP TABLE IF EXISTS starter_characters;
DROP TABLE IF EXISTS media_assets;
DROP TABLE IF EXISTS discovery_items;
DROP TABLE IF EXISTS discovery_sources;
DROP TABLE IF EXISTS billing_events;
DROP TABLE IF EXISTS credit_ledger;
DROP TABLE IF EXISTS ai_model_invocations;
DROP TABLE IF EXISTS provider_account_leases;
DROP TABLE IF EXISTS provider_accounts;
DROP TABLE IF EXISTS deletion_jobs;
DROP TABLE IF EXISTS clone_profiles;

CREATE TABLE IF NOT EXISTS accounts (
  user_id TEXT PRIMARY KEY,
  email TEXT,
  display_name TEXT,
  plan TEXT NOT NULL DEFAULT 'free',
  max_active_clones INTEGER NOT NULL DEFAULT 1,
  generation_priority TEXT NOT NULL DEFAULT 'standard',
  watermark_exports INTEGER NOT NULL DEFAULT 1,
  polar_customer_id TEXT,
  polar_subscription_id TEXT,
  deletion_status TEXT NOT NULL DEFAULT 'active',
  preferences_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);

CREATE TABLE IF NOT EXISTS clone_profiles (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  handle TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'manual_upload',
  status TEXT NOT NULL DEFAULT 'active',
  soul_status TEXT NOT NULL DEFAULT 'queued',
  provider TEXT NOT NULL DEFAULT 'higgsfield',
  provider_soul_id TEXT,
  provider_config_json TEXT NOT NULL DEFAULT '{}',
  reference_count_total INTEGER NOT NULL DEFAULT 0,
  reference_count_training_selected INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT,
  UNIQUE(user_id, handle)
);

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
  deleted_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS clone_reference_assets (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  media_asset_id TEXT NOT NULL,
  sort_order INTEGER NOT NULL,
  role TEXT NOT NULL DEFAULT 'identity',
  eligibility_status TEXT NOT NULL DEFAULT 'accepted',
  quality_score REAL,
  variety_tags_json TEXT NOT NULL DEFAULT '[]',
  training_selected INTEGER NOT NULL DEFAULT 1,
  rejection_reason TEXT,
  audit_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE CASCADE,
  UNIQUE(clone_id, media_asset_id)
);

CREATE TABLE IF NOT EXISTS soul_training_jobs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  provider TEXT NOT NULL DEFAULT 'higgsfield',
  provider_account_id TEXT,
  provider_job_id TEXT,
  status TEXT NOT NULL DEFAULT 'queued',
  idempotency_key TEXT NOT NULL UNIQUE,
  reference_count INTEGER NOT NULL,
  request_json TEXT NOT NULL DEFAULT '{}',
  response_json TEXT NOT NULL DEFAULT '{}',
  error_code TEXT,
  error_message TEXT,
  queued_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS provider_accounts (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  label TEXT NOT NULL,
  plan TEXT,
  capabilities_json TEXT NOT NULL DEFAULT '[]',
  health_state TEXT NOT NULL DEFAULT 'healthy',
  capacity_json TEXT NOT NULL DEFAULT '{}',
  secret_refs_json TEXT NOT NULL DEFAULT '{}',
  last_auth_check_at TEXT,
  last_successful_job_at TEXT,
  disabled_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS provider_account_leases (
  id TEXT PRIMARY KEY,
  provider_account_id TEXT NOT NULL,
  job_type TEXT NOT NULL,
  job_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  lease_expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  released_at TEXT,
  FOREIGN KEY (provider_account_id) REFERENCES provider_accounts(id) ON DELETE CASCADE,
  UNIQUE(job_type, job_id)
);

CREATE TABLE IF NOT EXISTS generation_jobs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  input_visual_reference_id TEXT,
  input_media_asset_id TEXT,
  provider TEXT NOT NULL DEFAULT 'higgsfield',
  provider_account_id TEXT,
  provider_job_ids_json TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL DEFAULT 'queued',
  prompt TEXT,
  aspect_ratio TEXT,
  quality TEXT,
  request_json TEXT NOT NULL DEFAULT '{}',
  response_json TEXT NOT NULL DEFAULT '{}',
  error_code TEXT,
  error_message TEXT,
  queued_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS generation_outputs (
  id TEXT PRIMARY KEY,
  job_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  media_asset_id TEXT,
  provider_asset_id TEXT,
  raw_url TEXT,
  output_index INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (job_id) REFERENCES generation_jobs(id) ON DELETE CASCADE,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS credit_ledger (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  entry_type TEXT NOT NULL,
  amount INTEGER NOT NULL,
  balance_after INTEGER,
  related_job_type TEXT,
  related_job_id TEXT,
  idempotency_key TEXT NOT NULL UNIQUE,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS billing_events (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  event_type TEXT NOT NULL,
  provider TEXT NOT NULL DEFAULT 'polar',
  external_event_id TEXT,
  polar_customer_id TEXT,
  polar_subscription_id TEXT,
  polar_product_id TEXT,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_model_invocations (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  task TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  input_hash TEXT,
  status TEXT NOT NULL,
  latency_ms INTEGER,
  cost_estimate_micros INTEGER,
  result_json TEXT NOT NULL DEFAULT '{}',
  error_message TEXT,
  trace_id TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS inspiration_bubbles (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT,
  slug TEXT NOT NULL,
  title TEXT NOT NULL,
  vibe_summary TEXT NOT NULL DEFAULT '',
  search_queries_json TEXT NOT NULL DEFAULT '[]',
  selected INTEGER NOT NULL DEFAULT 0,
  weight REAL NOT NULL DEFAULT 1,
  sort_order INTEGER NOT NULL DEFAULT 0,
  source TEXT NOT NULL DEFAULT 'default',
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS niche_research_queries (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  bubble_id TEXT,
  query TEXT NOT NULL,
  source TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'new',
  cluster TEXT,
  created_at TEXT NOT NULL,
  used_at TEXT,
  FOREIGN KEY (bubble_id) REFERENCES inspiration_bubbles(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS niche_knowledge (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  bit TEXT NOT NULL,
  cluster TEXT,
  source_platform TEXT,
  source_url TEXT,
  score REAL NOT NULL DEFAULT 1,
  raw_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

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
  thumbnail_url TEXT,
  image_url TEXT,
  source_url TEXT,
  metrics_json TEXT NOT NULL DEFAULT '{}',
  raw_json TEXT NOT NULL DEFAULT '{}',
  discovered_at TEXT NOT NULL,
  expires_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (source_id) REFERENCES discovery_sources(id) ON DELETE CASCADE,
  UNIQUE(platform, external_id)
);

CREATE TABLE IF NOT EXISTS visual_reference_candidates (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  discovery_item_id TEXT,
  source_platform TEXT NOT NULL,
  source_url TEXT,
  image_url TEXT,
  thumbnail_media_asset_id TEXT,
  human_presence_status TEXT NOT NULL DEFAULT 'unreviewed',
  human_presence_score REAL,
  rejection_reason TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  reviewed_at TEXT,
  FOREIGN KEY (discovery_item_id) REFERENCES discovery_items(id) ON DELETE SET NULL,
  FOREIGN KEY (thumbnail_media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS visual_references (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  candidate_id TEXT,
  media_asset_id TEXT,
  source_platform TEXT NOT NULL,
  source_url TEXT,
  aesthetic_tags_json TEXT NOT NULL DEFAULT '[]',
  human_presence_type TEXT NOT NULL,
  human_presence_score REAL NOT NULL,
  moderation_level INTEGER NOT NULL DEFAULT 4,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  FOREIGN KEY (candidate_id) REFERENCES visual_reference_candidates(id) ON DELETE SET NULL,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS user_inspiration_pool (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  bubble_id TEXT,
  visual_reference_id TEXT,
  discovery_item_id TEXT,
  score REAL NOT NULL DEFAULT 1,
  used_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (bubble_id) REFERENCES inspiration_bubbles(id) ON DELETE SET NULL,
  FOREIGN KEY (visual_reference_id) REFERENCES visual_references(id) ON DELETE CASCADE,
  FOREIGN KEY (discovery_item_id) REFERENCES discovery_items(id) ON DELETE CASCADE,
  UNIQUE(user_id, visual_reference_id),
  UNIQUE(user_id, discovery_item_id)
);

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

CREATE TABLE IF NOT EXISTS deletion_jobs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'queued',
  cursor_json TEXT NOT NULL DEFAULT '{}',
  error_message TEXT,
  queued_at TEXT NOT NULL,
  completed_at TEXT,
  updated_at TEXT NOT NULL
);
