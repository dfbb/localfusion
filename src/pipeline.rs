use crate::db::usage::UsageDelta;
use crate::db::Db;
use crate::error::FusionError;
use crate::unified::{CallRecorder, CallStatus, ModelUsage};

/// Align a timestamp to the start of its hour
pub fn hour_floor(ts: i64) -> i64 {
    ts - ts.rem_euclid(3600)
}

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

/// Design §8: accumulate three dimensions + request_log.
/// all_calls = recorder.drain() (Full path) or drain pre-Started failures + streaming Done.call (Stream path).
pub async fn write_stats(
    db: &Db,
    virtual_name: &str,
    strategy: &str,
    all_calls: &[ModelUsage],
    request_failed: bool,
    now_ts: i64,
) -> Result<(), FusionError> {
    let hour = hour_floor(now_ts);
    let mut agg_in = 0u64;
    let mut agg_out = 0u64;
    let mut agg_cost = 0.0f64;

    // Write real dimension keyed by actual model
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

    // virtual dimension: aggregate by virtual name, errors follow request_failed
    let req_err = request_failed as u64;
    let vd = UsageDelta {
        input_tokens: agg_in,
        output_tokens: agg_out,
        cost: agg_cost,
        errors: req_err,
    };
    db.usage_upsert(hour, "virtual", virtual_name, 1, &vd).await?;

    // total dimension: global aggregate, model_id is empty string
    let td = UsageDelta {
        input_tokens: agg_in,
        output_tokens: agg_out,
        cost: agg_cost,
        errors: req_err,
    };
    db.usage_upsert(hour, "total", "", 1, &td).await?;

    // write request log
    let status = if request_failed { "error" } else { "ok" };
    db.request_log_insert(
        virtual_name,
        strategy,
        status,
        (agg_in + agg_out) as i64,
        agg_cost,
        now_ts,
    )
    .await?;

    Ok(())
}

/// Full path: drain recorder then write to database
pub async fn finalize_full(
    db: &Db,
    virtual_name: &str,
    strategy: &str,
    recorder: &CallRecorder,
    request_failed: bool,
    now_ts: i64,
) -> Result<(), FusionError> {
    let calls = recorder.drain();
    write_stats(db, virtual_name, strategy, &calls, request_failed, now_ts).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{prices::PriceRow, Db};
    use crate::unified::{CallRole, CallStatus, ModelUsage};

    fn mu(model: &str, inn: u64, out: u64, status: CallStatus) -> ModelUsage {
        ModelUsage {
            model_id: model.into(),
            role: CallRole::Member,
            input_tokens: inn,
            output_tokens: out,
            billable_input_tokens: inn,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost: 0.0,
            status,
            estimated: false,
            latency_secs: 0.0,
        }
    }

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

    #[test]
    fn hour_floor_aligns() {
        assert_eq!(hour_floor(3661), 3600);
        assert_eq!(hour_floor(7200), 7200);
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

    #[tokio::test]
    async fn cost_uses_prices() {
        let db = Db::open_memory().await.unwrap();
        db.price_upsert(&PriceRow {
            model_id: "m".into(),
            price_in: 1.0,
            price_out: 2.0,
            cache_read: 0.0,
            cache_write: 0.0,
            updated_at: 0,
        })
        .await
        .unwrap();
        let c = cost_for(&db, &mu("m", 1_000_000, 1_000_000, CallStatus::Ok)).await;
        assert!((c - 3.0).abs() < 1e-9);
        assert_eq!(
            cost_for(&db, &mu("x", 100, 100, CallStatus::Ok)).await,
            0.0
        );
    }

    #[tokio::test]
    async fn write_stats_three_scopes() {
        let db = Db::open_memory().await.unwrap();
        let calls = vec![
            mu("a", 1, 1, CallStatus::Ok),
            mu("b", 2, 2, CallStatus::Failed),
        ];
        write_stats(&db, "vf", "synthesize", &calls, false, 3661)
            .await
            .unwrap();
        let real = db.usage_query("real", None, 0, 9999).await.unwrap();
        assert_eq!(real.len(), 2);
        assert_eq!(real.iter().map(|r| r.requests).sum::<i64>(), 2);
        let virt = db.usage_query("virtual", Some("vf"), 0, 9999).await.unwrap();
        assert_eq!(virt[0].requests, 1);
        let total = db.usage_query("total", Some(""), 0, 9999).await.unwrap();
        assert_eq!(total[0].requests, 1);
        assert_eq!(total[0].hour_ts, 3600);
    }

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
}

