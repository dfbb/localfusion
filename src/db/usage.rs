use crate::db::Db;
use crate::error::FusionError;

/// Incremental statistics for a single request
pub struct UsageDelta {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: f64,
    pub errors: u64,
}

/// A single row from the usage_hourly table, corresponding to a query result
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct UsageRow {
    pub hour_ts: i64,
    pub scope: String,
    pub name: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cost: f64,
    pub errors: i64,
}

impl Db {
    /// Atomic accumulation: write delta into usage_hourly; if the row exists, accumulate by three dimensions (hour_ts, scope, name)
    pub async fn usage_upsert(
        &self,
        hour_ts: i64,
        scope: &str,
        name: &str,
        requests: u64,
        d: &UsageDelta,
    ) -> Result<(), FusionError> {
        let total = (d.input_tokens + d.output_tokens) as i64;
        sqlx::query(
            "INSERT INTO usage_hourly(hour_ts,scope,name,requests,input_tokens,output_tokens,total_tokens,cost,errors)
             VALUES(?,?,?,?,?,?,?,?,?)
             ON CONFLICT(hour_ts,scope,name) DO UPDATE SET
               requests=requests+excluded.requests,
               input_tokens=input_tokens+excluded.input_tokens,
               output_tokens=output_tokens+excluded.output_tokens,
               total_tokens=total_tokens+excluded.total_tokens,
               cost=cost+excluded.cost,
               errors=errors+excluded.errors",
        )
        .bind(hour_ts)
        .bind(scope)
        .bind(name)
        .bind(requests as i64)
        .bind(d.input_tokens as i64)
        .bind(d.output_tokens as i64)
        .bind(total)
        .bind(d.cost)
        .bind(d.errors as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Query aggregated rows by scope / name (optional) / time range, returned in ascending hour_ts order
    pub async fn usage_query(
        &self,
        scope: &str,
        name: Option<&str>,
        from_ts: i64,
        to_ts: i64,
    ) -> Result<Vec<UsageRow>, FusionError> {
        let rows = match name {
            Some(n) => sqlx::query_as::<_, UsageRow>(
                "SELECT * FROM usage_hourly WHERE scope=? AND name=? AND hour_ts BETWEEN ? AND ? ORDER BY hour_ts",
            )
            .bind(scope)
            .bind(n)
            .bind(from_ts)
            .bind(to_ts)
            .fetch_all(&self.pool)
            .await?,
            None => sqlx::query_as::<_, UsageRow>(
                "SELECT * FROM usage_hourly WHERE scope=? AND hour_ts BETWEEN ? AND ? ORDER BY hour_ts",
            )
            .bind(scope)
            .bind(from_ts)
            .bind(to_ts)
            .fetch_all(&self.pool)
            .await?,
        };
        Ok(rows)
    }

    /// Insert a request_log record (external request dimension, scope=virtual/total)
    pub async fn request_log_insert(
        &self,
        virtual_name: &str,
        strategy: &str,
        status: &str,
        total_tokens: i64,
        cost: f64,
        created_at: i64,
    ) -> Result<(), FusionError> {
        sqlx::query(
            "INSERT INTO request_log(virtual_name, strategy, status, total_tokens, cost, created_at)
             VALUES(?,?,?,?,?,?)",
        )
        .bind(virtual_name)
        .bind(strategy)
        .bind(status)
        .bind(total_tokens)
        .bind(cost)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[tokio::test]
    async fn upsert_accumulates() {
        let db = Db::open_memory().await.unwrap();
        let d = UsageDelta { input_tokens: 5, output_tokens: 7, cost: 0.1, errors: 0 };
        db.usage_upsert(1000, "total", "", 1, &d).await.unwrap();
        db.usage_upsert(1000, "total", "", 1, &d).await.unwrap();
        let rows = db.usage_query("total", None, 0, 2000).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].requests, 2);
        assert_eq!(rows[0].input_tokens, 10);
        assert_eq!(rows[0].total_tokens, 24);
        assert!((rows[0].cost - 0.2).abs() < 1e-9);
    }

    #[tokio::test]
    async fn query_filters_by_scope_name_and_time() {
        let db = Db::open_memory().await.unwrap();
        let d = UsageDelta { input_tokens: 1, output_tokens: 1, cost: 0.0, errors: 1 };
        db.usage_upsert(1000, "real", "gpt-4o", 1, &d).await.unwrap();
        db.usage_upsert(1000, "real", "claude", 1, &d).await.unwrap();
        db.usage_upsert(5000, "real", "gpt-4o", 1, &d).await.unwrap();
        let rows = db.usage_query("real", Some("gpt-4o"), 0, 2000).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].errors, 1);
        assert_eq!(db.usage_query("real", None, 0, 9000).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn request_log_writes() {
        let db = Db::open_memory().await.unwrap();
        db.request_log_insert("vf", "failover", "ok", 12, 0.01, 100).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_log")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }
}
