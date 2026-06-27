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

/// Database handle holding a connection pool. Clone is lightweight (Arc internally).
#[derive(Clone)]
pub struct Db {
    pub pool: SqlitePool,
}

impl Db {
    /// Opens or creates a file-based database; executes three PRAGMAs per connection and runs schema migrations.
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

    /// Opens an in-memory database (for testing); limits connections to 1, also applies PRAGMAs.
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

/// Executed after each new connection is established, ensuring foreign key constraints, WAL mode, and busy timeout are active.
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
        // schema created, table should exist and be empty
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM models")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
        // verify that the foreign_keys PRAGMA is active
        let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(fk, 1);
    }
}
