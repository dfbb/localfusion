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
        run_migrations(&pool).await?;
        // Restrict the DB file to owner-only (0600): it holds encrypted upstream keys and
        // hashed admin/ingress tokens. At-rest encryption is bound to the host machine-id
        // (see crypto::derive_key), so a co-located reader who can open this file could
        // re-derive the key — tight file permissions are the first line of defense.
        restrict_db_permissions(path);
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
        run_migrations(&pool).await?;
        Ok(Db { pool })
    }
}

/// Best-effort: tighten the SQLite file to owner read/write only (0600) on Unix.
/// No-op on other platforms. A failure here is logged but not fatal — the DB still works,
/// it just isn't permission-hardened (e.g. on a filesystem that doesn't support chmod).
#[cfg(unix)]
fn restrict_db_permissions(path: &str) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        if perms.mode() & 0o077 != 0 {
            perms.set_mode(0o600);
            if let Err(e) = std::fs::set_permissions(path, perms) {
                tracing::warn!("could not restrict DB file permissions on {path}: {e}");
            }
        }
    }
}

#[cfg(not(unix))]
fn restrict_db_permissions(_path: &str) {}

/// Executed after each new connection is established, ensuring foreign key constraints, WAL mode, and busy timeout are active.
async fn apply_pragmas(conn: &mut sqlx::SqliteConnection) -> Result<(), sqlx::Error> {
    sqlx::query("PRAGMA foreign_keys = ON;").execute(&mut *conn).await?;
    sqlx::query("PRAGMA journal_mode = WAL;").execute(&mut *conn).await?;
    sqlx::query("PRAGMA busy_timeout = 5000;").execute(&mut *conn).await?;
    Ok(())
}

/// Idempotent column additions for upgrading pre-existing databases.
/// `CREATE TABLE IF NOT EXISTS` never alters an existing table, so columns added to a
/// table's definition over time must be applied with ALTER TABLE. SQLite has no
/// "ADD COLUMN IF NOT EXISTS", so a duplicate-column error is the expected no-op signal.
async fn run_migrations(pool: &SqlitePool) -> Result<(), FusionError> {
    let alters = [
        "ALTER TABLE prices ADD COLUMN cache_read REAL NOT NULL DEFAULT 0",
        "ALTER TABLE prices ADD COLUMN cache_write REAL NOT NULL DEFAULT 0",
    ];
    for sql in alters {
        if let Err(e) = sqlx::query(sql).execute(pool).await {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                return Err(FusionError::from(e));
            }
        }
    }
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

    #[tokio::test]
    async fn prices_has_cache_columns_and_price_defaults_table() {
        let db = Db::open_memory().await.unwrap();
        // price_defaults exists and is empty
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM price_defaults")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(n, 0);
        // prices has the two new columns (insert a row using them succeeds)
        sqlx::query("INSERT INTO prices(model_id,price_in,price_out,cache_read,cache_write,updated_at) VALUES('m',1.0,2.0,0.3,0.5,100)")
            .execute(&db.pool).await.unwrap();
        let cr: f64 = sqlx::query_scalar("SELECT cache_read FROM prices WHERE model_id='m'")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(cr, 0.3);
        // run_migrations is idempotent: a second pass on the same pool is a no-op (no error)
        super::run_migrations(&db.pool).await.unwrap();
    }
}
