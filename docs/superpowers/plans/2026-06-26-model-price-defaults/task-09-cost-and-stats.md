# Task 09: Cost formula (4 terms) + `write_stats` folds cache into input

**Files:**
- Modify: `src/pipeline.rs` (`cost_for` + `write_stats`)

**Interfaces:**
- Consumes: `PriceRow` cache fields (Task 02), `ModelUsage` `billable_input_tokens`/`cache_read_tokens`/`cache_write_tokens` (Tasks 07/08).
- Produces: cost includes all four token classes; usage statistics record the uniform true input (`billable + cache_read + cache_write`) so cache tokens don't vanish from totals.

**Context:** `cost_for(db, usage)` currently does `price_in*input_tokens/1e6 + price_out*output_tokens/1e6`. `write_stats` aggregates `agg_in += c.input_tokens` and writes `usage_hourly`/`request_log`. Because `input_tokens` means different things per provider (Anthropic excludes cache, OpenAI includes it), the stat aggregation must use the disjoint sum to stay uniform and complete.

- [ ] **Step 1: Write the failing cost test**

In `src/pipeline.rs` `mod tests`, the helper `mu(model, inn, out, status)` builds a `ModelUsage`. Add an overload-style helper and a test. First add a helper that sets cache fields:

```rust
    fn mu_cache(model: &str, billable: u64, out: u64, cr: u64, cw: u64) -> ModelUsage {
        ModelUsage {
            model_id: model.into(),
            role: CallRole::Member,
            input_tokens: billable + cr + cw,
            output_tokens: out,
            billable_input_tokens: billable,
            cache_read_tokens: cr,
            cache_write_tokens: cw,
            cost: 0.0,
            status: CallStatus::Ok,
            estimated: false,
            latency_secs: 0.0,
        }
    }

    #[tokio::test]
    async fn cost_includes_cache_terms() {
        use crate::db::prices::PriceRow;
        let db = Db::open_memory().await.unwrap();
        db.price_upsert(&PriceRow {
            model_id: "m".into(), price_in: 2.0, price_out: 4.0,
            cache_read: 1.0, cache_write: 8.0, updated_at: 1,
        }).await.unwrap();
        // billable=1e6, out=1e6, cache_read=1e6, cache_write=1e6
        let c = cost_for(&db, &mu_cache("m", 1_000_000, 1_000_000, 1_000_000, 1_000_000)).await;
        // 2 + 4 + 1 + 8 = 15.0
        assert!((c - 15.0).abs() < 1e-9, "got {c}");
        // no-cache case still equals input*price_in + out*price_out
        let c2 = cost_for(&db, &mu_cache("m", 1_000_000, 0, 0, 0)).await;
        assert!((c2 - 2.0).abs() < 1e-9, "got {c2}");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib pipeline::tests::cost_includes_cache_terms`
Expected: FAIL — cost still 6.0 (only input+output), not 15.0.

- [ ] **Step 3: Update `cost_for`**

Replace the body of `cost_for` in `src/pipeline.rs`:

```rust
/// Calculate the cost of a single call from the price table; returns 0.0 if no price is found.
/// Bills four disjoint token classes: non-cached input, cache-read, cache-write, output.
pub async fn cost_for(db: &Db, usage: &ModelUsage) -> f64 {
    match db.price_get(&usage.model_id).await {
        Ok(Some(p)) => {
            p.price_in * usage.billable_input_tokens as f64 / 1e6
                + p.cache_read * usage.cache_read_tokens as f64 / 1e6
                + p.cache_write * usage.cache_write_tokens as f64 / 1e6
                + p.price_out * usage.output_tokens as f64 / 1e6
        }
        _ => 0.0,
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib pipeline::tests::cost_includes_cache_terms`
Expected: PASS.

- [ ] **Step 5: Write the failing stat-folding test**

Add to `src/pipeline.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn write_stats_folds_cache_into_input_dimension() {
        let db = Db::open_memory().await.unwrap();
        // Anthropic-style: input_tokens excludes cache; billable=50, cache_read=10, cache_write=20 => true input 80
        let calls = vec![mu_cache("m", 50, 5, 10, 20)];
        write_stats(&db, "vf", "synthesize", &calls, false, 3661).await.unwrap();
        // request_log.total_tokens should be true_input(80) + output(5) = 85
        let tot: i64 = sqlx::query_scalar("SELECT total_tokens FROM request_log LIMIT 1")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(tot, 85);
        // usage_hourly 'total' scope input_tokens should be 80
        let inp: i64 = sqlx::query_scalar("SELECT input_tokens FROM usage_hourly WHERE scope='total'")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(inp, 80);
    }
```

- [ ] **Step 6: Run to verify it fails**

Run: `cargo test --lib pipeline::tests::write_stats_folds_cache_into_input_dimension`
Expected: FAIL — current code records `input_tokens` (50), so total is 55 and input is 50.

- [ ] **Step 7: Update `write_stats` aggregation**

In `src/pipeline.rs::write_stats`, the per-call loop currently does `agg_in += c.input_tokens;` and builds the "real" `UsageDelta` with `input_tokens: c.input_tokens`. Change both to the disjoint true-input sum. Define it once per call and reuse:

```rust
    for c in all_calls {
        let cost = cost_for(db, c).await;
        // Uniform true input = non-cached input + cache-read + cache-write (disjoint classes).
        // Folds cache tokens into the input dimension so they aren't lost from stats, and is
        // provider-uniform (Anthropic input excludes cache; OpenAI includes it — both reconcile here).
        let stat_in = c.billable_input_tokens + c.cache_read_tokens + c.cache_write_tokens;
        agg_in += stat_in;
        agg_out += c.output_tokens;
        agg_cost += cost;
        let d = UsageDelta {
            input_tokens: stat_in,
            output_tokens: c.output_tokens,
            cost,
            errors: (c.status == CallStatus::Failed) as u64,
        };
        db.usage_upsert(hour, "real", &c.model_id, 1, &d).await?;
    }
```

(The `virtual` and `total` deltas already use `agg_in`/`agg_out`, and `request_log` uses `agg_in + agg_out`, so they pick up the corrected sum automatically.)

- [ ] **Step 8: Run both pipeline tests + the existing write_stats test**

Run: `cargo test --lib pipeline::`
Expected: PASS. (The pre-existing `write_stats` test at pipeline.rs ~150 uses calls built by `mu(...)`; Task 07 set `billable_input_tokens == input_tokens` and cache=0 in that helper, so `stat_in == input_tokens` and the old assertions still hold.)

- [ ] **Step 9: Commit**

```bash
git add src/pipeline.rs
git commit -m "feat(pipeline): bill four token classes and fold cache tokens into usage stats"
```
