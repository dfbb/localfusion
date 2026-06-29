//! Daily refresh of the price_defaults snapshot from litellm's GitHub raw JSON.
//! Failure is non-fatal: the existing snapshot (embedded or last good) is kept.

use crate::db::Db;
use crate::error::FusionError;

/// litellm raw JSON (the blob page URL given in the spec is a GitHub HTML page;
/// the raw form below returns the JSON document itself).
pub const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

const DAY_SECS: u64 = 24 * 60 * 60;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Fetch the litellm JSON from `url`, parse it, and replace the price_defaults snapshot.
/// Returns the number of priced models written. On HTTP/parse error, returns Err and
/// leaves the existing snapshot untouched (the replace only runs after a successful parse).
pub async fn refresh_from_url(
    db: &Db,
    http: &reqwest::Client,
    url: &str,
    now: i64,
) -> Result<usize, FusionError> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| FusionError::Internal(format!("price refresh fetch: {e}")))?
        .error_for_status()
        .map_err(|e| FusionError::Internal(format!("price refresh status: {e}")))?;
    let body = resp
        .text()
        .await
        .map_err(|e| FusionError::Internal(format!("price refresh body: {e}")))?;
    let rows = crate::prices_litellm::parse_litellm(&body)?;
    let count = rows.len();
    db.defaults_replace_all(&rows, now).await?;
    Ok(count)
}

/// Background loop: refresh once immediately at startup, then every 24h. Exits on shutdown.
/// A failed refresh logs a warning and keeps the existing snapshot; the next tick retries.
pub fn spawn_price_refresh_loop(db: Db, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    tokio::spawn(async move {
        let http = reqwest::Client::new();
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(DAY_SECS));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    match refresh_from_url(&db, &http, LITELLM_URL, now_secs()).await {
                        Ok(n) => tracing::info!("price_defaults refreshed from litellm: {n} models"),
                        Err(e) => tracing::warn!("price_defaults refresh failed (keeping existing): {e}"),
                    }
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::info!("Price refresh task received shutdown signal, exiting");
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::prices::PriceValues;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn refresh_replaces_snapshot_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{ "gpt-4o": { "input_cost_per_token": 0.0000025, "output_cost_per_token": 0.00001 } }"#,
            ))
            .mount(&server)
            .await;
        let db = Db::open_memory().await.unwrap();
        // pre-seed with a row that the refresh should wipe
        db.defaults_replace_all(
            &[("old".to_string(), PriceValues { price_in: 9.0, price_out: 9.0, cache_read: 0.0, cache_write: 0.0 })],
            1,
        ).await.unwrap();
        let http = reqwest::Client::new();
        let n = refresh_from_url(&db, &http, &server.uri(), 100).await.unwrap();
        assert_eq!(n, 1);
        assert!(db.defaults_match("old").await.unwrap().is_none()); // old row gone
        assert!(db.defaults_match("gpt-4o").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn refresh_errors_on_http_failure_and_keeps_data() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let db = Db::open_memory().await.unwrap();
        db.defaults_replace_all(
            &[("keep".to_string(), PriceValues { price_in: 1.0, price_out: 1.0, cache_read: 0.0, cache_write: 0.0 })],
            1,
        ).await.unwrap();
        let http = reqwest::Client::new();
        let res = refresh_from_url(&db, &http, &server.uri(), 100).await;
        assert!(res.is_err());
        // existing snapshot untouched
        assert!(db.defaults_match("keep").await.unwrap().is_some());
    }
}
