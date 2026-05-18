ALTER TABLE visual_reference_candidates ADD COLUMN cleanup_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE visual_reference_candidates ADD COLUMN cleaned_image_url TEXT;
ALTER TABLE visual_reference_candidates ADD COLUMN compatibility_json TEXT NOT NULL DEFAULT '{}';

INSERT OR REPLACE INTO blitz_config (key, value, updated_at) VALUES
  ('instagram_search_terms_per_moodboard', '2', '2026-05-17T00:00:00.000Z'),
  ('instagram_reels_pages_per_term', '1', '2026-05-17T00:00:00.000Z'),
  ('instagram_max_handles_per_moodboard', '20', '2026-05-17T00:00:00.000Z'),
  ('instagram_min_image_width', '512', '2026-05-17T00:00:00.000Z'),
  ('instagram_min_image_height', '512', '2026-05-17T00:00:00.000Z'),
  ('visual_reference_cleanup_retry_limit', '3', '2026-05-17T00:00:00.000Z'),
  ('visual_reference_compatibility_retry_limit', '2', '2026-05-17T00:00:00.000Z'),
  ('clone_compatibility_reference_limit', '4', '2026-05-17T00:00:00.000Z');
