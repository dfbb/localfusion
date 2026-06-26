use axum::http::HeaderMap;
use localfusion::db::Db;
use localfusion::db::{models::ModelRow, virtual_models::VirtualModelRow};
use localfusion::ingress::handler::{handle, InferenceState, Proto};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn chat_e2e_failover_single_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices":[{"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":1,"completion_tokens":1}
        })))
        .mount(&server)
        .await;
    let db = Db::open_memory().await.unwrap();
    std::env::set_var("E2E_KEY", "k");
    db.model_upsert(&ModelRow {
        id: "m".into(),
        connector: "chat".into(),
        base_url: format!("{}/v1", server.uri()),
        api_key_enc: None,
        api_key_env: Some("E2E_KEY".into()),
        model: "gpt".into(),
        anthropic_version: None,
        extra: None,
    })
    .await
    .unwrap();
    db.vmodel_upsert(
        &VirtualModelRow {
            name: "vf".into(),
            strategy: "failover".into(),
            params: "{}".into(),
        },
        &["m".into()],
    )
    .await
    .unwrap();
    let id = db.key_insert("sk-1", None, 0).await.unwrap();
    db.key_set_acl(id, true, &[]).await.unwrap();
    let mut h = HeaderMap::new();
    h.insert("authorization", "Bearer sk-1".parse().unwrap());
    let resp = handle(
        InferenceState {
            db: db.clone(),
            enc_key: [0u8; 32],
        },
        Proto::Chat,
        h,
        serde_json::json!({"model":"vf","messages":[{"role":"user","content":"hi"}]}),
    )
    .await;
    assert_eq!(resp.status(), 200);
    // 统计已落库
    let total = db.usage_query("total", Some(""), 0, i64::MAX).await.unwrap();
    assert_eq!(total.iter().map(|r| r.requests).sum::<i64>(), 1);
}
