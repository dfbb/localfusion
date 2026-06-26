use crate::db::Db;
use crate::error::FusionError;

impl Db {
    pub async fn setting_get(&self, key: &str) -> Result<Option<String>, FusionError> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0))
    }

    pub async fn setting_set(&self, key: &str, value: &str) -> Result<(), FusionError> {
        sqlx::query(
            "INSERT INTO settings(key, value) VALUES(?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn setting_get_or(&self, key: &str, default: &str) -> Result<String, FusionError> {
        Ok(self.setting_get(key).await?.unwrap_or_else(|| default.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Db;

    #[tokio::test]
    async fn set_get_roundtrip_and_default() {
        let db = Db::open_memory().await.unwrap();
        assert_eq!(db.setting_get("log_level").await.unwrap(), None);
        assert_eq!(
            db.setting_get_or("log_level", "info").await.unwrap(),
            "info"
        );
        db.setting_set("log_level", "debug").await.unwrap();
        assert_eq!(
            db.setting_get("log_level").await.unwrap(),
            Some("debug".into())
        );
        db.setting_set("log_level", "error").await.unwrap();
        assert_eq!(
            db.setting_get_or("log_level", "info").await.unwrap(),
            "error"
        );
    }
}
