/// Complete database schema (design §4)
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY, value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS models (
  id TEXT PRIMARY KEY, connector TEXT NOT NULL, base_url TEXT NOT NULL,
  api_key_enc TEXT, api_key_env TEXT, model TEXT NOT NULL,
  anthropic_version TEXT, extra TEXT
);
CREATE TABLE IF NOT EXISTS virtual_models (
  name TEXT PRIMARY KEY, strategy TEXT NOT NULL, params TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS virtual_model_members (
  virtual_name TEXT NOT NULL REFERENCES virtual_models(name) ON DELETE CASCADE,
  model_id TEXT NOT NULL REFERENCES models(id),
  position INTEGER NOT NULL,
  PRIMARY KEY (virtual_name, model_id)
);
CREATE TABLE IF NOT EXISTS ingress_keys (
  id INTEGER PRIMARY KEY, key_hash TEXT NOT NULL UNIQUE, label TEXT,
  enabled INTEGER NOT NULL DEFAULT 1, acl_all INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ingress_key_acl (
  key_id INTEGER NOT NULL REFERENCES ingress_keys(id) ON DELETE CASCADE,
  virtual_name TEXT NOT NULL REFERENCES virtual_models(name) ON DELETE CASCADE,
  PRIMARY KEY (key_id, virtual_name)
);
CREATE TABLE IF NOT EXISTS prices (
  model_id TEXT PRIMARY KEY, price_in REAL NOT NULL, price_out REAL NOT NULL,
  cache_read REAL NOT NULL DEFAULT 0, cache_write REAL NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS price_defaults (
  model_key TEXT PRIMARY KEY, price_in REAL NOT NULL, price_out REAL NOT NULL,
  cache_read REAL NOT NULL DEFAULT 0, cache_write REAL NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS latency_samples (
  id INTEGER PRIMARY KEY, model_id TEXT NOT NULL, tokens_out INTEGER NOT NULL,
  output_secs REAL NOT NULL, throughput REAL NOT NULL,
  is_probe INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_latency_model_time ON latency_samples(model_id, created_at);
CREATE TABLE IF NOT EXISTS request_log (
  id INTEGER PRIMARY KEY, virtual_name TEXT, strategy TEXT, status TEXT,
  total_tokens INTEGER, cost REAL, created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS usage_hourly (
  hour_ts INTEGER NOT NULL, scope TEXT NOT NULL, name TEXT NOT NULL,
  requests INTEGER NOT NULL DEFAULT 0, input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0, total_tokens INTEGER NOT NULL DEFAULT 0,
  cost REAL NOT NULL DEFAULT 0, errors INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (hour_ts, scope, name)
);
CREATE INDEX IF NOT EXISTS idx_usage_hour ON usage_hourly(hour_ts);
"#;
