UPDATE provider_accounts
SET secret_refs_json = json_remove(
      secret_refs_json,
      '$.refreshTokenValue',
      '$.refreshTokenUpdatedAt',
      '$.refreshTokenExpiresIn'
    ),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE provider = 'higgsfield'
  AND json_valid(secret_refs_json)
  AND (
    json_type(secret_refs_json, '$.refreshTokenValue') IS NOT NULL
    OR json_type(secret_refs_json, '$.refreshTokenUpdatedAt') IS NOT NULL
    OR json_type(secret_refs_json, '$.refreshTokenExpiresIn') IS NOT NULL
  );

UPDATE provider_accounts
SET secret_refs_json = json_set(
      CASE
        WHEN json_valid(secret_refs_json) AND json_type(secret_refs_json) = 'object'
          THEN secret_refs_json
        ELSE '{}'
      END,
      '$.refreshToken',
      'HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER'
    ),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 'pa_higgsfield_founder'
  AND provider = 'higgsfield'
  AND (
    NOT json_valid(secret_refs_json)
    OR COALESCE(
      CASE
        WHEN json_valid(secret_refs_json)
          THEN json_extract(secret_refs_json, '$.refreshToken')
      END,
      ''
    ) != 'HIGGSFIELD_PROVIDER_REFRESH_TOKEN_FOUNDER'
  );
