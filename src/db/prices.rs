use crate::db::Db;
use crate::error::FusionError;

// 价格行记录：模型 ID、入价（美元/百万 tokens）、出价、更新时间戳
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct PriceRow {
    pub model_id: String,
    pub price_in: f64,
    pub price_out: f64,
    pub updated_at: i64,
}

impl Db {
    /// 获取所有价格记录，按 model_id 排序
    pub async fn price_list(&self) -> Result<Vec<PriceRow>, FusionError> {
        Ok(sqlx::query_as::<_, PriceRow>("SELECT * FROM prices ORDER BY model_id")
            .fetch_all(&self.pool)
            .await?)
    }

    /// 按 model_id 获取单条价格记录，不存在时返回 None
    pub async fn price_get(&self, model_id: &str) -> Result<Option<PriceRow>, FusionError> {
        Ok(sqlx::query_as::<_, PriceRow>("SELECT * FROM prices WHERE model_id = ?")
            .bind(model_id)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// 插入或更新价格行；model_id 冲突时更新 price_in, price_out, updated_at
    pub async fn price_upsert(&self, p: &PriceRow) -> Result<(), FusionError> {
        sqlx::query(
            "INSERT INTO prices(model_id, price_in, price_out, updated_at) VALUES(?,?,?,?)
             ON CONFLICT(model_id) DO UPDATE SET price_in=excluded.price_in,
               price_out=excluded.price_out, updated_at=excluded.updated_at",
        )
        .bind(&p.model_id)
        .bind(p.price_in)
        .bind(p.price_out)
        .bind(p.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_and_get() {
        let db = Db::open_memory().await.unwrap();
        assert!(db.price_get("gpt-4o").await.unwrap().is_none());
        db.price_upsert(&PriceRow {
            model_id: "gpt-4o".into(),
            price_in: 2.5,
            price_out: 10.0,
            updated_at: 100,
        })
        .await
        .unwrap();
        assert_eq!(
            db.price_get("gpt-4o").await.unwrap().unwrap().price_out,
            10.0
        );
        assert_eq!(db.price_list().await.unwrap().len(), 1);
    }
}
