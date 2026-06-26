# P1-T05 DB 连接池 / 迁移 / PRAGMA

**阶段:** 1 基础层 · **前置:** P1-T02 · 见全局约束: `00-index.md`

**Goal:** sqlx 连接池 + 完整 schema 迁移 + 每连接 PRAGMA（设计 §4）。

**Files:** Modify: `src/db/mod.rs`；Create: `src/db/schema.rs`；占位 `src/db/{models,keys,latency,prices,usage,settings}.rs`

**Produces:** `Db{pool}`、`Db::open(path)`、`Db::open_memory()`、`schema::SCHEMA_SQL`；子模块声明。

- [ ] **Step 1: 写 db/schema.rs（设计 §4 完整 schema，逐字照抄）**

```rust
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
```

- [ ] **Step 2: 写 db/mod.rs（含失败测试）**

```rust
pub mod keys;
pub mod latency;
pub mod models;
pub mod prices;
pub mod schema;
pub mod settings;
pub mod usage;
pub mod virtual_models;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{ConnectOptions, SqlitePool};
use std::str::FromStr;

use crate::error::FusionError;

#[derive(Clone)]
pub struct Db { pub pool: SqlitePool }

impl Db {
    pub async fn open(path: &str) -> Result<Db, FusionError> {
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{path}"))
            .map_err(|e| FusionError::Internal(format!("db opts: {e}")))?
            .create_if_missing(true)
            .disable_statement_logging();
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .after_connect(|conn, _| Box::pin(async move { apply_pragmas(conn).await }))
            .connect_with(opts).await
            .map_err(|e| FusionError::Internal(format!("db connect: {e}")))?;
        sqlx::query(schema::SCHEMA_SQL).execute(&pool).await?;
        Ok(Db { pool })
    }

    pub async fn open_memory() -> Result<Db, FusionError> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(|e| FusionError::Internal(format!("db opts: {e}")))?;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .after_connect(|conn, _| Box::pin(async move { apply_pragmas(conn).await }))
            .connect_with(opts).await
            .map_err(|e| FusionError::Internal(format!("db connect: {e}")))?;
        sqlx::query(schema::SCHEMA_SQL).execute(&pool).await?;
        Ok(Db { pool })
    }
}

async fn apply_pragmas(conn: &mut sqlx::SqliteConnection) -> Result<(), sqlx::Error> {
    sqlx::query("PRAGMA foreign_keys = ON;").execute(&mut *conn).await?;
    sqlx::query("PRAGMA journal_mode = WAL;").execute(&mut *conn).await?;
    sqlx::query("PRAGMA busy_timeout = 5000;").execute(&mut *conn).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn open_memory_creates_schema_and_fk_on() {
        let db = Db::open_memory().await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM models").fetch_one(&db.pool).await.unwrap();
        assert_eq!(n, 0);
        let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys").fetch_one(&db.pool).await.unwrap();
        assert_eq!(fk, 1);
    }
}
```

- [ ] **Step 3: 占位子模块** — 创建 `src/db/{models,keys,latency,prices,usage,settings,virtual_models}.rs`，内容 `// filled in later task`。

- [ ] **Step 4: 运行确认通过** — Run: `cargo test --lib db::tests` → PASS。

- [ ] **Step 5: 提交**

```bash
git add src/db/
git commit -m "feat: DB 连接池/迁移/PRAGMA(外键·WAL·busy_timeout)"
```
