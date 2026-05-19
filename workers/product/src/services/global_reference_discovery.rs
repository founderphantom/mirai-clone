pub fn bootstrap_global_search_state_sql() -> &'static str {
    r#"
    INSERT OR IGNORE INTO global_moodboard_search_state (
      id,
      moodboard_slug,
      search_term,
      date_window,
      page,
      status,
      created_at,
      updated_at
    )
    VALUES (?, ?, ?, ?, ?, 'active', ?, ?)
    "#
}

pub fn select_global_search_work_sql() -> &'static str {
    r#"
    SELECT id, moodboard_slug, search_term, date_window, page
    FROM global_moodboard_search_state
    WHERE moodboard_slug = ?
      AND status IN ('active', 'cooldown')
      AND (next_eligible_at IS NULL OR next_eligible_at <= ?)
    ORDER BY
      CASE WHEN last_run_at IS NULL THEN 0 ELSE 1 END ASC,
      COALESCE(last_run_at, '0000-00-00T00:00:00Z') ASC,
      failure_count ASC,
      page ASC,
      search_term ASC
    LIMIT ?
    "#
}

pub fn select_global_handle_work_sql() -> &'static str {
    r#"
    SELECT id, moodboard_slug, handle, discovered_via, related_depth, next_cursor AS next_max_id
    FROM global_moodboard_handles
    WHERE moodboard_slug = ?
      AND status IN ('active', 'cooldown')
      AND (cooldown_until IS NULL OR cooldown_until <= ?)
    ORDER BY
      last_fetched_at IS NULL DESC,
      accepted_count DESC,
      rejected_count ASC,
      fetch_count ASC,
      COALESCE(last_fetched_at, '0000-00-00T00:00:00Z') ASC,
      handle ASC
    LIMIT ?
    "#
}

pub fn upsert_global_handle_sql() -> &'static str {
    r#"
    INSERT INTO global_moodboard_handles (
      id,
      moodboard_slug,
      handle,
      discovered_via,
      related_depth,
      status,
      created_at,
      updated_at
    )
    VALUES (?, ?, lower(?), ?, ?, 'active', ?, ?)
    ON CONFLICT(moodboard_slug, handle) DO UPDATE SET
      discovered_via = excluded.discovered_via,
      related_depth = MIN(global_moodboard_handles.related_depth, excluded.related_depth),
      status = CASE
        WHEN global_moodboard_handles.status IN ('disabled', 'bad_source') THEN global_moodboard_handles.status
        ELSE 'active'
      END,
      updated_at = excluded.updated_at
    "#
}

pub fn upsert_global_candidate_sql() -> &'static str {
    r#"
    INSERT INTO global_visual_reference_candidates (
      id,
      platform,
      source_image_key,
      source_handle,
      source_profile_id,
      source_post_id,
      source_post_code,
      source_url,
      source_published_at,
      source_caption,
      media_type,
      image_url,
      image_width,
      image_height,
      like_count,
      comment_count,
      play_count,
      discovery_moodboard_slug,
      discovered_via,
      first_seen_run_id,
      last_seen_run_id,
      candidate_status,
      review_status,
      cleanup_status,
      metadata_json,
      raw_json,
      created_at,
      updated_at
    )
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', 'queued', 'not_required', ?, ?, ?, ?)
    ON CONFLICT(platform, source_image_key) DO UPDATE SET
      last_seen_run_id = excluded.last_seen_run_id,
      metadata_json = excluded.metadata_json,
      updated_at = excluded.updated_at
    -- uniqueness contract: UNIQUE(platform, source_image_key)
    "#
}

pub fn audit_global_candidate_discovery_sql() -> &'static str {
    r#"
    INSERT OR IGNORE INTO global_visual_candidate_discoveries (
      id,
      candidate_id,
      run_id,
      moodboard_slug,
      source_key,
      source_id,
      discovered_via,
      source_handle,
      created_at
    )
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
    -- uniqueness contract: UNIQUE(candidate_id, run_id, moodboard_slug, source_key)
    "#
}

pub fn source_key_for_reels_search(search_term: &str, date_window: &str, page: u32) -> String {
    let search_term = search_term.trim().to_ascii_lowercase();
    let date_window = date_window.trim().to_ascii_lowercase();
    format!(
        "instagram_reels_search:{}:{}:p={}",
        length_prefixed_field("q", &search_term),
        length_prefixed_field("w", &date_window),
        page.max(1)
    )
}

pub fn source_key_for_instagram_handle(handle: &str, post_or_profile_key: &str) -> String {
    let handle = handle.trim().trim_start_matches('@').to_ascii_lowercase();
    let post_or_profile_key = post_or_profile_key.trim();
    format!(
        "instagram_handle:{}:{}",
        length_prefixed_field("h", &handle),
        length_prefixed_field("k", post_or_profile_key)
    )
}

fn length_prefixed_field(label: &str, value: &str) -> String {
    format!("{}{}:{}", label, value.len(), value)
}
