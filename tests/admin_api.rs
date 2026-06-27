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
