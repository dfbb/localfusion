# Task 05: Bundle litellm snapshot + seed `price_defaults` on startup

**Files:**
- Create: `assets/litellm_model_prices.json` (committed snapshot, ~1.5MB)
- Create: `scripts/update-litellm-snapshot.sh` (manual snapshot updater)
- Modify: `src/prices_litellm.rs` (add `embedded_snapshot()` + `seed_defaults_if_empty`)
- Modify: `src/db/prices.rs` (add `price_defaults_count`)

**Interfaces:**
- Consumes: `parse_litellm` + `defaults_replace_all` (Task 03), `defaults_match`/snapshot table (Task 04).
- Produces:
  - `pub fn embedded_snapshot() -> &'static str` returning the compiled-in litellm JSON.
  - `pub async fn seed_defaults_if_empty(db: &Db, now: i64) -> Result<(), FusionError>` — if `price_defaults` is empty, parse the embedded snapshot and `defaults_replace_all` it; otherwise no-op.
  - `Db::price_defaults_count(&self) -> Result<i64, FusionError>`.

**Context:** The snapshot is embedded with `include_str!` (a compile-time string include — simpler than rust-embed for a single file, and the file is plain UTF-8 JSON). Committing a 1.5MB JSON is acceptable (the repo already embeds the built frontend). The daily refresh (Task 06) keeps it fresh at runtime; this task only provides the build-time default and the empty-table seed. `now` is passed in (the caller uses `now_secs()`), keeping the seed function testable.

- [ ] **Step 1: Fetch and commit the snapshot via a script**

Create `scripts/update-litellm-snapshot.sh`:

```bash
#!/usr/bin/env bash
# Refresh the bundled litellm price snapshot. Run manually to update the build-time default.
# The running server also refreshes this data daily into the DB (see src/price_refresh.rs).
set -euo pipefail
URL="https://raw.githubusercontent.com/BerriAI/litellm/litellm_internal_staging/model_prices_and_context_window.json"
DEST="$(dirname "$0")/../assets/litellm_model_prices.json"
mkdir -p "$(dirname "$DEST")"
curl -fsSL --max-time 60 "$URL" -o "$DEST"
echo "wrote $DEST ($(wc -c < "$DEST") bytes)"
```

Make it executable and run it to produce the committed snapshot:

```bash
chmod +x scripts/update-litellm-snapshot.sh
./scripts/update-litellm-snapshot.sh
```

Expected: writes `assets/litellm_model_prices.json` (~1.5MB). Verify it is valid JSON and contains `gpt-4o`:
`python3 -c "import json;d=json.load(open('assets/litellm_model_prices.json'));print(len(d),'gpt-4o' in d)"`
Expected: prints a count (~2900) and `True`.

- [ ] **Step 2: Add `price_defaults_count` in `src/db/prices.rs` + test**

Add inside `impl Db`:

```rust
    /// Number of rows in the price_defaults snapshot.
    pub async fn price_defaults_count(&self) -> Result<i64, FusionError> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM price_defaults")
            .fetch_one(&self.pool)
            .await?)
    }
```

- [ ] **Step 3: Write the failing test for `seed_defaults_if_empty`**

Add to `src/prices_litellm.rs` `mod tests`:

```rust
    use crate::db::Db;

    #[tokio::test]
    async fn seed_fills_when_empty_and_is_idempotent() {
        let db = Db::open_memory().await.unwrap();
        assert_eq!(db.price_defaults_count().await.unwrap(), 0);
        seed_defaults_if_empty(&db, 100).await.unwrap();
        let n = db.price_defaults_count().await.unwrap();
        assert!(n > 100, "embedded snapshot should seed many rows, got {n}");
        // gpt-4o is matchable after seeding
        assert!(db.defaults_match("gpt-4o").await.unwrap().is_some());
        // second call is a no-op (table non-empty): count unchanged
        seed_defaults_if_empty(&db, 200).await.unwrap();
        assert_eq!(db.price_defaults_count().await.unwrap(), n);
    }

    #[test]
    fn embedded_snapshot_is_parseable() {
        let rows = parse_litellm(embedded_snapshot()).unwrap();
        assert!(rows.iter().any(|(k, _)| k == "gpt-4o"));
    }
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test --lib prices_litellm::tests::embedded_snapshot_is_parseable`
Expected: FAIL to compile — `embedded_snapshot` / `seed_defaults_if_empty` not defined.

- [ ] **Step 5: Implement `embedded_snapshot` + `seed_defaults_if_empty`**

Add to `src/prices_litellm.rs` (after `parse_litellm`):

```rust
use crate::db::Db;

/// The litellm price snapshot compiled into the binary at build time.
pub fn embedded_snapshot() -> &'static str {
    include_str!("../assets/litellm_model_prices.json")
}

/// Seed the price_defaults snapshot from the embedded JSON if (and only if) the table
/// is empty. Idempotent: a non-empty table is left untouched (the daily refresh owns
/// updates thereafter). `now` is the snapshot's updated_at (Unix seconds).
pub async fn seed_defaults_if_empty(db: &Db, now: i64) -> Result<(), FusionError> {
    if db.price_defaults_count().await? > 0 {
        return Ok(());
    }
    let rows = parse_litellm(embedded_snapshot())?;
    db.defaults_replace_all(&rows, now).await?;
    tracing::info!("seeded price_defaults from embedded litellm snapshot: {} rows", rows.len());
    Ok(())
}
```

Note: `use crate::db::Db;` may already be present from the test module's `use` — place the non-test `use` at the top of the file with the other imports (next to `use crate::db::prices::PriceValues;`), not inside the function, and remove the duplicate from the test module if it now conflicts.

- [ ] **Step 6: Run the tests**

Run: `cargo test --lib prices_litellm::`
Expected: PASS (parse tests + `embedded_snapshot_is_parseable` + `seed_fills_when_empty_and_is_idempotent`).

- [ ] **Step 7: Commit**

```bash
git add assets/litellm_model_prices.json scripts/update-litellm-snapshot.sh src/prices_litellm.rs src/db/prices.rs
git commit -m "feat: embed litellm snapshot and seed price_defaults when empty"
```
