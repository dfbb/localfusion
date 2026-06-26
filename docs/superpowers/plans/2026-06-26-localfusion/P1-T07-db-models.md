# P1-T07 models 表 CRUD

**阶段:** 1 基础层 · **前置:** P1-T05 · 见全局约束: `00-index.md`

**Goal:** 真实模型表 CRUD。

**Files:** Modify: `src/db/models.rs`

**Produces:** `ModelRow{id,connector,base_url,api_key_enc:Option<String>,api_key_env:Option<String>,model,anthropic_version:Option<String>,extra:Option<String>}`（`FromRow+Serialize+Deserialize+Clone+Debug`）；`Db::{model_list,model_get,model_upsert,model_delete}`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    fn sample() -> ModelRow {
        ModelRow { id: "gpt-4o".into(), connector: "chat".into(),
            base_url: "https://api.openai.com/v1".into(), api_key_enc: Some("ENC".into()),
            api_key_env: None, model: "gpt-4o".into(), anthropic_version: None, extra: None }
    }
    #[tokio::test]
    async fn crud_cycle() {
        let db = Db::open_memory().await.unwrap();
        assert!(db.model_list().await.unwrap().is_empty());
        db.model_upsert(&sample()).await.unwrap();
        assert_eq!(db.model_get("gpt-4o").await.unwrap().unwrap().model, "gpt-4o");
        let mut m = sample(); m.model = "gpt-4o-mini".into();
        db.model_upsert(&m).await.unwrap();
        assert_eq!(db.model_get("gpt-4o").await.unwrap().unwrap().model, "gpt-4o-mini");
        db.model_delete("gpt-4o").await.unwrap();
        assert!(db.model_get("gpt-4o").await.unwrap().is_none());
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib db::models` → FAIL。

- [ ] **Step 3: 实现**

```rust
use crate::db::Db;
use crate::error::FusionError;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct ModelRow {
    pub id: String,
    pub connector: String,
    pub base_url: String,
    pub api_key_enc: Option<String>,
    pub api_key_env: Option<String>,
    pub model: String,
    pub anthropic_version: Option<String>,
    pub extra: Option<String>,
}

impl Db {
    pub async fn model_list(&self) -> Result<Vec<ModelRow>, FusionError> {
        Ok(sqlx::query_as::<_, ModelRow>("SELECT * FROM models ORDER BY id").fetch_all(&self.pool).await?)
    }
    pub async fn model_get(&self, id: &str) -> Result<Option<ModelRow>, FusionError> {
        Ok(sqlx::query_as::<_, ModelRow>("SELECT * FROM models WHERE id = ?")
            .bind(id).fetch_optional(&self.pool).await?)
    }
    pub async fn model_upsert(&self, m: &ModelRow) -> Result<(), FusionError> {
        sqlx::query(
            "INSERT INTO models(id, connector, base_url, api_key_enc, api_key_env, model, anthropic_version, extra)
             VALUES(?,?,?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET connector=excluded.connector, base_url=excluded.base_url,
               api_key_enc=excluded.api_key_enc, api_key_env=excluded.api_key_env,
               model=excluded.model, anthropic_version=excluded.anthropic_version, extra=excluded.extra")
            .bind(&m.id).bind(&m.connector).bind(&m.base_url).bind(&m.api_key_enc)
            .bind(&m.api_key_env).bind(&m.model).bind(&m.anthropic_version).bind(&m.extra)
            .execute(&self.pool).await?;
        Ok(())
    }
    pub async fn model_delete(&self, id: &str) -> Result<(), FusionError> {
        sqlx::query("DELETE FROM models WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }
}
```

(测试块置于文件末尾。)

- [ ] **Step 4: 运行确认通过** — Run: `cargo test --lib db::models` → PASS。

- [ ] **Step 5: 提交**

```bash
git add src/db/models.rs
git commit -m "feat: models 表 CRUD"
```
