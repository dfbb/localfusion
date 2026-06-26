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

/// 数据库句柄，持有连接池。Clone 是轻量克隆（Arc 内部）。
#[derive(Clone)]
pub struct Db {
    pub pool: SqlitePool,
}

impl Db {
    /// 打开或创建文件数据库；每连接执行三条 PRAGMA，并运行 schema 迁移。
    pub async fn open(path: &str) -> Result<Db, FusionError> {
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{path}"))
            .map_err(|e| FusionError::Internal(format!("db opts: {e}")))?
            .create_if_missing(true)
            .disable_statement_logging();
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .after_connect(|conn, _| Box::pin(async move { apply_pragmas(conn).await }))
            .connect_with(opts)
            .await
            .map_err(|e| FusionError::Internal(format!("db connect: {e}")))?;
        sqlx::query(schema::SCHEMA_SQL).execute(&pool).await?;
        Ok(Db { pool })
    }

    /// 打开内存数据库（用于测试）；连接数限 1，同样执行 PRAGMA。
    pub async fn open_memory() -> Result<Db, FusionError> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(|e| FusionError::Internal(format!("db opts: {e}")))?;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .after_connect(|conn, _| Box::pin(async move { apply_pragmas(conn).await }))
            .connect_with(opts)
            .await
            .map_err(|e| FusionError::Internal(format!("db connect: {e}")))?;
        sqlx::query(schema::SCHEMA_SQL).execute(&pool).await?;
        Ok(Db { pool })
    }
}

/// 每个新连接建立后执行，确保外键约束、WAL 模式、繁忙超时均生效。
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
        // schema 已创建，表应存在且为空
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM models")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
        // 验证 foreign_keys PRAGMA 已生效
        let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(fk, 1);
    }
}
