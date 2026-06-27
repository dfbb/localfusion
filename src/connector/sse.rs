//! 通用 SSE 出口引擎
//! 字节级 `\n\n` 帧分割（UTF-8 安全），data:/[DONE] 解析，先发 Started 再转发事件

use futures::StreamExt;
use reqwest::header::HeaderMap;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::unified::{ConnError, UnifiedStream, UnifiedStreamEvent};

/// SSE 事件翻译器 trait
/// push：处理一个 JSON 块，返回零或多个统一事件
/// finish：流结束时产出最终事件（如 Done{usage, call, ...}）
/// fail：流中途失败时产出据实统计（status=Failed），默认基于 finish() 的用量改判失败
pub trait SseTranslator: Send {
    fn push(&mut self, chunk: &Value) -> Result<Vec<UnifiedStreamEvent>, ConnError>;
    fn finish(&mut self) -> Vec<UnifiedStreamEvent>;
    /// 流中途失败：取 finish() 已累计的据实用量，状态改为 Failed，
    /// 返回供出口层落统计的 ModelUsage（已 Started 后失败也要记，见设计 §6.1）
    fn fail(&mut self) -> Option<crate::unified::ModelUsage> {
        use crate::unified::CallStatus;
        for ev in self.finish() {
            if let UnifiedStreamEvent::Done { call: Some(mut c), .. } = ev {
                c.status = CallStatus::Failed;
                return Some(c);
            }
        }
        None
    }
}

/// 同步发 POST + 状态码校验后 spawn 异步读取任务，返回 UnifiedStream
pub async fn run_egress(
    url: String,
    headers: HeaderMap,
    body: Value,
    http: reqwest::Client,
    mut translator: Box<dyn SseTranslator>,
    model_id: String,
) -> Result<UnifiedStream, ConnError> {
    let resp = http
        .post(&url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| ConnError::Http(format!("request failed: {e}")))?;

    // 提取上游请求 ID（可选）
    let upstream_request_id = resp
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(ConnError::Http(format!("upstream {status}: {text}")));
    }

    let (tx, rx) = mpsc::channel::<Result<UnifiedStreamEvent, ConnError>>(64);

    // 先发送 Started 事件
    let _ = tx.send(Ok(UnifiedStreamEvent::Started { model_id })).await;

    let mut byte_stream = resp.bytes_stream();

    tokio::spawn(async move {
        let mut buf: Vec<u8> = Vec::new();
        let mut done = false;

        while let Some(chunk) = byte_stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let call = translator.fail();
                    let _ = tx
                        .send(Ok(UnifiedStreamEvent::Error {
                            message: format!("stream error: {e}"),
                            call,
                        }))
                        .await;
                    return;
                }
            };
            buf.extend_from_slice(&chunk);

            // 字节级 \n\n 切帧
            while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                let frame = buf[..pos].to_vec();
                buf.drain(..pos + 2);

                // UTF-8 安全解码完整帧
                let block = match std::str::from_utf8(&frame) {
                    Ok(s) => s.to_string(),
                    Err(e) => {
                        let call = translator.fail();
                        let _ = tx
                            .send(Ok(UnifiedStreamEvent::Error {
                                message: format!("utf8: {e}"),
                                call,
                            }))
                            .await;
                        return;
                    }
                };

                for line in block.lines() {
                    let line = line.trim_start();
                    let data = match line.strip_prefix("data:") {
                        Some(d) => d.trim(),
                        None => continue,
                    };
                    if data == "[DONE]" {
                        done = true;
                        break;
                    }
                    if data.is_empty() {
                        continue;
                    }
                    let json: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(e) => {
                            let call = translator.fail();
                            let _ = tx
                                .send(Ok(UnifiedStreamEvent::Error {
                                    message: format!("bad json: {e}"),
                                    call,
                                }))
                                .await;
                            return;
                        }
                    };
                    match translator.push(&json) {
                        Ok(events) => {
                            for ev in events {
                                if tx.send(Ok(ev)).await.is_err() {
                                    return;
                                }
                            }
                        }
                        Err(ce) => {
                            let call = translator.fail();
                            let _ = tx
                                .send(Ok(UnifiedStreamEvent::Error {
                                    message: ce.to_string(),
                                    call,
                                }))
                                .await;
                            return;
                        }
                    }
                }
                if done {
                    break;
                }
            }
            if done {
                break;
            }
        }

        // 流结束，translator.finish() 负责产出 Done{usage, call, ...}
        for ev in translator.finish() {
            if tx.send(Ok(ev)).await.is_err() {
                return;
            }
        }
    });

    Ok(UnifiedStream {
        rx,
        upstream_request_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unified::UnifiedStreamEvent;

    struct T;
    impl SseTranslator for T {
        fn push(&mut self, chunk: &serde_json::Value) -> Result<Vec<UnifiedStreamEvent>, ConnError> {
            if chunk.get("boom").is_some() {
                return Err(ConnError::Http("boom".into()));
            }
            if let Some(t) = chunk.get("t").and_then(|v| v.as_str()) {
                return Ok(vec![UnifiedStreamEvent::TextDelta { text: t.into() }]);
            }
            Ok(vec![])
        }
        fn finish(&mut self) -> Vec<UnifiedStreamEvent> {
            use crate::unified::{CallRole, CallStatus, ModelUsage, Usage};
            vec![UnifiedStreamEvent::Done {
                usage: Usage { input_tokens: 5, output_tokens: 7 },
                call: Some(ModelUsage {
                    model_id: "m1".into(),
                    role: CallRole::Member,
                    input_tokens: 5,
                    output_tokens: 7,
                    cost: 0.0,
                    status: CallStatus::Ok,
                    estimated: true,
                    latency_secs: 0.0,
                }),
                finish_reason: None,
            }]
        }
    }

    #[tokio::test]
    async fn streams_started_then_deltas_then_closes() {
        let server = wiremock::MockServer::start().await;
        let body = "data: {\"t\":\"你好\"}\n\ndata: {\"t\":\"世界\"}\n\ndata: [DONE]\n\n";
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .mount(&server)
            .await;
        let mut s = run_egress(
            format!("{}/v1", server.uri()),
            reqwest::header::HeaderMap::new(),
            serde_json::json!({}),
            reqwest::Client::new(),
            Box::new(T),
            "m1".into(),
        )
        .await
        .unwrap();
        let mut events = Vec::new();
        while let Some(ev) = s.rx.recv().await {
            events.push(ev.unwrap());
        }
        assert!(matches!(events[0], UnifiedStreamEvent::Started { .. }));
        let texts: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                UnifiedStreamEvent::TextDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["你好", "世界"]);
    }

    #[tokio::test]
    async fn non_2xx_returns_err_before_stream() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(401).set_body_string("nope"))
            .mount(&server)
            .await;
        let r = run_egress(
            format!("{}/v1", server.uri()),
            reqwest::header::HeaderMap::new(),
            serde_json::json!({}),
            reqwest::Client::new(),
            Box::new(T),
            "m1".into(),
        )
        .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn mid_stream_failure_emits_error_with_failed_call() {
        use crate::unified::CallStatus;
        let server = wiremock::MockServer::start().await;
        // 第二帧触发 translator.push 失败（已 Started 并产出过 token）
        let body = "data: {\"t\":\"hi\"}\n\ndata: {\"boom\":1}\n\n";
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .mount(&server)
            .await;
        let mut s = run_egress(
            format!("{}/v1", server.uri()),
            reqwest::header::HeaderMap::new(),
            serde_json::json!({}),
            reqwest::Client::new(),
            Box::new(T),
            "m1".into(),
        )
        .await
        .unwrap();
        let mut err_call = None;
        while let Some(ev) = s.rx.recv().await {
            if let Ok(UnifiedStreamEvent::Error { call, .. }) = ev {
                err_call = call;
            }
        }
        // 中途失败必须产出 Error 事件，且携带 status=Failed 的据实用量（供统计）
        let call = err_call.expect("expected Error event with call");
        assert_eq!(call.status, CallStatus::Failed);
    }
}
