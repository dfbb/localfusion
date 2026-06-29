# Task 01: Schema — `price_defaults` table + `prices` cache columns

**Files:**
- Modify: `src/db/schema.rs` (the `SCHEMA_SQL` const)
- Modify: `src/db/mod.rs` (add a guarded migration after `SCHEMA_SQL` runs, in both `open` and `open_memory`)

**Interfaces:**
- Consumes: nothing (first task).
- Produces: a `price_defaults` table `(model_key TEXT PRIMARY KEY, price_in REAL NOT NULL, price_out REAL NOT NULL, cache_read REAL NOT NULL, cache_write REAL NOT NULL, updated_at INTEGER NOT NULL)`; the existing `prices` table gains `cache_read REAL NOT NULL DEFAULT 0` and `cache_write REAL NOT NULL DEFAULT 0`. Both fresh DBs (via `CREATE TABLE`) and pre-existing DBs (via guarded `ALTER TABLE`) end with these columns.

**Context:** `Db::open` / `Db::open_memory` run `sqlx::query(schema::SCHEMA_SQL)` once at startup. Every statement is `CREATE TABLE IF NOT EXISTS`, so adding columns to the `prices` CREATE only affects brand-new databases — existing deployed DBs already have a `prices` table without the new columns and `CREATE TABLE IF NOT EXISTS` will not alter them. SQLite cannot add a column via `CREATE`, so a separate idempotent `ALTER TABLE ... ADD COLUMN` (ignoring the "duplicate column name" error) is required for upgrades.

- [ ] **Step 1: Add the new columns to the `prices` CREATE and add the `price_defaults` table in `schema.rs`**

In `src/db/schema.rs`, change the `prices` block and add `price_defaults` right after it:

```rust
CREATE TABLE IF NOT EXISTS prices (
  model_id TEXT PRIMARY KEY, price_in REAL NOT NULL, price_out REAL NOT NULL,
  cache_read REAL NOT NULL DEFAULT 0, cache_write REAL NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS price_defaults (
  model_key TEXT PRIMARY KEY, price_in REAL NOT NULL, price_out REAL NOT NULL,
  cache_read REAL NOT NULL DEFAULT 0, cache_write REAL NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL
);
```

- [ ] **Step 2: Add a guarded migration helper in `src/db/mod.rs`**

Add this free function in `src/db/mod.rs` (near `apply_pragmas`). It runs each `ALTER TABLE ADD COLUMN` and treats a "duplicate column name" error as success (column already present):

```rust
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
```

- [ ] **Step 3: Call `run_migrations` after `SCHEMA_SQL` in both `open` and `open_memory`**

In `src/db/mod.rs`, in `Db::open`, immediately after the existing
`sqlx::query(schema::SCHEMA_SQL).execute(&pool).await?;` line and before `restrict_db_permissions(path);`:

```rust
        sqlx::query(schema::SCHEMA_SQL).execute(&pool).await?;
        run_migrations(&pool).await?;
```

In `Db::open_memory`, after its `sqlx::query(schema::SCHEMA_SQL).execute(&pool).await?;`:

```rust
        sqlx::query(schema::SCHEMA_SQL).execute(&pool).await?;
        run_migrations(&pool).await?;
```

- [ ] **Step 4: Write the test (migration idempotency + columns present)**

Add to the `#[cfg(test)] mod tests` in `src/db/mod.rs`:

```rust
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
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --lib db::tests::prices_has_cache_columns_and_price_defaults_table -- --nocapture` and `cargo test --lib db::`
Expected: PASS. Also run `cargo test --lib db::prices::` to confirm the existing `prices` upsert test still passes (it inserts only the 4 original columns; the two new columns have DEFAULT 0 so the old INSERT still works).

- [ ] **Step 6: Commit**

```bash
git add src/db/schema.rs src/db/mod.rs
git commit -m "feat(db): add price_defaults table and prices cache columns with guarded migration"
```
