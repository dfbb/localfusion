use std::sync::Arc;

use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response, Sse};
use axum::Json;
use serde_json::Value;

use crate::db::Db;
use crate::error::FusionError;
use crate::ingress::{anthropic, openai_chat, openai_responses};
use crate::pipeline::{finalize_full, write_stats};
use crate::router::Router as FusionRouter;
use crate::strategy::StrategyOutput;
use crate::unified::{CallRecorder, ModelUsage, UnifiedStreamEvent};

#[derive(Clone)]
pub struct InferenceState {
    pub db: Db,
    pub enc_key: [u8; 32],
}

#[derive(Clone, Copy)]
pub enum Proto {
    Chat,
    Responses,
    Anthropic,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn parse(proto: Proto, body: &Value) -> Result<crate::unified::UnifiedRequest, FusionError> {
    match proto {
        Proto::Chat => openai_chat::parse_request(body),
        Proto::Responses => openai_responses::parse_request(body),
        Proto::Anthropic => anthropic::parse_request(body),
    }
}
fn format(proto: Proto, resp: &crate::unified::UnifiedResponse) -> Value {
    match proto {
        Proto::Chat => openai_chat::format_response(resp),
        Proto::Responses => openai_responses::format_response(resp),
        Proto::Anthropic => anthropic::format_response(resp),
    }
}
fn sse_lines(proto: Proto, ev: &UnifiedStreamEvent) -> Vec<String> {
    match proto {
        Proto::Chat => openai_chat::sse_events(ev),
        Proto::Responses => openai_responses::sse_events(ev),
        Proto::Anthropic => anthropic::sse_events(ev),
    }
}

pub async fn handle(state: InferenceState, proto: Proto, headers: HeaderMap, body: Value) -> Response {
    let model = match body.get("model").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => return err(FusionError::InvalidRequest("model required".into())),
    };
    if let Err(e) = crate::auth::authorize_ingress(&state.db, &headers, &model).await {
        let code = if matches!(e, FusionError::Unauthorized(_)) {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::FORBIDDEN
        };
        return (code, Json(serde_json::json!({"error": e.to_string()}))).into_response();
    }
    let req = match parse(proto, &body) {
        Ok(r) => r,
        Err(e) => return err(e),
    };
    let want_stream = req.stream;

    // 取策略名（写 request_log 用）
    let strategy = state
        .db
        .vmodel_get(&model)
        .await
        .ok()
        .flatten()
        .map(|v| v.strategy)
        .unwrap_or_default();
    let router = FusionRouter::new(state.db.clone(), state.enc_key);
    let recorder = Arc::new(CallRecorder::default());

    match router.dispatch(&model, req, want_stream, &recorder, None).await {
        Ok(StrategyOutput::Full(resp)) => {
            let _ = finalize_full(&state.db, &model, &strategy, &recorder, false, now_secs()).await;
            if want_stream {
                stream_from_full(proto, resp)
            } else {
                Json(format(proto, &resp)).into_response()
            }
        }
        Ok(StrategyOutput::Stream(stream)) => {
            stream_real(state.db.clone(), model, strategy, recorder, proto, stream)
        }
        Err(e) => {
            // 错误路径也写统计（drain 已发生调用）
            let calls = recorder.drain();
            let _ = write_stats(&state.db, &model, &strategy, &calls, true, now_secs()).await;
            err(e)
        }
    }
}

fn err(e: FusionError) -> Response {
    let code = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (code, Json(serde_json::json!({"error": e.to_string()}))).into_response()
}

/// panel/multimodal 的 Full → 伪流 SSE（统计已在 finalize_full 落库）。
fn stream_from_full(proto: Proto, resp: crate::unified::UnifiedResponse) -> Response {
    use futures::stream;
    let text = resp
        .items
        .iter()
        .find_map(|i| match i {
            crate::unified::Item::Message { content, .. } => Some(
                content
                    .iter()
                    .filter_map(|c| match c {
                        crate::unified::ContentBlock::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let mut evs: Vec<UnifiedStreamEvent> = vec![UnifiedStreamEvent::Started {
        model_id: resp.model_id.clone(),
    }];
    evs.push(UnifiedStreamEvent::TextDelta { text });
    evs.push(UnifiedStreamEvent::Done {
        usage: resp.usage,
        call: None,
        finish_reason: Some("stop".into()),
    });
    let mut lines: Vec<String> = Vec::new();
    for ev in &evs {
        for l in sse_lines(proto, ev) {
            lines.push(l);
        }
    }
    let s = stream::iter(
        lines
            .into_iter()
            .map(|l| Ok::<_, std::convert::Infallible>(Event::default().data(l))),
    );
    Sse::new(s).into_response()
}

/// 单模型真流：边转发边收集尾用量，流关闭后 write_stats。
fn stream_real(
    db: Db,
    model: String,
    strategy: String,
    recorder: Arc<CallRecorder>,
    proto: Proto,
    mut stream: crate::unified::UnifiedStream,
) -> Response {
    use async_stream::stream as astream;
    let body = astream! {
        let mut tail: Option<ModelUsage> = None;
        let mut failed = false;
        while let Some(item) = stream.rx.recv().await {
            match item {
                Ok(ev) => {
                    if let UnifiedStreamEvent::Done { call, .. } = &ev { tail = call.clone(); }
                    if let UnifiedStreamEvent::Error { call, .. } = &ev { tail = call.clone(); failed = true; }
                    for l in sse_lines(proto, &ev) {
                        yield Ok::<_, std::convert::Infallible>(Event::default().data(l));
                    }
                }
                Err(e) => { failed = true; yield Ok(Event::default().data(format!("{{\"error\":\"{e}\"}}"))); break; }
            }
        }
        // 合并 recorder 暂存的失败尝试 + 尾用量，写统计
        let mut all = recorder.drain();
        if let Some(t) = tail { all.push(t); }
        let _ = write_stats(&db, &model, &strategy, &all, failed, now_secs()).await;
    };
    Sse::new(body).into_response()
}
