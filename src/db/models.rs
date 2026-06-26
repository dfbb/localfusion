use crate::db::Db;
use crate::error::FusionError;

/// 模型表行结构体。
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
    /// 列出所有模型（按 id 排序）。
    pub async fn model_list(&self) -> Result<Vec<ModelRow>, FusionError> {
        Ok(sqlx::query_as::<_, ModelRow>("SELECT * FROM models ORDER BY id")
            .fetch_all(&self.pool)
            .await?)
    }

    /// 按 id 获取单个模型；不存在时返回 None。
    pub async fn model_get(&self, id: &str) -> Result<Option<ModelRow>, FusionError> {
        Ok(sqlx::query_as::<_, ModelRow>("SELECT * FROM models WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// Upsert 模型行（不存在时插入，存在时更新）。
    pub async fn model_upsert(&self, m: &ModelRow) -> Result<(), FusionError> {
        sqlx::query(
            "INSERT INTO models(id, connector, base_url, api_key_enc, api_key_env, model, anthropic_version, extra)
             VALUES(?,?,?,?,?,?,?,?)
             ON CONFLICT(id) DO UPDATE SET connector=excluded.connector, base_url=excluded.base_url,
               api_key_enc=excluded.api_key_enc, api_key_env=excluded.api_key_env,
               model=excluded.model, anthropic_version=excluded.anthropic_version, extra=excluded.extra",
        )
        .bind(&m.id)
        .bind(&m.connector)
        .bind(&m.base_url)
        .bind(&m.api_key_enc)
        .bind(&m.api_key_env)
        .bind(&m.model)
        .bind(&m.anthropic_version)
        .bind(&m.extra)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 按 id 删除模型行。
    pub async fn model_delete(&self, id: &str) -> Result<(), FusionError> {
        sqlx::query("DELETE FROM models WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ModelRow {
        ModelRow {
            id: "gpt-4o".into(),
            connector: "chat".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key_enc: Some("ENC".into()),
            api_key_env: None,
            model: "gpt-4o".into(),
            anthropic_version: None,
            extra: None,
        }
    }

    #[tokio::test]
    async fn crud_cycle() {
        let db = Db::open_memory().await.unwrap();
        assert!(db.model_list().await.unwrap().is_empty());
        db.model_upsert(&sample()).await.unwrap();
        assert_eq!(
            db.model_get("gpt-4o")
                .await
                .unwrap()
                .unwrap()
                .model,
            "gpt-4o"
        );
        let mut m = sample();
        m.model = "gpt-4o-mini".into();
        db.model_upsert(&m).await.unwrap();
        assert_eq!(
            db.model_get("gpt-4o")
                .await
                .unwrap()
                .unwrap()
                .model,
            "gpt-4o-mini"
        );
        db.model_delete("gpt-4o").await.unwrap();
        assert!(db.model_get("gpt-4o").await.unwrap().is_none());
    }
}
