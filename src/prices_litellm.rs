//! Parse litellm's model_prices_and_context_window.json into model-id-free PriceValues.
//! Field map (USD/token -> x1e6): input_cost_per_token, output_cost_per_token,
//! cache_read_input_token_cost, cache_creation_input_token_cost. Skips `sample_spec`
//! and any entry without `input_cost_per_token`.

use crate::db::prices::PriceValues;
use crate::db::Db;
use crate::error::FusionError;

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
}
