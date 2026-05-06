-- Better Auth uses this table for OAuth state, verification tokens, and other
-- short-lived auth challenges. Email/password flows can work without touching
-- it, but Google social login needs it before redirecting.
CREATE TABLE IF NOT EXISTS verification (
  id TEXT PRIMARY KEY,
  identifier TEXT NOT NULL,
  value TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsecond') * 1000 AS INTEGER)),
  updated_at INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsecond') * 1000 AS INTEGER))
);

CREATE INDEX IF NOT EXISTS verification_identifier_idx
  ON verification(identifier);
