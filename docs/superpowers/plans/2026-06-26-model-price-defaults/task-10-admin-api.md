# Task 10: Admin API — price-fill on add, `PUT /models/:id/prices`, delete cascade

**Files:**
- Modify: `src/admin/api.rs` (`upsert_from_body` price step, `create_model`/`update_model` pass-through, new `update_prices` handler + route, `delete_model` cascade)

**Interfaces:**
- Consumes: `defaults_match` (Task 04), `price_get`/`price_upsert`/`PriceRow` (Task 02/04), `model_get`/`model_delete_cascade` (Task 02).
- Produces: `PUT /admin/api/models/:id/prices` route; add-model fills/overrides prices; delete cascades. No new types exposed downstream.

**Context:** Handlers return `axum::response::Response`; errors go through `err_response(FusionError)` which maps `InvalidRequest`→400 (`src/admin/mod.rs:35`). There is no `NotFound` variant, so the 404 in the prices handler is built inline with `StatusCode::NOT_FOUND` (the codebase already does this, e.g. api.rs:470). `now_secs()` is not in scope in api.rs — add a small private helper. `upsert_from_body` currently returns `Result<(), FusionError>`; the price logic must run after the model upsert succeeds but price *validation* must run before the model write.

- [ ] **Step 1: Add a `now_secs` helper + price validation/extraction helpers in `src/admin/api.rs`**

Near the top of `src/admin/api.rs` (after the imports), add:

```rust
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Read an optional price field from a JSON body. Returns:
///   Ok(None)      -> field absent or JSON null (not present)
///   Ok(Some(v))   -> a finite, non-negative number
///   Err(..)       -> present but invalid (non-number, NaN/inf, or negative) => 400
fn price_field(body: &serde_json::Value, key: &str) -> Result<Option<f64>, crate::error::FusionError> {
    use crate::error::FusionError;
    match body.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => {
            let n = v.as_f64().ok_or_else(|| {
                FusionError::InvalidRequest(format!("{key} must be a number"))
            })?;
            if !n.is_finite() || n < 0.0 {
                return Err(FusionError::InvalidRequest(format!(
                    "{key} must be a finite non-negative number"
                )));
            }
            Ok(Some(n))
        }
    }
}
```

- [ ] **Step 2: Add the price step to `upsert_from_body`**

`upsert_from_body(s, body, id_override)` ends with `s.db.model_upsert(&row).await`. Change the tail so prices are validated BEFORE the model write and resolved after. Replace the final `s.db.model_upsert(&row).await` line with:

```rust
    // Validate price fields BEFORE writing the model, so an invalid price never mutates the model.
    let pin = price_field(body, "price_in")?;
    let pout = price_field(body, "price_out")?;
    let pcr = price_field(body, "cache_read")?;
    let pcw = price_field(body, "cache_write")?;
    let any_price = pin.or(pout).or(pcr).or(pcw).is_some();

    let model_name = row.model.clone();
    let model_id = row.id.clone();
    s.db.model_upsert(&row).await?;

    if any_price {
        // Explicit prices win (including on a repeat POST): absent fields default to 0.
        s.db.price_upsert(&crate::db::prices::PriceRow {
            model_id: model_id.clone(),
            price_in: pin.unwrap_or(0.0),
            price_out: pout.unwrap_or(0.0),
            cache_read: pcr.unwrap_or(0.0),
            cache_write: pcw.unwrap_or(0.0),
            updated_at: now_secs(),
        })
        .await?;
    } else if s.db.price_get(&model_id).await?.is_none() {
        // No explicit prices and no existing row: fill from the litellm snapshot if matched.
        if let Some(v) = s.db.defaults_match(&model_name).await? {
            s.db.price_upsert(&crate::db::prices::PriceRow {
                model_id: model_id.clone(),
                price_in: v.price_in,
                price_out: v.price_out,
                cache_read: v.cache_read,
                cache_write: v.cache_write,
                updated_at: now_secs(),
            })
            .await?;
        }
    }
    // else: no explicit prices but a price row already exists -> leave it untouched
    // (a repeat POST must never clobber hand-set prices with a default).
    Ok(())
```

(The function signature stays `Result<(), FusionError>`; `create_model`/`update_model` are unchanged — they already map the result through `err_response`.)

- [ ] **Step 3: Add the `update_prices` handler + register the route**

Add the handler:

```rust
/// PUT /admin/api/models/:id/prices
/// body: { price_in, price_out, cache_read, cache_write } — all four required, finite, >= 0.
/// 404 if the model does not exist (prevents orphan price rows).
async fn update_prices(
    State(s): State<AdminState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    // Model must exist (prices has no FK; enforce here to avoid orphan rows).
    match s.db.model_get(&id).await {
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "model not found"}))).into_response()
        }
        Err(e) => return err_response(e),
        Ok(Some(_)) => {}
    }
    // All four fields required (full replace). Reuse price_field but reject absent.
    let require = |key: &str| -> Result<f64, crate::error::FusionError> {
        match price_field(&body, key)? {
            Some(v) => Ok(v),
            None => Err(crate::error::FusionError::InvalidRequest(format!("{key} is required"))),
        }
    };
    let row = match (require("price_in"), require("price_out"), require("cache_read"), require("cache_write")) {
        (Ok(pi), Ok(po), Ok(cr), Ok(cw)) => crate::db::prices::PriceRow {
            model_id: id.clone(), price_in: pi, price_out: po, cache_read: cr, cache_write: cw,
            updated_at: now_secs(),
        },
        (e1, e2, e3, e4) => {
            // Surface the first error.
            for r in [e1, e2, e3, e4] {
                if let Err(e) = r {
                    return err_response(e);
                }
            }
            unreachable!()
        }
    };
    match s.db.price_upsert(&row).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}
```

Register the route in `models_routes`:

```rust
pub fn models_routes() -> Router<AdminState> {
    Router::new()
        .route("/admin/api/models", get(list_models).post(create_model))
        .route("/admin/api/models/:id", put(update_model).delete(delete_model))
        .route("/admin/api/models/:id/prices", put(update_prices))
}
```

- [ ] **Step 4: Make `delete_model` cascade**

In `delete_model`, change the success branch from `s.db.model_delete(&id)` to `s.db.model_delete_cascade(&id)`:

```rust
        Ok(_) => match s.db.model_delete_cascade(&id).await {
            Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
            Err(e) => err_response(e),
        },
```

- [ ] **Step 5: Build + clippy**

Run: `cargo build && cargo clippy --all-targets`
Expected: clean. (Confirm `StatusCode`, `put`, `Path`, `Json`, `Value` are already imported in api.rs — they are, used by existing handlers.)

- [ ] **Step 6: Integration tests (wiremock-free; direct admin app)**

These follow the existing admin API test pattern in `tests/admin_api.rs` (build the router with a `Db`, drive it with tower `oneshot`). There is no `app_with_db` helper — the existing tests construct the db inline. `Db` is `Clone` (Arc inside), so build the router with `db.clone()` and keep `db` to assert on afterward. Add a local helper at the top of `tests/admin_api.rs`:

```rust
// Build an admin router over an existing db (admin token = "adm"); keeps db for assertions.
async fn app_with_db(db: &Db) -> axum::Router {
    db.setting_set("admin_token_hash", &localfusion::crypto::sha256_hex("adm"))
        .await
        .unwrap();
    let log = Arc::new(localfusion::logging::init("info", None, false));
    router(AdminState { db: db.clone(), log, enc_key: [0u8; 32] })
}
```

Each test: create `let db = Db::open_memory().await.unwrap();`, seed as needed, `let app = app_with_db(&db).await;`, drive a request, assert status, then assert on `db.price_get(...)`. Request construction mirrors the existing tests — POST example:

```rust
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    let r = app.clone().oneshot(
        Request::builder()
            .method(Method::POST)
            .uri("/admin/api/models")
            .header("authorization", "Bearer adm")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
```

Tests to add (the `db.*` assertions are the substantive checks):

```rust
// price precedence: explicit prices on POST are written and override fuzzy match
#[tokio::test]
async fn post_model_with_explicit_prices_writes_them() {
    let db = Db::open_memory().await.unwrap();
    db.defaults_replace_all(&[("gpt-4o".into(), localfusion::db::prices::PriceValues{price_in:2.5,price_out:10.0,cache_read:0.0,cache_write:0.0})], 1).await.unwrap();
    let app = app_with_db(&db).await;
    let body = serde_json::json!({
        "id":"my-gpt","connector":"chat","base_url":"http://127.0.0.1:1234/v1","model":"gpt-4o",
        "price_in": 1.0, "price_out": 2.0, "cache_read": 0.3, "cache_write": 0.4
    });
    // POST as above, assert 200, then:
    let p = db.price_get("my-gpt").await.unwrap().unwrap();
    assert_eq!(p.price_in, 1.0);
    assert_eq!(p.cache_write, 0.4);
}

// fuzzy fallback only when no explicit prices and no existing row
#[tokio::test]
async fn post_model_without_prices_fills_from_defaults() {
    let db = Db::open_memory().await.unwrap();
    db.defaults_replace_all(&[("gpt-4o".into(), localfusion::db::prices::PriceValues{price_in:2.5,price_out:10.0,cache_read:0.0,cache_write:0.0})], 1).await.unwrap();
    let app = app_with_db(&db).await;
    // POST {id:"g", connector:"chat", base_url:"http://127.0.0.1/v1", model:"gpt-4o"} (no price fields), assert 200, then:
    let p = db.price_get("g").await.unwrap().unwrap();
    assert_eq!(p.price_in, 2.5);
}

// repeat POST with no prices must NOT clobber an existing hand-set price
#[tokio::test]
async fn repeat_post_preserves_hand_set_prices() {
    let db = Db::open_memory().await.unwrap();
    db.defaults_replace_all(&[("gpt-4o".into(), localfusion::db::prices::PriceValues{price_in:2.5,price_out:10.0,cache_read:0.0,cache_write:0.0})], 1).await.unwrap();
    db.price_upsert(&localfusion::db::prices::PriceRow{model_id:"g".into(),price_in:99.0,price_out:99.0,cache_read:0.0,cache_write:0.0,updated_at:1}).await.unwrap();
    let app = app_with_db(&db).await;
    // POST {id:"g", connector:"chat", base_url:"http://127.0.0.1/v1", model:"gpt-4o"} with NO price fields, assert 200, then:
    assert_eq!(db.price_get("g").await.unwrap().unwrap().price_in, 99.0);
}

// invalid price -> 400 and NO model created
#[tokio::test]
async fn post_invalid_price_returns_400_and_no_model() {
    let db = Db::open_memory().await.unwrap();
    let app = app_with_db(&db).await;
    // POST {id:"bad", connector:"chat", base_url:"http://127.0.0.1/v1", model:"x", price_in:-1}, assert 400, then:
    assert!(db.model_get("bad").await.unwrap().is_none());
}

// PUT prices 404 for missing model
#[tokio::test]
async fn put_prices_404_for_missing_model() {
    let db = Db::open_memory().await.unwrap();
    let app = app_with_db(&db).await;
    // PUT /admin/api/models/nope/prices {price_in:1,price_out:1,cache_read:0,cache_write:0}, assert 404, then:
    assert!(db.price_get("nope").await.unwrap().is_none());
}

// delete cascades the price row
#[tokio::test]
async fn delete_model_removes_price_row() {
    let db = Db::open_memory().await.unwrap();
    db.model_upsert(&localfusion::db::models::ModelRow{id:"d".into(),connector:"chat".into(),base_url:"http://127.0.0.1/v1".into(),api_key_enc:None,api_key_env:None,model:"x".into(),anthropic_version:None,extra:None}).await.unwrap();
    db.price_upsert(&localfusion::db::prices::PriceRow{model_id:"d".into(),price_in:1.0,price_out:1.0,cache_read:0.0,cache_write:0.0,updated_at:1}).await.unwrap();
    let app = app_with_db(&db).await;
    // DELETE /admin/api/models/d, assert 200, then:
    assert!(db.price_get("d").await.unwrap().is_none());
}
```

> Read `tests/admin_api.rs` first and mirror its exact request/assert style (it already has `models_crud_and_conflict` doing POST/PUT/DELETE against `/admin/api/models`). `PriceValues`/`PriceRow`/`ModelRow` are `pub` under `localfusion::db::prices` / `localfusion::db::models`.

- [ ] **Step 7: Run the admin API tests**

Run: `cargo test --test admin_api`
Expected: PASS (new tests + existing).

- [ ] **Step 8: Commit**

```bash
git add src/admin/api.rs tests/admin_api.rs
git commit -m "feat(admin): price-fill on add, PUT model prices (404-guarded), delete cascade"
```
