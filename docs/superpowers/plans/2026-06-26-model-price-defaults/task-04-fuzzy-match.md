# Task 04: `defaults_match` — 4-step fuzzy matcher

**Files:**
- Modify: `src/db/prices.rs` (add `defaults_match` + a private `price_defaults_all` helper)

**Interfaces:**
- Consumes: `PriceValues` (Task 02), the `price_defaults` table populated via `defaults_replace_all` (Task 03).
- Produces: `Db::defaults_match(&self, model: &str) -> Result<Option<PriceValues>, FusionError>` implementing the 4-step match. Used by the admin add-model flow (Task 10).

**Context:** Matching is case-insensitive against `price_defaults.model_key`. Order: (1) exact, (2) normalized exact (`.`→`-`), (3) substring-contains where the normalized model is a substring of a key — among multiple containing keys pick the **shortest**, ties broken by **lexicographically greatest** — (4) none. The matcher loads all keys into memory and matches in Rust (the snapshot is ~2900 rows; simple and avoids encoding the tie-break in SQL). Keep a small `PriceDefaultKey` helper struct for the in-memory rows.

- [ ] **Step 1: Write the failing test**

Add to `src/db/prices.rs` `mod tests`:

```rust
    async fn seed_defaults(db: &Db) {
        let rows = vec![
            ("gpt-4o".to_string(), PriceValues { price_in: 2.5, price_out: 10.0, cache_read: 0.0, cache_write: 0.0 }),
            ("claude-opus-4-8-20260527".to_string(), PriceValues { price_in: 5.0, price_out: 25.0, cache_read: 0.5, cache_write: 6.0 }),
            ("claude-opus-4-8-20260615".to_string(), PriceValues { price_in: 5.0, price_out: 25.0, cache_read: 0.5, cache_write: 6.0 }),
            ("claude-opus-4-8-20260527-thinking".to_string(), PriceValues { price_in: 9.0, price_out: 9.0, cache_read: 0.0, cache_write: 0.0 }),
        ];
        db.defaults_replace_all(&rows, 1).await.unwrap();
    }

    #[tokio::test]
    async fn fuzzy_match_exact_normalized_contains_and_none() {
        let db = Db::open_memory().await.unwrap();
        seed_defaults(&db).await;

        // 1. exact
        assert_eq!(db.defaults_match("gpt-4o").await.unwrap().unwrap().price_in, 2.5);
        // case-insensitive exact
        assert_eq!(db.defaults_match("GPT-4O").await.unwrap().unwrap().price_in, 2.5);
        // 2. normalized exact: '.' -> '-' (no exact "claude-opus-4.8" key, but contains applies after normalize)
        // 3. contains: "claude-opus-4-8" is a substring of three keys; shortest wins,
        //    tie between the two date keys -> lexicographically greatest (20260615).
        let m = db.defaults_match("claude-opus-4.8").await.unwrap().unwrap();
        assert_eq!(m.price_out, 25.0); // matched a date key, not the longer "-thinking" key
        // the chosen key is the shortest containing one; both date keys share length,
        // so lexicographically-greatest 20260615 is picked (price identical here, asserting it resolves)
        // 4. none
        assert!(db.defaults_match("totally-unknown-model").await.unwrap().is_none());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib db::prices::tests::fuzzy_match_exact_normalized_contains_and_none`
Expected: FAIL to compile — `defaults_match` not defined.

- [ ] **Step 3: Implement `defaults_match` and the loader**

Add inside `impl Db` in `src/db/prices.rs`:

```rust
    /// Load all (model_key, PriceValues) from the snapshot (model_key lowercased for matching).
    async fn price_defaults_all(&self) -> Result<Vec<(String, PriceValues)>, FusionError> {
        let rows = sqlx::query_as::<_, (String, f64, f64, f64, f64)>(
            "SELECT model_key, price_in, price_out, cache_read, cache_write FROM price_defaults",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(k, pi, po, cr, cw)| {
                (k, PriceValues { price_in: pi, price_out: po, cache_read: cr, cache_write: cw })
            })
            .collect())
    }

    /// Fuzzy-match a model name against the litellm snapshot. Returns model-id-free
    /// PriceValues. Order: exact -> normalized('.'->'-') exact -> substring-contains
    /// (shortest key wins; ties -> lexicographically greatest) -> None. Case-insensitive.
    pub async fn defaults_match(&self, model: &str) -> Result<Option<PriceValues>, FusionError> {
        let all = self.price_defaults_all().await?;
        if all.is_empty() {
            return Ok(None);
        }
        let needle = model.to_lowercase();
        let normalized = needle.replace('.', "-");

        // Build (lowercased_key, original_index) once.
        let lowered: Vec<(String, usize)> =
            all.iter().enumerate().map(|(i, (k, _))| (k.to_lowercase(), i)).collect();

        // 1. exact on raw needle
        if let Some((_, i)) = lowered.iter().find(|(k, _)| *k == needle) {
            return Ok(Some(all[*i].1.clone()));
        }
        // 2. normalized exact
        if let Some((_, i)) = lowered.iter().find(|(k, _)| *k == normalized) {
            return Ok(Some(all[*i].1.clone()));
        }
        // 3. substring-contains (normalized needle inside key); shortest key, ties -> greatest key
        let best = lowered
            .iter()
            .filter(|(k, _)| k.contains(&normalized))
            .min_by(|a, b| {
                a.0.len()
                    .cmp(&b.0.len())
                    .then_with(|| b.0.cmp(&a.0)) // tie: lexicographically greatest first
            });
        if let Some((_, i)) = best {
            return Ok(Some(all[*i].1.clone()));
        }
        // 4. none
        Ok(None)
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib db::prices::tests::fuzzy_match_exact_normalized_contains_and_none`
Expected: PASS.

- [ ] **Step 5: Add a focused tie-break test (distinct prices to prove selection)**

Add to `mod tests`:

```rust
    #[tokio::test]
    async fn fuzzy_match_contains_prefers_shortest_then_greatest() {
        let db = Db::open_memory().await.unwrap();
        let rows = vec![
            ("m-1-long-suffix".to_string(), PriceValues { price_in: 1.0, price_out: 1.0, cache_read: 0.0, cache_write: 0.0 }),
            ("m-1-aaa".to_string(), PriceValues { price_in: 2.0, price_out: 2.0, cache_read: 0.0, cache_write: 0.0 }),
            ("m-1-zzz".to_string(), PriceValues { price_in: 3.0, price_out: 3.0, cache_read: 0.0, cache_write: 0.0 }),
        ];
        db.defaults_replace_all(&rows, 1).await.unwrap();
        // "m-1" is contained in all three; "m-1-aaa" and "m-1-zzz" are shortest (len 7),
        // tie broken by lexicographically greatest -> "m-1-zzz" (price_in 3.0).
        let m = db.defaults_match("m-1").await.unwrap().unwrap();
        assert_eq!(m.price_in, 3.0);
    }
```

- [ ] **Step 6: Run both fuzzy tests**

Run: `cargo test --lib db::prices::tests::fuzzy_match`
Expected: PASS (both).

- [ ] **Step 7: Commit**

```bash
git add src/db/prices.rs
git commit -m "feat(db): defaults_match 4-step fuzzy price matcher"
```
