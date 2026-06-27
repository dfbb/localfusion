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

fn format_err_body(proto: Proto, message: &str) -> Value {
    match proto {
        Proto::Chat => openai_chat::format_error(message),
        Proto::Responses => openai_responses::format_error(message),
        Proto::Anthropic => anthropic::format_error(message),
    }
}

pub async fn handle(state: InferenceState, proto: Proto, headers: HeaderMap, body: Value) -> Response {
    let model = match body.get("model").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => return err(proto, FusionError::InvalidRequest("model required".into())),
    };
    if let Err(e) = crate::auth::authorize_ingress(&state.db, &headers, &model).await {
        let code = if matches!(e, FusionError::Unauthorized(_)) {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::FORBIDDEN
        };
        return (code, Json(format_err_body(proto, &e.to_string()))).into_response();
    }
    let req = match parse(proto, &body) {
        Ok(r) => r,
        Err(e) => return err(proto, e),
    };
    let want_stream = req.stream;

    // Get strategy name (used for writing request_log)
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
            // Error path also writes stats (drain has already occurred for calls)
            let calls = recorder.drain();
            let _ = write_stats(&state.db, &model, &strategy, &calls, true, now_secs()).await;
            err(proto, e)
        }
    }
}

fn err(proto: Proto, e: FusionError) -> Response {
    let code = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (code, Json(format_err_body(proto, &e.to_string()))).into_response()
}

/// Full → pseudo-streaming SSE for panel/multimodal (stats already persisted in finalize_full).
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

/// Single-model real streaming: forward while collecting tail usage, write_stats after stream closes.
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
                Err(e) => {
                    failed = true;
                    let ev = UnifiedStreamEvent::Error { message: e.to_string(), call: None };
                    for l in sse_lines(proto, &ev) {
                        yield Ok::<_, std::convert::Infallible>(Event::default().data(l));
                    }
                    break;
                }
            }
        }
        // Merge recorder's buffered failed attempts + tail usage, write stats
        let mut all = recorder.drain();
        if let Some(t) = tail { all.push(t); }
        let _ = write_stats(&db, &model, &strategy, &all, failed, now_secs()).await;
    };
    Sse::new(body).into_response()
}
