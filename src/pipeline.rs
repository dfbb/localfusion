use crate::db::usage::UsageDelta;
use crate::db::Db;
use crate::error::FusionError;
use crate::unified::{CallRecorder, CallStatus, ModelUsage};

/// 将时间戳对齐到小时起点
pub fn hour_floor(ts: i64) -> i64 {
    ts - ts.rem_euclid(3600)
}

/// 根据价格表计算单次调用的费用，找不到价格时返回 0.0
pub async fn cost_for(db: &Db, usage: &ModelUsage) -> f64 {
    match db.price_get(&usage.model_id).await {
        Ok(Some(p)) => {
            p.price_in * usage.input_tokens as f64 / 1e6
                + p.price_out * usage.output_tokens as f64 / 1e6
        }
        _ => 0.0,
    }
}

/// 设计 §8：三维度累加 + request_log。
/// all_calls = recorder.drain()（Full 路径）或 drain pre-Started 失败 + 流式 Done.call（Stream 路径）。
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

    // 按真实模型写 real 维度
    for c in all_calls {
        let cost = cost_for(db, c).await;
        agg_in += c.input_tokens;
        agg_out += c.output_tokens;
        agg_cost += cost;
        let d = UsageDelta {
            input_tokens: c.input_tokens,
            output_tokens: c.output_tokens,
            cost,
            errors: (c.status == CallStatus::Failed) as u64,
        };
        db.usage_upsert(hour, "real", &c.model_id, 1, &d).await?;
    }

    // virtual 维度：按虚拟名聚合，errors 以 request_failed 为准
    let req_err = request_failed as u64;
    let vd = UsageDelta {
        input_tokens: agg_in,
        output_tokens: agg_out,
        cost: agg_cost,
        errors: req_err,
    };
    db.usage_upsert(hour, "virtual", virtual_name, 1, &vd).await?;

    // total 维度：全局聚合，model_id 用空字符串
    let td = UsageDelta {
        input_tokens: agg_in,
        output_tokens: agg_out,
        cost: agg_cost,
        errors: req_err,
    };
    db.usage_upsert(hour, "total", "", 1, &td).await?;

    // 写请求日志
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

/// Full 路径：drain recorder 后写库
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
            cost: 0.0,
            status,
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
    async fn cost_uses_prices() {
        let db = Db::open_memory().await.unwrap();
        db.price_upsert(&PriceRow {
            model_id: "m".into(),
            price_in: 1.0,
            price_out: 2.0,
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
}
