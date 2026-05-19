PRAGMA defer_foreign_keys = true;

DROP TABLE IF EXISTS queue_message_reservations;
DROP TABLE IF EXISTS clone_pool_waiting_moodboards;
DROP TABLE IF EXISTS clone_reference_compatibility_attempts;
DROP TABLE IF EXISTS clone_visual_reference_compatibility;
DROP TABLE IF EXISTS clone_pool_runs;
DROP TABLE IF EXISTS clone_reference_state;
DROP TABLE IF EXISTS global_moodboard_references;
DROP TABLE IF EXISTS global_visual_candidate_discoveries;
DROP TABLE IF EXISTS global_visual_reference_candidates;
DROP TABLE IF EXISTS global_moodboard_handles;
DROP TABLE IF EXISTS global_moodboard_search_state;
DROP TABLE IF EXISTS global_moodboard_source_runs;
DROP TABLE IF EXISTS global_moodboard_reference_state;
DROP TABLE IF EXISTS user_reference_state;
DROP TABLE IF EXISTS user_inspiration_pool;
DROP TABLE IF EXISTS visual_references;
DROP TABLE IF EXISTS moodboards;
DROP TABLE IF EXISTS global_moodboard_definitions;

CREATE TABLE IF NOT EXISTS global_moodboard_definitions (
  slug TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  vibe_summary TEXT NOT NULL DEFAULT '',
  search_queries_json TEXT NOT NULL DEFAULT '[]',
  sort_order INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  disabled_at TEXT
);

CREATE TABLE IF NOT EXISTS moodboards (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  slug TEXT NOT NULL,
  selected INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(user_id, slug),
  FOREIGN KEY (slug) REFERENCES global_moodboard_definitions(slug) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS user_reference_state (
  user_id TEXT PRIMARY KEY,
  selected_moodboard_ids_json TEXT NOT NULL DEFAULT '[]',
  selected_moodboard_slugs_json TEXT NOT NULL DEFAULT '[]',
  selected_moodboard_hash TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS global_moodboard_reference_state (
  moodboard_slug TEXT PRIMARY KEY,
  current_run_id TEXT,
  status TEXT NOT NULL DEFAULT 'queued',
  active_reference_count INTEGER NOT NULL DEFAULT 0,
  target_reference_count INTEGER NOT NULL DEFAULT 25,
  underfilled INTEGER NOT NULL DEFAULT 1,
  next_retry_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_successful_refresh_at TEXT,
  last_ready_at TEXT,
  last_underfilled_at TEXT,
  last_insufficient_at TEXT,
  FOREIGN KEY (moodboard_slug) REFERENCES global_moodboard_definitions(slug) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS global_moodboard_source_runs (
  id TEXT PRIMARY KEY,
  moodboard_slug TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'queued',
  reason TEXT NOT NULL DEFAULT '',
  selected_search_terms_json TEXT NOT NULL DEFAULT '[]',
  selected_date_windows_json TEXT NOT NULL DEFAULT '[]',
  discovered_handle_count INTEGER NOT NULL DEFAULT 0,
  candidate_count INTEGER NOT NULL DEFAULT 0,
  approved_count INTEGER NOT NULL DEFAULT 0,
  cleaned_count INTEGER NOT NULL DEFAULT 0,
  error_code TEXT,
  error_message TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  FOREIGN KEY (moodboard_slug) REFERENCES global_moodboard_definitions(slug) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS global_moodboard_search_state (
  id TEXT PRIMARY KEY,
  moodboard_slug TEXT NOT NULL,
  search_term TEXT NOT NULL,
  date_window TEXT NOT NULL DEFAULT '',
  page INTEGER NOT NULL DEFAULT 1,
  status TEXT NOT NULL DEFAULT 'active',
  last_run_at TEXT,
  next_eligible_at TEXT,
  seen_result_count INTEGER NOT NULL DEFAULT 0,
  failure_count INTEGER NOT NULL DEFAULT 0,
  last_error_code TEXT,
  last_error_message TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(moodboard_slug, search_term, date_window, page),
  FOREIGN KEY (moodboard_slug) REFERENCES global_moodboard_definitions(slug) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS global_moodboard_handles (
  id TEXT PRIMARY KEY,
  moodboard_slug TEXT NOT NULL,
  handle TEXT NOT NULL,
  discovered_via TEXT NOT NULL,
  related_depth INTEGER NOT NULL DEFAULT 0,
  last_fetched_at TEXT,
  next_cursor TEXT,
  accepted_count INTEGER NOT NULL DEFAULT 0,
  rejected_count INTEGER NOT NULL DEFAULT 0,
  fetch_count INTEGER NOT NULL DEFAULT 0,
  failure_count INTEGER NOT NULL DEFAULT 0,
  zero_acceptance_count INTEGER NOT NULL DEFAULT 0,
  cooldown_until TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(moodboard_slug, handle),
  FOREIGN KEY (moodboard_slug) REFERENCES global_moodboard_definitions(slug) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS global_visual_reference_candidates (
  id TEXT PRIMARY KEY,
  platform TEXT NOT NULL DEFAULT 'instagram',
  source_image_key TEXT NOT NULL,
  source_handle TEXT,
  source_profile_id TEXT,
  source_post_id TEXT,
  source_post_code TEXT,
  source_url TEXT,
  source_published_at TEXT,
  source_caption TEXT,
  media_type TEXT,
  image_url TEXT,
  image_width INTEGER,
  image_height INTEGER,
  like_count INTEGER,
  comment_count INTEGER,
  play_count INTEGER,
  discovery_moodboard_slug TEXT NOT NULL,
  assigned_moodboard_slug TEXT,
  discovered_via TEXT NOT NULL DEFAULT 'reels_owner',
  first_seen_run_id TEXT,
  last_seen_run_id TEXT,
  candidate_status TEXT NOT NULL DEFAULT 'active',
  review_status TEXT NOT NULL DEFAULT 'queued',
  review_run_id TEXT,
  review_attempt_count INTEGER NOT NULL DEFAULT 0,
  review_next_retry_at TEXT,
  review_claim_id TEXT,
  review_locked_until TEXT,
  review_json TEXT NOT NULL DEFAULT '{}',
  review_error_code TEXT,
  review_error_message TEXT,
  cleanup_status TEXT NOT NULL DEFAULT 'not_required',
  cleanup_attempt_count INTEGER NOT NULL DEFAULT 0,
  cleanup_next_retry_at TEXT,
  cleanup_json TEXT NOT NULL DEFAULT '{}',
  cleanup_error_code TEXT,
  cleanup_error_message TEXT,
  cleaned_image_url TEXT,
  source_error_code TEXT,
  source_error_message TEXT,
  metadata_json TEXT NOT NULL DEFAULT '{}',
  raw_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  reviewed_at TEXT,
  cleaned_at TEXT,
  UNIQUE(platform, source_image_key)
);

CREATE TABLE IF NOT EXISTS global_visual_candidate_discoveries (
  id TEXT PRIMARY KEY,
  candidate_id TEXT NOT NULL,
  run_id TEXT NOT NULL,
  moodboard_slug TEXT NOT NULL,
  source_key TEXT NOT NULL,
  source_id TEXT,
  discovered_via TEXT NOT NULL,
  source_handle TEXT,
  created_at TEXT NOT NULL,
  UNIQUE(candidate_id, run_id, moodboard_slug, source_key),
  FOREIGN KEY (candidate_id) REFERENCES global_visual_reference_candidates(id) ON DELETE CASCADE,
  FOREIGN KEY (run_id) REFERENCES global_moodboard_source_runs(id) ON DELETE CASCADE,
  FOREIGN KEY (moodboard_slug) REFERENCES global_moodboard_definitions(slug) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS global_moodboard_references (
  id TEXT PRIMARY KEY,
  candidate_id TEXT NOT NULL,
  media_asset_id TEXT NOT NULL,
  moodboard_slug TEXT NOT NULL,
  source_run_id TEXT,
  discovery_moodboard_slug TEXT NOT NULL,
  source_platform TEXT NOT NULL DEFAULT 'instagram',
  source_image_key TEXT NOT NULL,
  source_handle TEXT,
  source_post_id TEXT,
  source_post_code TEXT,
  source_url TEXT,
  source_published_at TEXT,
  image_width INTEGER,
  image_height INTEGER,
  editorial_composition_score REAL NOT NULL DEFAULT 0,
  real_pose_angle_score REAL NOT NULL DEFAULT 0,
  fashion_culture_cue_score REAL NOT NULL DEFAULT 0,
  lighting_color_direction_score REAL NOT NULL DEFAULT 0,
  moodboard_fit_score REAL NOT NULL DEFAULT 0,
  overall_reference_score REAL NOT NULL DEFAULT 0,
  pose TEXT,
  scene TEXT,
  lighting TEXT,
  framing TEXT,
  camera_feel TEXT,
  styling_direction TEXT,
  color_palette_json TEXT NOT NULL DEFAULT '[]',
  fashion_culture_cues_json TEXT NOT NULL DEFAULT '[]',
  composition_notes TEXT,
  review_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(candidate_id),
  UNIQUE(media_asset_id),
  FOREIGN KEY (candidate_id) REFERENCES global_visual_reference_candidates(id) ON DELETE CASCADE,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE RESTRICT,
  FOREIGN KEY (moodboard_slug) REFERENCES global_moodboard_definitions(slug) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS clone_reference_state (
  clone_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  current_pool_run_id TEXT,
  selected_moodboard_hash TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'queued',
  compatibility_queued_count INTEGER NOT NULL DEFAULT 0,
  compatibility_accepted_count INTEGER NOT NULL DEFAULT 0,
  compatibility_rejected_count INTEGER NOT NULL DEFAULT 0,
  compatibility_failed_count INTEGER NOT NULL DEFAULT 0,
  waiting_moodboard_slugs_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_usable_pool_at TEXT,
  last_ready_at TEXT,
  last_partial_ready_at TEXT,
  last_insufficient_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS clone_pool_runs (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'queued',
  reason TEXT NOT NULL DEFAULT '',
  selected_moodboard_ids_snapshot_json TEXT NOT NULL DEFAULT '[]',
  selected_moodboard_slugs_snapshot_json TEXT NOT NULL DEFAULT '[]',
  selected_moodboard_hash TEXT NOT NULL DEFAULT '',
  waiting_moodboard_slugs_json TEXT NOT NULL DEFAULT '[]',
  compatibility_queued_count INTEGER NOT NULL DEFAULT 0,
  compatibility_accepted_count INTEGER NOT NULL DEFAULT 0,
  compatibility_rejected_count INTEGER NOT NULL DEFAULT 0,
  compatibility_failed_count INTEGER NOT NULL DEFAULT 0,
  error_code TEXT,
  error_message TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS clone_visual_reference_compatibility (
  id TEXT PRIMARY KEY,
  clone_id TEXT NOT NULL,
  global_reference_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'queued',
  body_proportions_compatible INTEGER,
  hair_length_compatible INTEGER,
  facial_hair_compatible INTEGER,
  review_json TEXT NOT NULL DEFAULT '{}',
  attempt_count INTEGER NOT NULL DEFAULT 0,
  last_error_code TEXT,
  last_error_message TEXT,
  next_retry_at TEXT,
  last_attempted_at TEXT,
  accepted_at TEXT,
  rejected_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(clone_id, global_reference_id),
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (global_reference_id) REFERENCES global_moodboard_references(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS clone_reference_compatibility_attempts (
  id TEXT PRIMARY KEY,
  pool_run_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  global_reference_id TEXT NOT NULL,
  status TEXT NOT NULL,
  error_code TEXT,
  error_message TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (pool_run_id) REFERENCES clone_pool_runs(id) ON DELETE CASCADE,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (global_reference_id) REFERENCES global_moodboard_references(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS clone_pool_waiting_moodboards (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  pool_run_id TEXT NOT NULL,
  moodboard_slug TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'waiting',
  created_at TEXT NOT NULL,
  resolved_at TEXT,
  UNIQUE(pool_run_id, moodboard_slug),
  FOREIGN KEY (pool_run_id) REFERENCES clone_pool_runs(id) ON DELETE CASCADE,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (moodboard_slug) REFERENCES global_moodboard_definitions(slug) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS visual_references (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  global_reference_id TEXT NOT NULL,
  media_asset_id TEXT NOT NULL,
  source_platform TEXT NOT NULL DEFAULT 'instagram',
  source_image_key TEXT,
  source_handle TEXT,
  source_post_id TEXT,
  source_post_code TEXT,
  source_url TEXT,
  source_published_at TEXT,
  image_width INTEGER,
  image_height INTEGER,
  moodboard_id TEXT,
  moodboard_slug TEXT NOT NULL,
  niche_cluster TEXT,
  human_presence_type TEXT NOT NULL DEFAULT 'person',
  human_presence_score REAL NOT NULL DEFAULT 1,
  organic_photo_score REAL NOT NULL DEFAULT 1,
  freshness_visual_score REAL NOT NULL DEFAULT 1,
  visual_fit_score REAL NOT NULL DEFAULT 0,
  pose TEXT,
  scene TEXT,
  lighting TEXT,
  framing TEXT,
  camera_feel TEXT,
  styling_direction TEXT,
  aesthetic_tags_json TEXT NOT NULL DEFAULT '[]',
  source_caption_removed INTEGER NOT NULL DEFAULT 1,
  generation_use_count INTEGER NOT NULL DEFAULT 0,
  last_used_batch_id TEXT,
  last_liked_at TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(clone_id, global_reference_id),
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (global_reference_id) REFERENCES global_moodboard_references(id) ON DELETE CASCADE,
  FOREIGN KEY (media_asset_id) REFERENCES media_assets(id) ON DELETE RESTRICT,
  FOREIGN KEY (moodboard_id) REFERENCES moodboards(id) ON DELETE SET NULL,
  FOREIGN KEY (last_used_batch_id) REFERENCES blitz_batches(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS user_inspiration_pool (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  clone_id TEXT NOT NULL,
  moodboard_id TEXT,
  visual_reference_id TEXT,
  discovery_item_id TEXT,
  score REAL NOT NULL DEFAULT 1,
  used_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (clone_id) REFERENCES clone_profiles(id) ON DELETE CASCADE,
  FOREIGN KEY (moodboard_id) REFERENCES moodboards(id) ON DELETE SET NULL,
  FOREIGN KEY (visual_reference_id) REFERENCES visual_references(id) ON DELETE CASCADE,
  FOREIGN KEY (discovery_item_id) REFERENCES discovery_items(id) ON DELETE CASCADE,
  UNIQUE(clone_id, visual_reference_id),
  UNIQUE(clone_id, discovery_item_id)
);

CREATE TABLE IF NOT EXISTS queue_message_reservations (
  id TEXT PRIMARY KEY,
  queue_name TEXT NOT NULL,
  message_kind TEXT NOT NULL,
  dedupe_key TEXT NOT NULL,
  run_id TEXT,
  pool_run_id TEXT,
  status TEXT NOT NULL DEFAULT 'reserved',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  UNIQUE(queue_name, message_kind, dedupe_key)
);

CREATE INDEX IF NOT EXISTS idx_global_refs_slug_status
ON global_moodboard_references(moodboard_slug, status);

CREATE INDEX IF NOT EXISTS idx_global_candidates_review
ON global_visual_reference_candidates(discovery_moodboard_slug, candidate_status, review_status, cleanup_status);

CREATE INDEX IF NOT EXISTS idx_global_discoveries_run
ON global_visual_candidate_discoveries(run_id, moodboard_slug);

CREATE INDEX IF NOT EXISTS idx_global_handles_rotation
ON global_moodboard_handles(moodboard_slug, status, cooldown_until, last_fetched_at);

CREATE INDEX IF NOT EXISTS idx_clone_waiting_slug_status
ON clone_pool_waiting_moodboards(moodboard_slug, status);

CREATE INDEX IF NOT EXISTS idx_clone_waiting_clone_run
ON clone_pool_waiting_moodboards(clone_id, pool_run_id);

CREATE INDEX IF NOT EXISTS idx_clone_waiting_user_status
ON clone_pool_waiting_moodboards(user_id, status);

CREATE INDEX IF NOT EXISTS idx_visual_references_clone_status
ON visual_references(user_id, clone_id, status);

CREATE INDEX IF NOT EXISTS idx_visual_references_slug_status
ON visual_references(moodboard_slug, status);

CREATE INDEX IF NOT EXISTS idx_visual_references_global
ON visual_references(global_reference_id);

CREATE INDEX IF NOT EXISTS idx_queue_reservations_active
ON queue_message_reservations(queue_name, message_kind, status, expires_at);

INSERT OR REPLACE INTO blitz_config (key, value, updated_at) VALUES
  ('global_refs_per_moodboard_target', '25', '2026-05-18T00:00:00.000Z'),
  ('global_refs_per_moodboard_min_ready', '5', '2026-05-18T00:00:00.000Z'),
  ('global_refs_for_pool_min', '5', '2026-05-18T00:00:00.000Z'),
  ('global_library_stale_after_hours', '168', '2026-05-18T00:00:00.000Z'),
  ('global_discovery_run_stale_after_minutes', '60', '2026-05-18T00:00:00.000Z'),
  ('global_insufficient_retry_after_hours', '24', '2026-05-18T00:00:00.000Z'),
  ('global_source_failure_retry_after_hours', '12', '2026-05-18T00:00:00.000Z'),
  ('clone_pool_stale_after_hours', '24', '2026-05-18T00:00:00.000Z'),
  ('clone_pool_run_stale_after_minutes', '30', '2026-05-18T00:00:00.000Z'),
  ('instagram_search_terms_per_moodboard', '2', '2026-05-18T00:00:00.000Z'),
  ('instagram_reels_pages_per_term', '1', '2026-05-18T00:00:00.000Z'),
  ('instagram_reels_date_windows_json', '["last-month","last-year"]', '2026-05-18T00:00:00.000Z'),
  ('instagram_max_handles_per_moodboard_run', '20', '2026-05-18T00:00:00.000Z'),
  ('instagram_profiles_per_moodboard_run', '8', '2026-05-18T00:00:00.000Z'),
  ('instagram_related_profiles_per_seed', '2', '2026-05-18T00:00:00.000Z'),
  ('instagram_max_profiles_per_global_run', '40', '2026-05-18T00:00:00.000Z'),
  ('instagram_posts_per_profile', '12', '2026-05-18T00:00:00.000Z'),
  ('instagram_pages_per_profile', '1', '2026-05-18T00:00:00.000Z'),
  ('instagram_images_per_post', '3', '2026-05-18T00:00:00.000Z'),
  ('instagram_candidate_review_limit', '80', '2026-05-18T00:00:00.000Z'),
  ('instagram_min_image_width', '512', '2026-05-18T00:00:00.000Z'),
  ('instagram_min_image_height', '512', '2026-05-18T00:00:00.000Z'),
  ('accepted_refs_per_profile_cap', '3', '2026-05-18T00:00:00.000Z'),
  ('max_accepted_refs_per_global_run', '50', '2026-05-18T00:00:00.000Z'),
  ('visual_reference_review_retry_limit', '2', '2026-05-18T00:00:00.000Z'),
  ('visual_reference_cleanup_retry_limit', '3', '2026-05-18T00:00:00.000Z'),
  ('visual_reference_compatibility_retry_limit', '2', '2026-05-18T00:00:00.000Z'),
  ('clone_compatibility_reference_limit', '4', '2026-05-18T00:00:00.000Z'),
  ('clone_pool_global_reference_review_limit', '40', '2026-05-18T00:00:00.000Z'),
  ('clone_pool_compatibility_wave_size', '10', '2026-05-18T00:00:00.000Z'),
  ('batch_size', '5', '2026-05-18T00:00:00.000Z');

PRAGMA defer_foreign_keys = false;
