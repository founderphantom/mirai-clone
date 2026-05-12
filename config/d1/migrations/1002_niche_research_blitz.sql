PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS blitz_swipes;
DROP TABLE IF EXISTS generation_outputs;
DROP TABLE IF EXISTS generation_jobs;
DROP TABLE IF EXISTS user_inspiration_pool;
DROP TABLE IF EXISTS visual_references;
DROP TABLE IF EXISTS visual_reference_candidates;
DROP TABLE IF EXISTS niche_knowledge;
DROP TABLE IF EXISTS niche_research_queries;
DROP TABLE IF EXISTS inspiration_bubbles;
DROP TABLE IF EXISTS discovery_items;
DROP TABLE IF EXISTS discovery_sources;
DROP TABLE IF EXISTS blitz_batches;
DROP TABLE IF EXISTS generation_daily_usage;
DROP TABLE IF EXISTS blitz_config;

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
  source_published_at TEXT,
  metrics_json TEXT NOT NULL DEFAULT '{}',
  raw_json TEXT NOT NULL DEFAULT '{}',
  discovered_at TEXT NOT NULL,
  expires_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (source_id) REFERENCES discovery_sources(id) ON DELETE CASCADE,
  UNIQUE(source_id, platform, external_id)
);

CREATE TABLE IF NOT EXISTS inspiration_bubbles (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
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
  clone_id TEXT NOT NULL,
  bubble_id TEXT,
  query TEXT NOT NULL,
  source TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'new',
  cluster TEXT,
  cluster_relevance_score REAL,
  cluster_relevance_reason TEXT,
  raw_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  used_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (bubble_id) REFERENCES inspiration_bubbles(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS niche_knowledge (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  clone_id TEXT NOT NULL,
  bit TEXT NOT NULL,
  cluster TEXT,
  cluster_relevance_score REAL,
  cluster_relevance_reason TEXT,
  source_platform TEXT,
  source_url TEXT,
  score REAL NOT NULL DEFAULT 1,
  raw_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS blitz_batches (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  batch_number INTEGER NOT NULL,
  batch_size INTEGER NOT NULL DEFAULT 5,
  status TEXT NOT NULL DEFAULT 'pending',
  influence_json TEXT NOT NULL DEFAULT '{}',
  generation_count INTEGER NOT NULL DEFAULT 0,
  like_count INTEGER NOT NULL DEFAULT 0,
  dislike_count INTEGER NOT NULL DEFAULT 0,
  error_code TEXT,
  error_message TEXT,
  created_at TEXT NOT NULL,
  ready_at TEXT,
  served_at TEXT,
  completed_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  UNIQUE(clone_id, batch_number)
);

CREATE TABLE IF NOT EXISTS generation_jobs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  blitz_batch_id TEXT,
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
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (blitz_batch_id) REFERENCES blitz_batches(id) ON DELETE SET NULL,
  FOREIGN KEY (input_visual_reference_id) REFERENCES visual_references(id) ON DELETE SET NULL
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

CREATE TABLE IF NOT EXISTS visual_reference_candidates (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  clone_id TEXT NOT NULL,
  discovery_item_id TEXT,
  source_platform TEXT NOT NULL,
  source_url TEXT,
  source_published_at TEXT,
  freshness_status TEXT NOT NULL DEFAULT 'unreviewed',
  image_url TEXT,
  thumbnail_media_asset_id TEXT,
  human_presence_status TEXT NOT NULL DEFAULT 'unreviewed',
  human_presence_score REAL,
  organic_photo_score REAL,
  freshness_visual_score REAL,
  capture_style TEXT,
  niche_cluster TEXT,
  rejection_reason TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  reviewed_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (discovery_item_id) REFERENCES discovery_items(id) ON DELETE SET NULL,
  FOREIGN KEY (thumbnail_media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS visual_references (
  id TEXT PRIMARY KEY,
  user_id TEXT,
  clone_id TEXT NOT NULL,
  candidate_id TEXT,
  media_asset_id TEXT,
  source_platform TEXT NOT NULL,
  source_url TEXT,
  source_published_at TEXT,
  generation_use_count INTEGER NOT NULL DEFAULT 0,
  last_used_batch_id TEXT,
  last_liked_at TEXT,
  aesthetic_tags_json TEXT NOT NULL DEFAULT '[]',
  niche_cluster TEXT,
  human_presence_type TEXT NOT NULL,
  human_presence_score REAL NOT NULL,
  organic_photo_score REAL NOT NULL DEFAULT 0,
  freshness_visual_score REAL NOT NULL DEFAULT 0,
  moderation_level INTEGER NOT NULL DEFAULT 4,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (candidate_id) REFERENCES visual_reference_candidates(id) ON DELETE SET NULL,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE SET NULL,
  FOREIGN KEY (last_used_batch_id) REFERENCES blitz_batches(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS user_inspiration_pool (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  bubble_id TEXT,
  visual_reference_id TEXT,
  discovery_item_id TEXT,
  score REAL NOT NULL DEFAULT 1,
  used_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (bubble_id) REFERENCES inspiration_bubbles(id) ON DELETE SET NULL,
  FOREIGN KEY (visual_reference_id) REFERENCES visual_references(id) ON DELETE CASCADE,
  FOREIGN KEY (discovery_item_id) REFERENCES discovery_items(id) ON DELETE CASCADE,
  UNIQUE(clone_id, visual_reference_id),
  UNIQUE(clone_id, discovery_item_id)
);

CREATE TABLE IF NOT EXISTS blitz_swipes (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  batch_id TEXT NOT NULL,
  generation_output_id TEXT,
  visual_reference_id TEXT,
  action TEXT NOT NULL,
  output_metadata_json TEXT NOT NULL DEFAULT '{}',
  swipe_index INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (batch_id) REFERENCES blitz_batches(id) ON DELETE CASCADE,
  FOREIGN KEY (generation_output_id) REFERENCES generation_outputs(id) ON DELETE SET NULL,
  FOREIGN KEY (visual_reference_id) REFERENCES visual_references(id) ON DELETE SET NULL,
  UNIQUE(batch_id, swipe_index),
  UNIQUE(batch_id, generation_output_id)
);

CREATE TABLE IF NOT EXISTS generation_daily_usage (
  user_id TEXT NOT NULL,
  usage_date TEXT NOT NULL,
  images_generated INTEGER NOT NULL DEFAULT 0,
  images_limit INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (user_id, usage_date)
);

CREATE TABLE IF NOT EXISTS blitz_config (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

INSERT INTO blitz_config (key, value, updated_at) VALUES
  ('batch_size', '5', '2026-05-11T00:00:00.000Z'),
  ('free_daily_limit', '10', '2026-05-11T00:00:00.000Z'),
  ('pro_daily_limit', '50', '2026-05-11T00:00:00.000Z'),
  ('influence_window', '5', '2026-05-11T00:00:00.000Z'),
  ('min_visual_refs', '5', '2026-05-11T00:00:00.000Z'),
  ('platform_engagement_thresholds_json', '{"tiktok":{"likes":10000},"instagram":{"likes":5000}}', '2026-05-11T00:00:00.000Z'),
  ('freshness_window_years', '5', '2026-05-11T00:00:00.000Z'),
  ('allow_unknown_source_date', 'true', '2026-05-11T00:00:00.000Z'),
  ('recent_search_window', 'last-year', '2026-05-11T00:00:00.000Z'),
  ('cluster_relevance_threshold', '0.72', '2026-05-11T00:00:00.000Z'),
  ('expand_clusters_per_run', '4', '2026-05-11T00:00:00.000Z'),
  ('max_seed_queries_per_platform', '8', '2026-05-11T00:00:00.000Z'),
  ('max_reference_generation_uses', '4', '2026-05-11T00:00:00.000Z'),
  ('scrape_delay_ms', '1000', '2026-05-11T00:00:00.000Z');

CREATE INDEX IF NOT EXISTS idx_inspiration_bubbles_user_clone ON inspiration_bubbles(user_id, clone_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_blitz_batches_clone_status ON blitz_batches(clone_id, status, batch_number DESC);
CREATE INDEX IF NOT EXISTS idx_blitz_batches_user_date ON blitz_batches(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_blitz_swipes_batch ON blitz_swipes(batch_id, swipe_index);
CREATE INDEX IF NOT EXISTS idx_blitz_swipes_clone ON blitz_swipes(clone_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_generation_daily_usage_date ON generation_daily_usage(user_id, usage_date DESC);
CREATE INDEX IF NOT EXISTS idx_generation_jobs_user ON generation_jobs(user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_generation_jobs_status ON generation_jobs(status, updated_at);
CREATE INDEX IF NOT EXISTS idx_generation_jobs_batch ON generation_jobs(blitz_batch_id) WHERE blitz_batch_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_generation_jobs_visual_ref ON generation_jobs(input_visual_reference_id, status) WHERE input_visual_reference_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_generation_outputs_job ON generation_outputs(job_id, output_index);
CREATE INDEX IF NOT EXISTS idx_discovery_items_source ON discovery_items(source_id, discovered_at DESC);
CREATE INDEX IF NOT EXISTS idx_discovery_items_platform_published ON discovery_items(platform, source_published_at DESC);
CREATE INDEX IF NOT EXISTS idx_discovery_items_platform_discovered ON discovery_items(platform, discovered_at DESC);
CREATE INDEX IF NOT EXISTS idx_visual_references_clone ON visual_references(clone_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_visual_references_clone_published ON visual_references(clone_id, source_published_at DESC) WHERE source_published_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_visual_references_clone_reuse ON visual_references(clone_id, generation_use_count, last_liked_at);
CREATE INDEX IF NOT EXISTS idx_visual_ref_candidates_clone ON visual_reference_candidates(clone_id, human_presence_status);
CREATE INDEX IF NOT EXISTS idx_niche_research_queries_status ON niche_research_queries(status, created_at);
CREATE INDEX IF NOT EXISTS idx_niche_research_queries_clone ON niche_research_queries(clone_id, status);
CREATE INDEX IF NOT EXISTS idx_niche_knowledge_clone ON niche_knowledge(clone_id, cluster);
CREATE INDEX IF NOT EXISTS idx_user_inspiration_pool_clone_unused ON user_inspiration_pool(clone_id, used_at, score DESC);

PRAGMA foreign_keys = ON;
