PRAGMA foreign_keys = ON;

ALTER TABLE clone_profiles ADD COLUMN starter_character_id TEXT;
ALTER TABLE clone_profiles ADD COLUMN soul_source TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE clone_profiles ADD COLUMN soul_status TEXT NOT NULL DEFAULT 'pending_script';
ALTER TABLE clone_profiles ADD COLUMN soul_character_id TEXT;
ALTER TABLE clone_profiles ADD COLUMN soul_script_job_id TEXT;
ALTER TABLE clone_profiles ADD COLUMN source_snapshot_json TEXT NOT NULL DEFAULT '{}';

CREATE INDEX IF NOT EXISTS idx_clone_profiles_soul_status
ON clone_profiles(user_id, soul_status, updated_at DESC);

CREATE TABLE IF NOT EXISTS instagram_harvest_jobs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  request_key TEXT NOT NULL UNIQUE,
  handle TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'queued',
  candidate_count INTEGER NOT NULL DEFAULT 0,
  accepted_count INTEGER NOT NULL DEFAULT 0,
  fail_reason TEXT,
  clone_id TEXT,
  accepted_media_asset_ids_json TEXT NOT NULL DEFAULT '[]',
  raw_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_instagram_harvest_jobs_user
ON instagram_harvest_jobs(user_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_instagram_harvest_jobs_status
ON instagram_harvest_jobs(status, updated_at);

CREATE TABLE IF NOT EXISTS starter_characters (
  id TEXT PRIMARY KEY,
  slug TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  persona TEXT NOT NULL DEFAULT '',
  style_prompt TEXT NOT NULL DEFAULT '',
  hero_media_id TEXT,
  provider_config_json TEXT NOT NULL DEFAULT '{}',
  sort INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'setup_pending',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (hero_media_id) REFERENCES media_assets(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_starter_characters_status_sort
ON starter_characters(status, sort);

CREATE TABLE IF NOT EXISTS inspiration_bubbles (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT,
  slug TEXT NOT NULL,
  title TEXT NOT NULL,
  vibe_summary TEXT NOT NULL DEFAULT '',
  search_queries_json TEXT NOT NULL DEFAULT '[]',
  example_keywords TEXT NOT NULL DEFAULT '',
  source TEXT NOT NULL DEFAULT 'persona_agent',
  selected INTEGER NOT NULL DEFAULT 0,
  weight REAL NOT NULL DEFAULT 1,
  sort INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_inspiration_bubbles_user_clone
ON inspiration_bubbles(user_id, clone_id, sort);

CREATE TABLE IF NOT EXISTS user_inspiration_pool (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  bubble_id TEXT,
  discovery_item_id TEXT NOT NULL,
  score REAL NOT NULL DEFAULT 1,
  used_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (bubble_id) REFERENCES inspiration_bubbles(id) ON DELETE SET NULL,
  FOREIGN KEY (discovery_item_id) REFERENCES discovery_items(id) ON DELETE CASCADE,
  UNIQUE(user_id, discovery_item_id)
);

CREATE INDEX IF NOT EXISTS idx_user_inspiration_pool_user_unused
ON user_inspiration_pool(user_id, used_at, score DESC);

INSERT INTO starter_characters
  (id, slug, name, persona, style_prompt, hero_media_id, provider_config_json, sort, status, created_at, updated_at)
VALUES
  ('starter_sky_soft_glam', 'sky-soft-glam', 'Sky - Soft Glam', 'A soft-glam lifestyle creator with clean beauty, cafe mornings, pastel athleisure, and bright apartment light.', 'soft glam creator portrait, polished lifestyle, warm daylight, editorial but approachable', NULL, '{}', 10, 'setup_pending', '2026-05-06T00:00:00.000Z', '2026-05-06T00:00:00.000Z'),
  ('starter_marina_coastal', 'marina-coastal', 'Marina - Coastal', 'A coastal creator with linen outfits, beach walks, golden-hour dinners, and effortless vacation energy.', 'coastal lifestyle editorial, linen wardrobe, ocean light, relaxed luxury', NULL, '{}', 20, 'setup_pending', '2026-05-06T00:00:00.000Z', '2026-05-06T00:00:00.000Z'),
  ('starter_aiden_streetwear', 'aiden-streetwear', 'Aiden - Streetwear', 'A streetwear creator with city backdrops, layered fits, sneaker details, and confident motion poses.', 'streetwear fashion editorial, city night, crisp outfit focus, social-first composition', NULL, '{}', 30, 'setup_pending', '2026-05-06T00:00:00.000Z', '2026-05-06T00:00:00.000Z'),
  ('starter_noor_editorial', 'noor-editorial', 'Noor - Editorial', 'An editorial creator with bold makeup, dramatic silhouettes, studio lighting, and magazine-style posing.', 'high-fashion editorial portrait, dramatic styling, clean studio light, premium creator look', NULL, '{}', 40, 'setup_pending', '2026-05-06T00:00:00.000Z', '2026-05-06T00:00:00.000Z'),
  ('starter_juno_fitness', 'juno-fitness', 'Juno - Fitness', 'A wellness and fitness creator with Pilates sets, morning routines, smoothie bars, and bright activewear.', 'fitness lifestyle creator, clean gym light, healthy routine, energetic but polished', NULL, '{}', 50, 'setup_pending', '2026-05-06T00:00:00.000Z', '2026-05-06T00:00:00.000Z'),
  ('starter_valentin_luxury_travel', 'valentin-luxury-travel', 'Valentin - Luxury Travel', 'A luxury travel creator with rooftop hotels, airport fits, city breaks, and elevated dining scenes.', 'luxury travel creator, rooftop city view, premium outfit, cinematic social content', NULL, '{}', 60, 'setup_pending', '2026-05-06T00:00:00.000Z', '2026-05-06T00:00:00.000Z'),
  ('starter_sienna_cottagecore', 'sienna-cottagecore', 'Sienna - Cottagecore', 'A cottagecore creator with garden walks, vintage dresses, market flowers, and soft countryside scenes.', 'cottagecore lifestyle, garden daylight, vintage styling, dreamy natural texture', NULL, '{}', 70, 'setup_pending', '2026-05-06T00:00:00.000Z', '2026-05-06T00:00:00.000Z'),
  ('starter_kai_cyber_night', 'kai-cyber-night', 'Kai - Cyber Night', 'A cyber-night creator with neon streets, glossy jackets, nightlife edits, and futuristic styling.', 'neon nightlife creator, cyber streetwear, glossy reflections, bold social visual', NULL, '{}', 80, 'setup_pending', '2026-05-06T00:00:00.000Z', '2026-05-06T00:00:00.000Z'),
  ('starter_maya_minimal_clean', 'maya-minimal-clean', 'Maya - Minimal Clean', 'A clean minimal creator with neutral outfits, desk setups, gallery visits, and calm city mornings.', 'minimal lifestyle creator, neutral palette, clean composition, modern city calm', NULL, '{}', 90, 'setup_pending', '2026-05-06T00:00:00.000Z', '2026-05-06T00:00:00.000Z'),
  ('starter_rio_festival', 'rio-festival', 'Rio - Festival', 'A festival and nightlife creator with statement outfits, crowd energy, flash photos, and playful edits.', 'festival creator portrait, expressive outfit, night flash, energetic social-first style', NULL, '{}', 100, 'setup_pending', '2026-05-06T00:00:00.000Z', '2026-05-06T00:00:00.000Z')
ON CONFLICT(slug) DO NOTHING;
