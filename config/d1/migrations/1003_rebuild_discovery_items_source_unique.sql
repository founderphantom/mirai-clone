PRAGMA foreign_keys = OFF;

DROP INDEX IF EXISTS idx_discovery_items_source;
DROP INDEX IF EXISTS idx_discovery_items_platform_published;
DROP INDEX IF EXISTS idx_discovery_items_platform_discovered;
DROP TABLE IF EXISTS discovery_items_new;

CREATE TABLE IF NOT EXISTS discovery_items_new (
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

INSERT INTO discovery_items_new (
  id,
  source_id,
  external_id,
  platform,
  media_type,
  title,
  author_handle,
  thumbnail_url,
  image_url,
  source_url,
  source_published_at,
  metrics_json,
  raw_json,
  discovered_at,
  expires_at,
  created_at
)
SELECT
  id,
  source_id,
  external_id,
  platform,
  media_type,
  title,
  author_handle,
  thumbnail_url,
  image_url,
  source_url,
  source_published_at,
  metrics_json,
  raw_json,
  discovered_at,
  expires_at,
  created_at
FROM discovery_items;

DROP TABLE discovery_items;
ALTER TABLE discovery_items_new RENAME TO discovery_items;

CREATE INDEX IF NOT EXISTS idx_discovery_items_source ON discovery_items(source_id, discovered_at DESC);
CREATE INDEX IF NOT EXISTS idx_discovery_items_platform_published ON discovery_items(platform, source_published_at DESC);
CREATE INDEX IF NOT EXISTS idx_discovery_items_platform_discovered ON discovery_items(platform, discovered_at DESC);

INSERT OR IGNORE INTO blitz_config (key, value, updated_at)
VALUES ('max_seed_queries_per_platform', '8', '2026-05-11T00:00:00.000Z');

PRAGMA foreign_keys = ON;
