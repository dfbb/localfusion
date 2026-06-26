# P1-T06 settings 表读写

**阶段:** 1 基础层 · **前置:** P1-T05 · 见全局约束: `00-index.md`

**Goal:** kv 配置读写（upsert）。

**Files:** Modify: `src/db/settings.rs`

**Produces（`impl Db`）:** `setting_get(key)->Option<String>`、`setting_set(key,value)`、`setting_get_or(key,default)->String`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use crate::db::Db;
    #[tokio::test]
    async fn set_get_roundtrip_and_default() {
        let db = Db::open_memory().await.unwrap();
        assert_eq!(db.setting_get("log_level").await.unwrap(), None);
        assert_eq!(db.setting_get_or("log_level", "info").await.unwrap(), "info");
        db.setting_set("log_level", "debug").await.unwrap();
        assert_eq!(db.setting_get("log_level").await.unwrap(), Some("debug".into()));
        db.setting_set("log_level", "error").await.unwrap();
        assert_eq!(db.setting_get_or("log_level", "info").await.unwrap(), "error");
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib db::settings` → FAIL。

- [ ] **Step 3: 实现**

```rust
use crate::db::Db;
use crate::error::FusionError;

impl Db {
    pub async fn setting_get(&self, key: &str) -> Result<Option<String>, FusionError> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| r.0))
    }
    pub async fn setting_set(&self, key: &str, value: &str) -> Result<(), FusionError> {
        sqlx::query("INSERT INTO settings(key, value) VALUES(?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(key).bind(value).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn setting_get_or(&self, key: &str, default: &str) -> Result<String, FusionError> {
        Ok(self.setting_get(key).await?.unwrap_or_else(|| default.to_string()))
    }
}
```

(把 Step 1 测试块置于文件末尾。)

- [ ] **Step 4: 运行确认通过** — Run: `cargo test --lib db::settings` → PASS。

- [ ] **Step 5: 提交**

```bash
git add src/db/settings.rs
git commit -m "feat: settings 表 kv 读写(upsert)"
```
