// tests/admin_api.rs — 管理 API 集成测试
use localfusion::admin::{router, AdminState};
use localfusion::db::Db;
use std::sync::Arc;

async fn app() -> axum::Router {
    let db = Db::open_memory().await.unwrap();
    db.setting_set("admin_token_hash", &localfusion::crypto::sha256_hex("adm"))
        .await
        .unwrap();
    let log = Arc::new(localfusion::logging::init("info", None, false));
    router(AdminState {
        db,
        log,
        enc_key: [0u8; 32],
    })
}

#[tokio::test]
async fn health_requires_admin_token() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = app().await;

    // 无 token → 401
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // 带正确 token → 200
    let r = app
        .oneshot(
            Request::builder()
                .uri("/admin/api/health")
                .header("authorization", "Bearer adm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn models_crud_and_conflict() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    let db = Db::open_memory().await.unwrap();
    db.setting_set("admin_token_hash", &localfusion::crypto::sha256_hex("adm"))
        .await
        .unwrap();
    let log = Arc::new(localfusion::logging::init("info", None, false));
    let app = router(AdminState {
        db,
        log,
        enc_key: [0u8; 32],
    });

    // POST /admin/api/models — 创建模型
    let body = serde_json::json!({
        "id": "m1",
        "connector": "chat",
        "base_url": "http://localhost",
        "model": "gpt-4o",
    })
    .to_string();

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/admin/api/models")
                .header("authorization", "Bearer adm")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    // GET /admin/api/models — 列表非空
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/api/models")
                .header("authorization", "Bearer adm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    // 创建 virtual-model 使 m1 成为 member
    let vbody = serde_json::json!({
        "name": "vf",
        "strategy": "failover",
        "params": {},
        "members": ["m1"],
    })
    .to_string();
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/admin/api/virtual-models")
                .header("authorization", "Bearer adm")
                .header("content-type", "application/json")
                .body(Body::from(vbody))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    // DELETE /admin/api/models/m1 → 409（被引用）
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/admin/api/models/m1")
                .header("authorization", "Bearer adm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn keys_create_and_list() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    let db = Db::open_memory().await.unwrap();
    db.setting_set("admin_token_hash", &localfusion::crypto::sha256_hex("adm"))
        .await
        .unwrap();
    let log = Arc::new(localfusion::logging::init("info", None, false));
    let app = router(AdminState {
        db,
        log,
        enc_key: [0u8; 32],
    });

    // POST /admin/api/keys
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/admin/api/keys")
                .header("authorization", "Bearer adm")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"label":"test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    // GET /admin/api/keys
    let r = app
        .oneshot(
            Request::builder()
                .uri("/admin/api/keys")
                .header("authorization", "Bearer adm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_all_models_empty_db_returns_empty_array() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = app().await; // existing helper: in-memory DB, token="adm", enc_key=[0u8;32]

    let r = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/api/models/test-all")
                .header("Authorization", "Bearer adm")
                .header("Content-Type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(r.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 0); // no models in empty DB
}

// ─── probe integration (wiremock-backed) ─────────────────────────────────────

/// Build an app sharing a specific Db, so a test can seed a model and then assert the
/// probe's persistence side effects on the same DB.
async fn app_with_db(db: &Db) -> axum::Router {
    db.setting_set("admin_token_hash", &localfusion::crypto::sha256_hex("adm"))
        .await
        .unwrap();
    let log = Arc::new(localfusion::logging::init("info", None, false));
    router(AdminState {
        db: db.clone(),
        log,
        enc_key: [0u8; 32],
    })
}

async fn probe_one_model(app: axum::Router, id: &str) -> serde_json::Value {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let r = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/api/models/{id}/test"))
                .header("Authorization", "Bearer adm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn probe_detects_and_persists_v1_fixup() {
    use localfusion::db::models::ModelRow;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Upstream only answers at /v1/chat/completions, not /chat/completions.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "pong"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        })))
        .mount(&server)
        .await;

    let db = Db::open_memory().await.unwrap();
    std::env::set_var("PROBE_IT_KEY", "k");
    // Configured base_url lacks the /v1 segment — the probe should discover and persist it.
    db.model_upsert(&ModelRow {
        id: "m".into(),
        connector: "chat".into(),
        base_url: server.uri(),
        api_key_enc: None,
        api_key_env: Some("PROBE_IT_KEY".into()),
        model: "gpt".into(),
        anthropic_version: None,
        extra: None,
    })
    .await
    .unwrap();

    let app = app_with_db(&db).await;
    let result = probe_one_model(app, "m").await;
    assert_eq!(result["ok"], true);
    assert_eq!(result["base_url_fixed"], format!("{}/v1", server.uri()));

    // Persistence: the DB row now carries the corrected base_url.
    let row = db.model_get("m").await.unwrap().unwrap();
    assert_eq!(row.base_url, format!("{}/v1", server.uri()));
}

#[tokio::test]
async fn probe_rejects_wrong_shaped_200() {
    use localfusion::db::models::ModelRow;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Every path returns a 200 that is NOT a completion (e.g. a gateway health page).
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "message": "ok"
        })))
        .mount(&server)
        .await;

    let db = Db::open_memory().await.unwrap();
    std::env::set_var("PROBE_IT_KEY2", "k");
    db.model_upsert(&ModelRow {
        id: "m".into(),
        connector: "chat".into(),
        base_url: server.uri(),
        api_key_enc: None,
        api_key_env: Some("PROBE_IT_KEY2".into()),
        model: "gpt".into(),
        anthropic_version: None,
        extra: None,
    })
    .await
    .unwrap();

    let app = app_with_db(&db).await;
    let result = probe_one_model(app, "m").await;
    // A bare non-completion 200 must NOT be accepted as a working connector.
    assert_eq!(result["ok"], false);

    // Persistence: the connector must be left unchanged (no wrong combo written).
    let row = db.model_get("m").await.unwrap().unwrap();
    assert_eq!(row.connector, "chat");
}

// ─── price-fill + cascade integration tests ──────────────────────────────────

// price precedence: explicit prices on POST are written and override fuzzy match
#[tokio::test]
async fn post_model_with_explicit_prices_writes_them() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    let db = Db::open_memory().await.unwrap();
    db.defaults_replace_all(
        &[("gpt-4o".into(), localfusion::db::prices::PriceValues { price_in: 2.5, price_out: 10.0, cache_read: 0.0, cache_write: 0.0 })],
        1,
    ).await.unwrap();
    let app = app_with_db(&db).await;
    let body = serde_json::json!({
        "id": "my-gpt", "connector": "chat", "base_url": "http://127.0.0.1:1234/v1", "model": "gpt-4o",
        "price_in": 1.0, "price_out": 2.0, "cache_read": 0.3, "cache_write": 0.4
    });
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
    let p = db.price_get("my-gpt").await.unwrap().unwrap();
    assert_eq!(p.price_in, 1.0);
    assert_eq!(p.cache_write, 0.4);
}

// fuzzy fallback only when no explicit prices and no existing row
#[tokio::test]
async fn post_model_without_prices_fills_from_defaults() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    let db = Db::open_memory().await.unwrap();
    db.defaults_replace_all(
        &[("gpt-4o".into(), localfusion::db::prices::PriceValues { price_in: 2.5, price_out: 10.0, cache_read: 0.0, cache_write: 0.0 })],
        1,
    ).await.unwrap();
    let app = app_with_db(&db).await;
    let body = serde_json::json!({
        "id": "g", "connector": "chat", "base_url": "http://127.0.0.1/v1", "model": "gpt-4o"
    });
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
    let p = db.price_get("g").await.unwrap().unwrap();
    assert_eq!(p.price_in, 2.5);
}

// repeat POST with no prices must NOT clobber an existing hand-set price
#[tokio::test]
async fn repeat_post_preserves_hand_set_prices() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    let db = Db::open_memory().await.unwrap();
    db.defaults_replace_all(
        &[("gpt-4o".into(), localfusion::db::prices::PriceValues { price_in: 2.5, price_out: 10.0, cache_read: 0.0, cache_write: 0.0 })],
        1,
    ).await.unwrap();
    db.price_upsert(&localfusion::db::prices::PriceRow {
        model_id: "g".into(), price_in: 99.0, price_out: 99.0, cache_read: 0.0, cache_write: 0.0, updated_at: 1,
    }).await.unwrap();
    // Also need the model row to exist for the upsert to work
    db.model_upsert(&localfusion::db::models::ModelRow {
        id: "g".into(), connector: "chat".into(), base_url: "http://127.0.0.1/v1".into(),
        api_key_enc: None, api_key_env: None, model: "gpt-4o".into(), anthropic_version: None, extra: None,
    }).await.unwrap();
    let app = app_with_db(&db).await;
    let body = serde_json::json!({
        "id": "g", "connector": "chat", "base_url": "http://127.0.0.1/v1", "model": "gpt-4o"
    });
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
    assert_eq!(db.price_get("g").await.unwrap().unwrap().price_in, 99.0);
}

// invalid price -> 400 and NO model created
#[tokio::test]
async fn post_invalid_price_returns_400_and_no_model() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    let db = Db::open_memory().await.unwrap();
    let app = app_with_db(&db).await;
    let body = serde_json::json!({
        "id": "bad", "connector": "chat", "base_url": "http://127.0.0.1/v1", "model": "x", "price_in": -1
    });
    let r = app.clone().oneshot(
        Request::builder()
            .method(Method::POST)
            .uri("/admin/api/models")
            .header("authorization", "Bearer adm")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(db.model_get("bad").await.unwrap().is_none());
}

// PUT prices 404 for missing model
#[tokio::test]
async fn put_prices_404_for_missing_model() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    let db = Db::open_memory().await.unwrap();
    let app = app_with_db(&db).await;
    let body = serde_json::json!({ "price_in": 1.0, "price_out": 1.0, "cache_read": 0.0, "cache_write": 0.0 });
    let r = app.clone().oneshot(
        Request::builder()
            .method(Method::PUT)
            .uri("/admin/api/models/nope/prices")
            .header("authorization", "Bearer adm")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    ).await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert!(db.price_get("nope").await.unwrap().is_none());
}

// delete cascades the price row
#[tokio::test]
async fn delete_model_removes_price_row() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    let db = Db::open_memory().await.unwrap();
    db.model_upsert(&localfusion::db::models::ModelRow {
        id: "d".into(), connector: "chat".into(), base_url: "http://127.0.0.1/v1".into(),
        api_key_enc: None, api_key_env: None, model: "x".into(), anthropic_version: None, extra: None,
    }).await.unwrap();
    db.price_upsert(&localfusion::db::prices::PriceRow {
        model_id: "d".into(), price_in: 1.0, price_out: 1.0, cache_read: 0.0, cache_write: 0.0, updated_at: 1,
    }).await.unwrap();
    let app = app_with_db(&db).await;
    let r = app.clone().oneshot(
        Request::builder()
            .method(Method::DELETE)
            .uri("/admin/api/models/d")
            .header("authorization", "Bearer adm")
            .body(Body::empty())
            .unwrap(),
    ).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(db.price_get("d").await.unwrap().is_none());
}

#[tokio::test]
async fn probe_parses_and_persists_output_cap_from_error() {
    use localfusion::db::models::ModelRow;
    use wiremock::matchers::{body_string_contains, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    // The 1M probe is rejected with the real cap stated in the error.
    Mock::given(method("POST"))
        .and(body_string_contains("1000000"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {"message": "the valid range of max_tokens is [1, 393216]"}
        })))
        .mount(&server)
        .await;
    // Any other max_tokens (the reachability ping and the 393216 confirm) succeeds.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "pong"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        })))
        .mount(&server)
        .await;

    let db = Db::open_memory().await.unwrap();
    std::env::set_var("PROBE_IT_KEY3", "k");
    db.model_upsert(&ModelRow {
        id: "m".into(),
        connector: "chat".into(),
        base_url: format!("{}/v1", server.uri()),
        api_key_enc: None,
        api_key_env: Some("PROBE_IT_KEY3".into()),
        model: "gpt".into(),
        anthropic_version: None,
        extra: None,
    })
    .await
    .unwrap();

    let app = app_with_db(&db).await;
    let result = probe_one_model(app, "m").await;
    assert_eq!(result["ok"], true);
    assert_eq!(result["max_tokens"], 393216);

    // Persistence: default_max_tokens round-trips into extra.
    let row = db.model_get("m").await.unwrap().unwrap();
    let extra: serde_json::Value = serde_json::from_str(row.extra.as_deref().unwrap()).unwrap();
    assert_eq!(extra["default_max_tokens"], 393216);
}
