PRAGMA foreign_keys = ON;

CREATE INDEX IF NOT EXISTS idx_accounts_plan ON accounts(plan);
CREATE INDEX IF NOT EXISTS idx_clone_profiles_user_status ON clone_profiles(user_id, status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_clone_profiles_soul_status ON clone_profiles(soul_status, updated_at);
CREATE INDEX IF NOT EXISTS idx_media_assets_user_kind ON media_assets(user_id, kind, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_media_assets_clone ON media_assets(clone_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_media_assets_sha ON media_assets(user_id, sha256);
CREATE INDEX IF NOT EXISTS idx_clone_reference_assets_clone ON clone_reference_assets(clone_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_soul_training_jobs_status ON soul_training_jobs(status, updated_at);
CREATE INDEX IF NOT EXISTS idx_soul_training_jobs_clone ON soul_training_jobs(clone_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_provider_accounts_provider_health ON provider_accounts(provider, health_state);
CREATE INDEX IF NOT EXISTS idx_provider_account_leases_active ON provider_account_leases(provider_account_id, status, lease_expires_at);
CREATE INDEX IF NOT EXISTS idx_generation_jobs_user ON generation_jobs(user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_generation_jobs_status ON generation_jobs(status, updated_at);
CREATE INDEX IF NOT EXISTS idx_generation_outputs_job ON generation_outputs(job_id, output_index);
CREATE INDEX IF NOT EXISTS idx_credit_ledger_user ON credit_ledger(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_billing_events_user ON billing_events(user_id, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_billing_events_provider_external_event
ON billing_events(provider, external_event_id)
WHERE external_event_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_ai_model_invocations_task ON ai_model_invocations(task, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_moodboards_user_clone ON moodboards(user_id, clone_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_niche_research_queries_status ON niche_research_queries(status, created_at);
CREATE INDEX IF NOT EXISTS idx_niche_knowledge_cluster ON niche_knowledge(user_id, cluster, score DESC);
CREATE INDEX IF NOT EXISTS idx_discovery_items_source ON discovery_items(source_id, discovered_at DESC);
CREATE INDEX IF NOT EXISTS idx_visual_reference_candidates_status ON visual_reference_candidates(human_presence_status, created_at);
CREATE INDEX IF NOT EXISTS idx_visual_references_user_status ON visual_references(user_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_user_inspiration_pool_user_unused ON user_inspiration_pool(user_id, used_at, score DESC);
CREATE INDEX IF NOT EXISTS idx_app_events_user_created ON app_events(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_deletion_jobs_status ON deletion_jobs(status, updated_at);
