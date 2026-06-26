use crate::db::Db;
use crate::error::FusionError;

impl Db {
    pub async fn latency_insert(&self, model_id: &str, tokens_out: i64, output_secs: f64,
        is_probe: bool, created_at: i64) -> Result<(), FusionError> {
        let throughput = if output_secs > 0.0 { tokens_out as f64 / output_secs } else { 0.0 };
        sqlx::query("INSERT INTO latency_samples(model_id, tokens_out, output_secs, throughput, is_probe, created_at)
             VALUES(?,?,?,?,?,?)")
            .bind(model_id).bind(tokens_out).bind(output_secs).bind(throughput)
            .bind(is_probe as i64).bind(created_at).execute(&self.pool).await?;
        Ok(())
    }
    /// 设计 §4：子查询取最近 limit 条再平均。
    pub async fn latency_avg_recent(&self, model_id: &str, limit: i64) -> Result<Option<f64>, FusionError> {
        let v: Option<f64> = sqlx::query_scalar(
            "SELECT AVG(throughput) FROM (
               SELECT throughput FROM latency_samples
               WHERE model_id = ? ORDER BY created_at DESC LIMIT ?)")
            .bind(model_id).bind(limit).fetch_one(&self.pool).await?;
        Ok(v)
    }
    pub async fn latency_models_without_recent(&self, since_ts: i64) -> Result<Vec<String>, FusionError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT model_id FROM latency_samples
             WHERE model_id NOT IN (
               SELECT DISTINCT model_id FROM latency_samples WHERE created_at >= ?)")
            .bind(since_ts).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }
    /// 最近 limit 条样本数（用于 latency 统计展示）
    pub async fn latency_sample_count(&self, model_id: &str, limit: i64) -> Result<i64, FusionError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM (
               SELECT 1 FROM latency_samples
               WHERE model_id = ? ORDER BY created_at DESC LIMIT ?)")
            .bind(model_id).bind(limit).fetch_one(&self.pool).await?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Db;
    #[tokio::test]
    async fn avg_recent_uses_last_n_subquery() {
        let db = Db::open_memory().await.unwrap();
        assert_eq!(db.latency_avg_recent("m", 10).await.unwrap(), None);
        db.latency_insert("m", 10, 1.0, false, 1).await.unwrap(); // throughput 10
        db.latency_insert("m", 40, 2.0, false, 2).await.unwrap(); // 20
        db.latency_insert("m", 90, 3.0, false, 3).await.unwrap(); // 30
        let avg = db.latency_avg_recent("m", 2).await.unwrap().unwrap(); // 最近2条 30,20 → 25
        assert!((avg - 25.0).abs() < 1e-9, "got {avg}");
    }
    #[tokio::test]
    async fn models_without_recent() {
        let db = Db::open_memory().await.unwrap();
        db.latency_insert("old", 10, 1.0, false, 100).await.unwrap();
        db.latency_insert("fresh", 10, 1.0, false, 500).await.unwrap();
        let stale = db.latency_models_without_recent(300).await.unwrap();
        assert!(stale.contains(&"old".to_string()));
        assert!(!stale.contains(&"fresh".to_string()));
    }
}
