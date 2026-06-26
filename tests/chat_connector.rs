//! ChatConnector 集成测试（使用 wiremock 模拟上游 OpenAI API）

use localfusion::connector::{make_connector, AuthKind, ConnectorKind, EgressCtx};
use localfusion::unified::*;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ctx(base: String) -> EgressCtx {
    EgressCtx {
        base_url: base,
        model: "gpt-4o".into(),
        auth: AuthKind::Bearer,
        key: Some("k".into()),
        anthropic_version: None,
        default_max_tokens: None,
        http: reqwest::Client::new(),
    }
}

fn req() -> UnifiedRequest {
    UnifiedRequest {
        items: vec![Item::Message {
            role: Role::User,
            content: vec![ContentBlock::Text("hi".into())],
        }],
        tools: vec![],
        max_tokens: Some(50),
        temperature: None,
        stream: false,
        raw_extra: serde_json::Value::Null,
    }
}

/// 验证非流式补全：usage 和 calls 字段正确填充
#[tokio::test]
async fn complete_against_fake_openai() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hello"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 2, "completion_tokens": 3}
        })))
        .mount(&server)
        .await;

    let c = make_connector(ConnectorKind::Chat);
    let r = c
        .complete(&req(), &ctx(format!("{}/v1", server.uri())))
        .await
        .unwrap();

    assert_eq!(r.usage.output_tokens, 3);
    assert_eq!(r.usage.input_tokens, 2);
    assert_eq!(r.calls.len(), 1);
    assert_eq!(r.calls[0].output_tokens, 3);
    assert_eq!(r.model_id, "gpt-4o");
}

/// 验证上游 4xx 错误时返回 ConnError::Http
#[tokio::test]
async fn complete_returns_err_on_upstream_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .mount(&server)
        .await;

    let c = make_connector(ConnectorKind::Chat);
    let err = c
        .complete(&req(), &ctx(format!("{}/v1", server.uri())))
        .await
        .unwrap_err();

    assert!(matches!(err, localfusion::unified::ConnError::Http(_)));
}
