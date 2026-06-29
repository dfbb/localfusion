# Task 02: `PriceRow` / `PriceValues` + `price_upsert` (4 fields) + `model_delete_cascade`

**Files:**
- Modify: `src/db/prices.rs` (extend `PriceRow`, add `PriceValues`, update `price_upsert`)
- Modify: `src/db/models.rs` (add `model_delete_cascade`)

**Interfaces:**
- Consumes: the `prices` schema (cache columns) from Task 01.
- Produces:
  - `pub struct PriceRow { model_id: String, price_in: f64, price_out: f64, cache_read: f64, cache_write: f64, updated_at: i64 }` (sqlx::FromRow + Serialize).
  - `pub struct PriceValues { price_in: f64, price_out: f64, cache_read: f64, cache_write: f64 }` (Clone, Debug, PartialEq; Serialize/Deserialize).
  - `Db::price_upsert(&PriceRow)` writing all four price columns + `updated_at`.
  - `Db::model_delete_cascade(&self, id: &str) -> Result<(), FusionError>` deleting the model row and its `prices` row in one transaction.

**Context:** The existing `prices.rs` `PriceRow` has only `price_in/price_out`. `price_get`/`price_list` use `SELECT *` so they will pick up the new columns automatically once the struct fields exist. The transaction pattern is `let mut tx = self.pool.begin().await?; ... ; tx.commit().await?;` (see `src/db/keys.rs:90` and `src/db/virtual_models.rs:55`).

- [ ] **Step 1: Extend `PriceRow` and add `PriceValues` in `src/db/prices.rs`**

Replace the existing `PriceRow` struct (lines ~5-11) with:

```rust
// Per-model price row (USD/million tokens). Read by cost calculation.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct PriceRow {
    pub model_id: String,
    pub price_in: f64,
    pub price_out: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub updated_at: i64,
}

// Model-id-free default prices (USD/million tokens), as matched from the litellm snapshot.
// The caller attaches a local model_id + updated_at to build a PriceRow.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PriceValues {
    pub price_in: f64,
    pub price_out: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}
```

- [ ] **Step 2: Update `price_upsert` to write all four price columns**

Replace the existing `price_upsert` body with:

```rust
    /// Insert or update a price row; on model_id conflict, update all four prices + updated_at.
    pub async fn price_upsert(&self, p: &PriceRow) -> Result<(), FusionError> {
        sqlx::query(
            "INSERT INTO prices(model_id, price_in, price_out, cache_read, cache_write, updated_at)
             VALUES(?,?,?,?,?,?)
             ON CONFLICT(model_id) DO UPDATE SET price_in=excluded.price_in,
               price_out=excluded.price_out, cache_read=excluded.cache_read,
               cache_write=excluded.cache_write, updated_at=excluded.updated_at",
        )
        .bind(&p.model_id)
        .bind(p.price_in)
        .bind(p.price_out)
        .bind(p.cache_read)
        .bind(p.cache_write)
        .bind(p.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
```

- [ ] **Step 3: Fix the existing `prices.rs` test for the new fields**

In `src/db/prices.rs`'s `mod tests`, update the `PriceRow` literal in `upsert_and_get` to include the new fields:

```rust
        db.price_upsert(&PriceRow {
            model_id: "gpt-4o".into(),
            price_in: 2.5,
            price_out: 10.0,
            cache_read: 0.0,
            cache_write: 0.0,
            updated_at: 100,
        })
        .await
        .unwrap();
```

- [ ] **Step 4: Add `model_delete_cascade` in `src/db/models.rs`**

Add this method inside `impl Db` in `src/db/models.rs` (keep the existing `model_delete` as-is):

```rust
    /// Delete a model and its price row atomically. A mid-way failure rolls back both,
    /// so no orphan `prices` row survives a model deletion.
    pub async fn model_delete_cascade(&self, id: &str) -> Result<(), FusionError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM models WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM prices WHERE model_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
```

- [ ] **Step 5: Write tests**

Add to `src/db/prices.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn upsert_writes_cache_prices() {
        let db = Db::open_memory().await.unwrap();
        db.price_upsert(&PriceRow {
            model_id: "m".into(), price_in: 1.0, price_out: 2.0,
            cache_read: 0.3, cache_write: 0.5, updated_at: 1,
        }).await.unwrap();
        let got = db.price_get("m").await.unwrap().unwrap();
        assert_eq!(got.cache_read, 0.3);
        assert_eq!(got.cache_write, 0.5);
    }
```

Add to `src/db/models.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn delete_cascade_removes_price_row() {
        use crate::db::prices::PriceRow;
        let db = Db::open_memory().await.unwrap();
        db.model_upsert(&sample()).await.unwrap();
        db.price_upsert(&PriceRow {
            model_id: "gpt-4o".into(), price_in: 1.0, price_out: 2.0,
            cache_read: 0.0, cache_write: 0.0, updated_at: 1,
        }).await.unwrap();
        db.model_delete_cascade("gpt-4o").await.unwrap();
        assert!(db.model_get("gpt-4o").await.unwrap().is_none());
        assert!(db.price_get("gpt-4o").await.unwrap().is_none());
    }
```

- [ ] **Step 6: Run the tests**

Run: `cargo test --lib db::prices:: db::models::`
Expected: PASS (all, including the updated `upsert_and_get`).

- [ ] **Step 7: Commit**

```bash
git add src/db/prices.rs src/db/models.rs
git commit -m "feat(db): PriceRow cache fields, PriceValues, 4-field upsert, model_delete_cascade"
```
