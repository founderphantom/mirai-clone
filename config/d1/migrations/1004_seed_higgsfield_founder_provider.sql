INSERT INTO provider_accounts (
  id,
  provider,
  label,
  plan,
  capabilities_json,
  health_state,
  capacity_json,
  secret_refs_json,
  created_at,
  updated_at
)
VALUES (
  'pa_higgsfield_founder',
  'higgsfield',
  'Founder Higgsfield',
  'founder',
  '["soul_training","image_generation"]',
  'healthy',
  '{"maxLeases":1}',
  '{"refreshToken":"HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER"}',
  strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
  strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
)
ON CONFLICT(id) DO UPDATE SET
  provider = excluded.provider,
  label = excluded.label,
  plan = excluded.plan,
  capabilities_json = excluded.capabilities_json,
  health_state = excluded.health_state,
  capacity_json = excluded.capacity_json,
  secret_refs_json = excluded.secret_refs_json,
  disabled_at = NULL,
  updated_at = excluded.updated_at;
