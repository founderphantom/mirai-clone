PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS user_inspiration_pool;
DROP TABLE IF EXISTS niche_research_queries;
DROP TABLE IF EXISTS moodboards;
DROP TABLE IF EXISTS inspiration_bubbles;

CREATE TABLE IF NOT EXISTS moodboards (
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
  moodboard_id TEXT,
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
  FOREIGN KEY (moodboard_id) REFERENCES moodboards(id) ON DELETE SET NULL
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

CREATE INDEX IF NOT EXISTS idx_moodboards_user_clone ON moodboards(user_id, clone_id, sort_order);
CREATE INDEX IF NOT EXISTS idx_niche_research_queries_status ON niche_research_queries(status, created_at);
CREATE INDEX IF NOT EXISTS idx_niche_research_queries_clone ON niche_research_queries(clone_id, status);
CREATE INDEX IF NOT EXISTS idx_user_inspiration_pool_clone_unused ON user_inspiration_pool(clone_id, used_at, score DESC);

PRAGMA foreign_keys = ON;
