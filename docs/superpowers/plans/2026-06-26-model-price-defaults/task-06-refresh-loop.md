# Task 06: Daily refresh fetch fn + `spawn_price_refresh_loop` + main wiring

**Files:**
- Create: `src/price_refresh.rs`
- Modify: `src/lib.rs` (add `pub mod price_refresh;`)
- Modify: `src/main.rs` (seed on startup + spawn the refresh loop)

**Interfaces:**
- Consumes: `parse_litellm` + `seed_defaults_if_empty` + `embedded_snapshot` (Tasks 03/05), `defaults_replace_all` (Task 03).
- Produces:
  - `pub async fn refresh_from_url(db: &Db, http: &reqwest::Client, url: &str, now: i64) -> Result<usize, FusionError>` — GET the URL, parse, replace the snapshot; returns the row count written. Errors propagate (caller logs + ignores).
  - `pub fn spawn_price_refresh_loop(db: Db, shutdown: tokio::sync::watch::Receiver<bool>)` — runs once immediately, then every 24h, mirroring `spawn_probe_loop`.

**Context:** Mirror `src/probe.rs::spawn_probe_loop` exactly (tokio::spawn + interval + watch shutdown + select!). The litellm raw URL is `https://raw.githubusercontent.com/BerriAI/litellm/litellm_internal_staging/model_prices_and_context_window.json`. Use a plain `reqwest::Client` for the fetch (this is a fixed, trusted GitHub URL — not a user-supplied base_url — so the SSRF/redirect concerns of `router::egress_client` don't apply; a default client is fine). Refresh failure must be non-fatal: log `warn`, keep existing data. `now_secs()` is private in probe.rs/bootstrap.rs; define a local copy in this module (consistent with the codebase, which duplicates it).

- [ ] **Step 1: Write the failing test (wiremock success + failure)**

Create `src/price_refresh.rs` with the test module referencing the not-yet-written fns:

```rust
//! Daily refresh of the price_defaults snapshot from litellm's GitHub raw JSON.
//! Failure is non-fatal: the existing snapshot (embedded or last good) is kept.

use crate::db::Db;
use crate::error::FusionError;

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
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib price_refresh::tests::refresh_replaces_snapshot_on_success`
Expected: FAIL to compile — `refresh_from_url` not defined. (Add `pub mod price_refresh;` to `src/lib.rs` first so the module is seen.)

- [ ] **Step 3: Implement `refresh_from_url`, `spawn_price_refresh_loop`, and the URL const**

Add to `src/price_refresh.rs` (above the test module):

```rust
/// litellm raw JSON (the blob page URL given in the spec is a GitHub HTML page;
/// the raw form below returns the JSON document itself).
pub const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/litellm_internal_staging/model_prices_and_context_window.json";

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
```

Note: `tokio::time::interval` fires its first tick immediately, so the loop refreshes on startup as required.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib price_refresh::`
Expected: PASS (both wiremock tests).

- [ ] **Step 5: Wire seeding + the loop into `src/main.rs`**

In `src/main.rs`, just after the existing probe spawn block:

```rust
    // Probe background task (exits when shutdown signal received)
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    localfusion::probe::spawn_probe_loop(db.clone(), enc_key, 1800, shutdown_rx.clone());

    // Seed price defaults from the embedded snapshot if the table is empty, then refresh daily.
    if let Err(e) = localfusion::prices_litellm::seed_defaults_if_empty(&db, now_secs_main()).await {
        tracing::warn!("price_defaults seed failed: {e}");
    }
    localfusion::price_refresh::spawn_price_refresh_loop(db.clone(), shutdown_rx);
```

Two adjustments needed in `main.rs`:
1. The existing probe spawn currently passes `shutdown_rx` by move. Change it to `shutdown_rx.clone()` (as shown) so the refresh loop can take the receiver too; `watch::Receiver` is `Clone`.
2. Provide `now_secs_main()` — if `main.rs` has no Unix-seconds helper, add a small private fn near the top of `main.rs`:

```rust
fn now_secs_main() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
```

(If `main.rs` already imports/derives a now-seconds value, reuse it instead and skip this helper.)

- [ ] **Step 6: Build to verify wiring compiles**

Run: `cargo build`
Expected: builds clean. Run `cargo clippy --all-targets` and confirm no new warnings.

- [ ] **Step 7: Commit**

```bash
git add src/price_refresh.rs src/lib.rs src/main.rs
git commit -m "feat: daily litellm price refresh loop + startup seed wiring"
```
