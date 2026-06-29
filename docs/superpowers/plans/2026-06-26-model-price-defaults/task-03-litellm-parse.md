# Task 03: litellm JSON parser + `defaults_replace_all`

**Files:**
- Create: `src/prices_litellm.rs`
- Modify: `src/lib.rs` (add `pub mod prices_litellm;`)
- Modify: `src/db/prices.rs` (add `defaults_replace_all`)

**Interfaces:**
- Consumes: `PriceValues` (Task 02), the `price_defaults` table (Task 01).
- Produces:
  - `pub fn parse_litellm(json: &str) -> Result<Vec<(String, PriceValues)>, FusionError>` — parses the litellm document into `(model_key, PriceValues)` pairs, skipping `sample_spec` and entries with no `input_cost_per_token`, converting per-token → per-million (×1e6).
  - `Db::defaults_replace_all(&self, rows: &[(String, PriceValues)], updated_at: i64) -> Result<(), FusionError>` — single-transaction full-table rewrite of `price_defaults` (DELETE all, then INSERT each) stamping every row with the caller-provided `updated_at`.

**Context:** The litellm file (`model_prices_and_context_window.json`) is a flat object `{ "<model_key>": { ...fields... }, ... }` (~2900 entries). Each value's cost fields are USD per single token. Field map (verified against the live file): `input_cost_per_token`, `output_cost_per_token`, `cache_read_input_token_cost`, `cache_creation_input_token_cost`. The `sample_spec` key is a schema-doc pseudo-entry. ~472 entries price by second/character/image and lack `input_cost_per_token` — skip them. `now_secs()` is defined in `src/probe.rs` and `src/bootstrap.rs` (private); this task takes the timestamp as a parameter so the function is pure and testable.

- [ ] **Step 1: Write the failing test for `parse_litellm`**

Create `src/prices_litellm.rs` with only the test module first (so the test compiles against the not-yet-written fn signature):

```rust
//! Parse litellm's model_prices_and_context_window.json into model-id-free PriceValues.
//! Field map (USD/token -> x1e6): input_cost_per_token, output_cost_per_token,
//! cache_read_input_token_cost, cache_creation_input_token_cost. Skips `sample_spec`
//! and any entry without `input_cost_per_token`.

use crate::db::prices::PriceValues;
use crate::error::FusionError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_maps_fields_and_skips() {
        let json = r#"{
          "sample_spec": { "input_cost_per_token": 0, "litellm_provider": "x" },
          "gpt-4o": {
            "input_cost_per_token": 0.0000025,
            "output_cost_per_token": 0.00001,
            "cache_read_input_token_cost": 0.00000125
          },
          "claude-x": {
            "input_cost_per_token": 0.000003,
            "output_cost_per_token": 0.000015,
            "cache_read_input_token_cost": 0.0000003,
            "cache_creation_input_token_cost": 0.00000375
          },
          "tts-by-second": { "output_cost_per_second": 0.001 }
        }"#;
        let mut rows = parse_litellm(json).unwrap();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        // sample_spec skipped; tts-by-second skipped (no input_cost_per_token)
        assert_eq!(rows.len(), 2);
        let (k, v) = &rows[0]; // claude-x
        assert_eq!(k, "claude-x");
        assert!((v.price_in - 3.0).abs() < 1e-9);      // 0.000003 * 1e6
        assert!((v.price_out - 15.0).abs() < 1e-9);
        assert!((v.cache_read - 0.3).abs() < 1e-9);
        assert!((v.cache_write - 3.75).abs() < 1e-9);
        let (k2, v2) = &rows[1]; // gpt-4o
        assert_eq!(k2, "gpt-4o");
        assert!((v2.price_in - 2.5).abs() < 1e-9);
        assert_eq!(v2.cache_write, 0.0);               // missing -> 0
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(parse_litellm("{ not json").is_err());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib prices_litellm::tests::parses_maps_fields_and_skips`
Expected: FAIL to compile — `parse_litellm` not defined.

- [ ] **Step 3: Implement `parse_litellm`**

Add above the test module in `src/prices_litellm.rs`:

```rust
const SCALE: f64 = 1e6; // litellm prices are USD per single token; we store per million.

/// Read a litellm cost field, scaling to USD/million tokens; missing/non-number -> 0.0.
fn field(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> f64 {
    obj.get(key).and_then(|v| v.as_f64()).map(|c| c * SCALE).unwrap_or(0.0)
}

/// Parse the litellm document into (model_key, PriceValues) pairs.
/// Skips the `sample_spec` pseudo-entry and any model lacking `input_cost_per_token`
/// (those price by second/character/image and cannot map to per-token pricing).
pub fn parse_litellm(json: &str) -> Result<Vec<(String, PriceValues)>, FusionError> {
    let doc: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| FusionError::Internal(format!("litellm json parse: {e}")))?;
    let map = doc
        .as_object()
        .ok_or_else(|| FusionError::Internal("litellm json is not an object".into()))?;

    let mut out = Vec::new();
    let mut skipped = 0usize;
    for (key, val) in map {
        if key == "sample_spec" {
            continue;
        }
        let Some(obj) = val.as_object() else { continue };
        // Skip entries without a per-token input cost (per-second/char/image pricing).
        if obj.get("input_cost_per_token").and_then(|v| v.as_f64()).is_none() {
            skipped += 1;
            continue;
        }
        out.push((
            key.clone(),
            PriceValues {
                price_in: field(obj, "input_cost_per_token"),
                price_out: field(obj, "output_cost_per_token"),
                cache_read: field(obj, "cache_read_input_token_cost"),
                cache_write: field(obj, "cache_creation_input_token_cost"),
            },
        ));
    }
    tracing::debug!("litellm parse: {} priced models, {} skipped (no per-token cost)", out.len(), skipped);
    Ok(out)
}
```

- [ ] **Step 4: Register the module and run the parse test**

Add to `src/lib.rs` (alongside the other `pub mod` lines): `pub mod prices_litellm;`
Run: `cargo test --lib prices_litellm::`
Expected: PASS (both tests).

- [ ] **Step 5: Add `defaults_replace_all` in `src/db/prices.rs` + test**

Add this method inside `impl Db` in `src/db/prices.rs`:

```rust
    /// Replace the entire price_defaults snapshot in one transaction.
    /// `updated_at` is supplied by the caller (Unix seconds) so the function stays pure.
    pub async fn defaults_replace_all(
        &self,
        rows: &[(String, crate::db::prices::PriceValues)],
        updated_at: i64,
    ) -> Result<(), FusionError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM price_defaults").execute(&mut *tx).await?;
        for (key, v) in rows {
            sqlx::query(
                "INSERT INTO price_defaults(model_key, price_in, price_out, cache_read, cache_write, updated_at)
                 VALUES(?,?,?,?,?,?)",
            )
            .bind(key)
            .bind(v.price_in)
            .bind(v.price_out)
            .bind(v.cache_read)
            .bind(v.cache_write)
            .bind(updated_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
```

Add to `src/db/prices.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn defaults_replace_all_rewrites_snapshot() {
        let db = Db::open_memory().await.unwrap();
        let rows = vec![
            ("a".to_string(), PriceValues { price_in: 1.0, price_out: 2.0, cache_read: 0.0, cache_write: 0.0 }),
            ("b".to_string(), PriceValues { price_in: 3.0, price_out: 4.0, cache_read: 0.1, cache_write: 0.2 }),
        ];
        db.defaults_replace_all(&rows, 100).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM price_defaults")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(n, 2);
        // a second replace with fewer rows fully rewrites (old rows gone)
        db.defaults_replace_all(&rows[..1], 200).await.unwrap();
        let n2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM price_defaults")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(n2, 1);
    }
```

- [ ] **Step 6: Run tests**

Run: `cargo test --lib prices_litellm:: db::prices::`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/prices_litellm.rs src/lib.rs src/db/prices.rs
git commit -m "feat: litellm price parser and price_defaults full-table replace"
```
