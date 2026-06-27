//! Generic SSE egress engine
//! Byte-level `\n\n` frame splitting (UTF-8 safe), data:/[DONE] parsing, sends Started before forwarding events

use futures::StreamExt;
use reqwest::header::HeaderMap;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::unified::{ConnError, UnifiedStream, UnifiedStreamEvent};

/// SSE frame-splitting buffer limit (hard cap on accumulated bytes when a frame is not yet closed).
/// Normal SSE frames are well below this value; exceeding it indicates upstream anomaly, triggering controlled failure instead of unbounded memory growth.
const MAX_FRAME_BUFFER: usize = 1024 * 1024;

/// SSE event translator trait
/// push: processes one JSON chunk, returns zero or more unified events
/// finish: produces final events when the stream ends (e.g. Done{usage, call, ...})
/// fail: produces actual-usage stats on mid-stream failure (status=Failed); defaults to repurposing finish() usage with Failed status
pub trait SseTranslator: Send {
    fn push(&mut self, chunk: &Value) -> Result<Vec<UnifiedStreamEvent>, ConnError>;
    fn finish(&mut self) -> Vec<UnifiedStreamEvent>;
    /// Mid-stream failure: takes the actual usage accumulated by finish(), changes status to Failed,
    /// returns ModelUsage for the egress layer to record stats (failures after Started must still be recorded, see design §6.1)
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

/// Finds the earliest SSE frame boundary in the buffer, returning (boundary start index, separator byte count).
/// Recognizes both `\n\n` (2 bytes) and `\r\n\r\n` (4 bytes); takes the earliest occurrence,
/// preferring the longer CRLF separator at the same position to avoid leaving `\r` in the next frame.
fn find_frame_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = buf.windows(2).position(|w| w == b"\n\n");
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(l), Some(c)) => {
            if c <= l {
                Some((c, 4))
            } else {
                Some((l, 2))
            }
        }
        (Some(l), None) => Some((l, 2)),
        (None, Some(c)) => Some((c, 4)),
        (None, None) => None,
    }
}

/// Sends POST + validates status code synchronously, then spawns async read task, returns UnifiedStream
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

    // Extract upstream request ID (optional)
    let upstream_request_id = resp
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(super::upstream_error(status, &text));
    }

    let (tx, rx) = mpsc::channel::<Result<UnifiedStreamEvent, ConnError>>(64);

    // Send Started event first
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

            // Buffer size guard: prevents an abnormal upstream that continuously sends bytes without frame separators from growing buf unboundedly
            if buf.len() > MAX_FRAME_BUFFER {
                let call = translator.fail();
                let _ = tx
                    .send(Ok(UnifiedStreamEvent::Error {
                        message: format!("frame buffer exceeded {MAX_FRAME_BUFFER} bytes"),
                        call,
                    }))
                    .await;
                return;
            }

            // Byte-level frame splitting: recognizes both \n\n and \r\n\r\n SSE frame separators (the latter used when passing through a CRLF proxy)
            while let Some((pos, sep_len)) = find_frame_boundary(&buf) {
                let frame = buf[..pos].to_vec();
                buf.drain(..pos + sep_len);

                // UTF-8 safe decode of complete frame
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

        // Stream ended; translator.finish() is responsible for emitting Done{usage, call, ...}
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
        // Second frame triggers translator.push failure (after Started and some tokens already emitted)
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
        // Mid-stream failure must emit an Error event carrying actual usage with status=Failed (for stats recording)
        let call = err_call.expect("expected Error event with call");
        assert_eq!(call.status, CallStatus::Failed);
    }

    #[tokio::test]
    async fn parses_crlf_frame_separators() {
        let server = wiremock::MockServer::start().await;
        // Frame separator is \r\n\r\n after passing through a CRLF proxy
        let body = "data: {\"t\":\"你好\"}\r\n\r\ndata: {\"t\":\"世界\"}\r\n\r\ndata: [DONE]\r\n\r\n";
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
        let mut texts = String::new();
        while let Some(ev) = s.rx.recv().await {
            if let Ok(UnifiedStreamEvent::TextDelta { text }) = ev {
                texts.push_str(&text);
            }
        }
        assert_eq!(texts, "你好世界");
    }
}
