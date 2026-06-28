// admin/api.rs — Admin REST API endpoint route implementations
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use serde_json::Value;
use std::collections::HashMap;

use super::{err_response, require_admin, AdminState};
use crate::db::models::ModelRow;

// ─── models ────────────────────────────────────────────────────────────────

pub fn models_routes() -> Router<AdminState> {
    Router::new()
        .route("/admin/api/models", get(list_models).post(create_model))
        .route("/admin/api/models/:id", put(update_model).delete(delete_model))
}

async fn list_models(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match s.db.model_list().await {
        Ok(mut rows) => {
            // Mask encrypted API key plaintext (design §5.3)
            for m in &mut rows {
                m.api_key_enc = m.api_key_enc.as_ref().map(|_| "***".into());
            }
            Json(rows).into_response()
        }
        Err(e) => err_response(e),
    }
}

/// body: { id, connector, base_url, api_key?(plaintext), api_key_env?, model, anthropic_version?, extra? }
async fn create_model(
    State(s): State<AdminState>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match upsert_from_body(&s, &body, None).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}

async fn update_model(
    State(s): State<AdminState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match upsert_from_body(&s, &body, Some(&id)).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}

async fn upsert_from_body(
    s: &AdminState,
    body: &Value,
    id_override: Option<&str>,
) -> Result<(), crate::error::FusionError> {
    use crate::error::FusionError;

    let id = id_override
        .map(String::from)
        .or_else(|| body.get("id").and_then(|v| v.as_str()).map(String::from))
        .ok_or_else(|| FusionError::InvalidRequest("id required".into()))?;
    let connector = body
        .get("connector")
        .and_then(|v| v.as_str())
        .ok_or_else(|| FusionError::InvalidRequest("connector required".into()))?
        .to_string();
    let base_url = body
        .get("base_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| FusionError::InvalidRequest("base_url required".into()))?
        .to_string();
    validate_base_url(&base_url)?;
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| FusionError::InvalidRequest("model required".into()))?
        .to_string();
    // api_key plaintext → encrypted; if not provided during edit, retain the existing value
    let api_key_enc = match body.get("api_key").and_then(|v| v.as_str()) {
        Some(pt) if !pt.is_empty() => Some(crate::crypto::encrypt(&s.enc_key, pt)?),
        _ => s.db.model_get(&id).await?.and_then(|m| m.api_key_enc),
    };
    let row = ModelRow {
        id,
        connector,
        base_url,
        api_key_enc,
        api_key_env: body
            .get("api_key_env")
            .and_then(|v| v.as_str())
            .map(String::from),
        model,
        anthropic_version: body
            .get("anthropic_version")
            .and_then(|v| v.as_str())
            .map(String::from),
        extra: body.get("extra").map(|v| v.to_string()),
    };
    s.db.model_upsert(&row).await
}

/// Validate a model `base_url` before persisting it.
///
/// `base_url` drives server-side outbound requests (inference egress + connectivity
/// probe), so an unvalidated value is an SSRF vector. LocalFusion legitimately targets
/// local/LAN model servers (Ollama, llama.cpp, LM Studio), so loopback and private
/// ranges are intentionally allowed. What is rejected is the link-local range
/// `169.254.0.0/16` / `fe80::/10` — no real LLM upstream lives there, but the cloud
/// metadata endpoint `169.254.169.254` does, making it a pure attack target.
/// Set `LOCALFUSION_ALLOW_LINK_LOCAL=1` to override (e.g. for testing).
fn validate_base_url(base_url: &str) -> Result<(), crate::error::FusionError> {
    use crate::error::FusionError;
    use std::net::IpAddr;

    let url = reqwest::Url::parse(base_url)
        .map_err(|e| FusionError::InvalidRequest(format!("invalid base_url: {e}")))?;
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(FusionError::InvalidRequest(format!(
                "base_url scheme must be http or https, got '{other}'"
            )))
        }
    }
    if std::env::var("LOCALFUSION_ALLOW_LINK_LOCAL").as_deref() == Ok("1") {
        return Ok(());
    }
    // Reject link-local literals (the cloud metadata range) when the host is an IP.
    // url::host_str() returns IPv6 literals wrapped in brackets, so strip them first.
    if let Some(host) = url.host_str() {
        let bare = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
        if let Ok(ip) = bare.parse::<IpAddr>() {
            let link_local = match ip {
                IpAddr::V4(v4) => v4.is_link_local(),
                // IPv6 link-local fe80::/10; also catch IPv4-mapped link-local.
                IpAddr::V6(v6) => {
                    (v6.segments()[0] & 0xffc0) == 0xfe80
                        || v6.to_ipv4().is_some_and(|m| m.is_link_local())
                }
            };
            if link_local {
                return Err(FusionError::InvalidRequest(
                    "base_url points at a link-local address (169.254.0.0/16 / fe80::/10); \
                     this is blocked to prevent SSRF against cloud metadata endpoints"
                        .into(),
                ));
            }
        }
    }
    Ok(())
}

async fn delete_model(
    State(s): State<AdminState>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match s.db.model_references(&id).await {
        Ok(refs) if !refs.is_empty() => (
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "model in use", "references": refs})),
        )
            .into_response(),
        Ok(_) => match s.db.model_delete(&id).await {
            Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
            Err(e) => err_response(e),
        },
        Err(e) => err_response(e),
    }
}

// ─── model connectivity test ───────────────────────────────────────────────

pub fn models_test_routes() -> Router<AdminState> {
    Router::new()
        .route("/admin/api/models/test-all", post(test_all_models))
        .route("/admin/api/models/:id/test", post(test_one_model))
}

/// Truncate an error message to at most 60 chars (UTF-8 safe) for the response.
fn short_err(msg: &str) -> String {
    let mut chars = msg.chars();
    let s: String = chars.by_ref().take(60).collect();
    if chars.next().is_some() { format!("{s}…") } else { s }
}

/// Generate alternative base_url candidates to try when a probe fails.
/// The most common misconfiguration is a missing (or extra) `/v1` segment, since
/// the connector appends the provider path (e.g. `/chat/completions`) onto base_url.
/// Returns the original first, followed by the toggled-`/v1` variant.
fn base_url_candidates(base_url: &str) -> Vec<String> {
    let trimmed = base_url.trim_end_matches('/');
    let mut out = vec![trimmed.to_string()];

    // For http URLs, add the https upgrade as the next candidate so it is tried
    // before any /v1 variants (scheme mismatch is more common than missing /v1).
    let https_of = |s: &str| -> Option<String> {
        s.strip_prefix("http://").map(|rest| format!("https://{rest}"))
    };
    let https_base = https_of(trimmed);
    if let Some(ref h) = https_base {
        out.push(h.clone());
    }

    // Toggle the /v1 suffix — common misconfiguration for both schemes.
    let v1_toggle = if trimmed.ends_with("/v1") {
        trimmed.trim_end_matches("/v1").to_string()
    } else {
        format!("{trimmed}/v1")
    };
    if !v1_toggle.is_empty() && v1_toggle != trimmed {
        out.push(v1_toggle.clone());
    }
    // Also include the https version of the /v1-toggled URL (for http inputs).
    if let Some(h) = https_base {
        let h_v1 = if h.ends_with("/v1") {
            h.trim_end_matches("/v1").to_string()
        } else {
            format!("{h}/v1")
        };
        if !h_v1.is_empty() && h_v1 != h {
            out.push(h_v1);
        }
    }

    out
}

/// Max output tokens we optimistically try first. If the upstream rejects it with a
/// message stating the valid range, we parse the real limit from that message and use it.
const PROBE_MAX_TOKENS: u32 = 1_000_000;
/// Tiny max_tokens used for reachability probes, so an oversized value never makes a
/// reachable endpoint look unreachable.
const PING_MAX_TOKENS: u32 = 8;

/// Extract the real max output token limit from an upstream rejection message.
///
/// Providers reject an oversized `max_tokens` in two common shapes:
///   - stating the valid range, e.g. `the valid range of max_tokens is [1, 393216]`
///   - echoing the rejected value, e.g. `max_tokens 1000000 exceeds the maximum of 4096`
///
/// We take the largest plausible (>= 1024) integer in the message, but EXCLUDE `sent`
/// (the value we just probed with) — otherwise the echoed request value would win and the
/// confirmation re-probe with that same value would fail again, leaving detection silently
/// broken for every provider that echoes the request.
/// Returns None if no usable integer is present.
fn parse_max_tokens_limit(msg: &str, sent: u32) -> Option<u32> {
    let mut best: Option<u64> = None;
    let mut cur = String::new();
    // Walk the string, collecting maximal digit runs; track the largest value seen.
    for ch in msg.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(n) = cur.parse::<u64>() {
                if n != sent as u64 {
                    best = Some(best.map_or(n, |b| b.max(n)));
                }
            }
            cur.clear();
        }
    }
    best.filter(|&n| (1024..=u32::MAX as u64).contains(&n)).map(|n| n as u32)
}

/// Serialize a model's `extra` JSON with `default_max_tokens` set to the given value,
/// preserving any other keys already present.
fn extra_with_max_tokens(extra: Option<&str>, max_tokens: u32) -> String {
    let mut obj = extra
        .and_then(|e| serde_json::from_str::<serde_json::Value>(e).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    obj.insert("default_max_tokens".into(), serde_json::json!(max_tokens));
    serde_json::Value::Object(obj).to_string()
}

/// Decide whether a probe response actually looks like a model completion, rather than
/// an arbitrary 200 from a gateway/health page/different API that happens to answer the path.
///
/// The connector parsers are lenient (`unwrap_or` throughout), so a non-completion 200 still
/// yields an `Ok(UnifiedResponse)` — just an empty one. A genuine reply to the "ping" probe
/// has either some assistant text or a usage block, so we require at least one of those.
/// Without this, the candidate loop could short-circuit on a wrong (connector, base_url) pair
/// and persist it, silently corrupting how all future traffic to the model is translated.
fn response_looks_like_completion(resp: &crate::unified::UnifiedResponse) -> bool {
    let has_text = resp.items.iter().any(|item| {
        if let crate::unified::Item::Message { content, .. } = item {
            content.iter().any(|c| matches!(c, crate::unified::ContentBlock::Text(t) if !t.is_empty()))
        } else {
            false
        }
    });
    let has_usage = resp.usage.input_tokens > 0
        || resp.usage.output_tokens > 0
        // estimated == false means the upstream returned a real usage block.
        || resp.calls.iter().any(|c| !c.estimated);
    has_text || has_usage
}

/// Run one probe request against (kind, base_url, model_name, max_tokens). Returns Ok(latency_ms) or Err(message).
async fn probe_attempt(
    kind: crate::connector::ConnectorKind,
    base_url: &str,
    model_name: &str,
    key: &Option<String>,
    anthropic_version: Option<&str>,
    max_tokens: u32,
    http: &reqwest::Client,
) -> Result<u64, String> {
    use crate::connector::{make_connector, EgressCtx};
    let egress = EgressCtx {
        base_url: base_url.to_string(),
        model: model_name.to_string(),
        auth: kind.auth_kind(),
        key: key.clone(),
        anthropic_version: anthropic_version.map(String::from),
        default_max_tokens: None,
        http: http.clone(),
    };
    // The probe request carries max_tokens explicitly (req.max_tokens takes precedence
    // over ctx.default_max_tokens in every connector), so this is what is actually sent.
    let req = crate::probe::probe_request_with(max_tokens);
    let connector = make_connector(kind);
    let start = std::time::Instant::now();
    let resp = connector
        .complete(&req, &egress)
        .await
        .map_err(|e| e.to_string())?;
    // A 200 that doesn't parse into a recognizable completion means this connector is the
    // wrong format for the endpoint — reject it so the candidate loop keeps searching.
    if !response_looks_like_completion(&resp) {
        return Err("upstream 200 but response is not a recognizable completion".into());
    }
    Ok(start.elapsed().as_millis() as u64)
}

/// Probe a single model row. Steps:
///  1. Find a working (connector, base_url) — trying the configured combo first, then variants.
///  2. Probe the accepted max output tokens: try 1M, and if rejected, parse the real upper
///     bound from the error message and confirm it. The accepted value is stored.
///
/// Any auto-correction (connector/base_url/default_max_tokens) is persisted via model_upsert.
/// Returns { id, ok, latency_ms?, error?, base_url_fixed?, connector_fixed?, max_tokens? }.
async fn probe_one(
    m: &crate::db::models::ModelRow,
    db: &crate::db::Db,
    enc_key: &[u8; 32],
    http: &reqwest::Client,
) -> serde_json::Value {
    use crate::connector::{resolve_key, ConnectorKind};
    use std::str::FromStr;

    let key = match resolve_key(m, enc_key) {
        Ok(k) => k,
        Err(e) => return serde_json::json!({ "id": m.id, "ok": false, "error": short_err(&e.to_string()) }),
    };
    if key.is_none() {
        return serde_json::json!({ "id": m.id, "ok": false, "error": "key unavailable" });
    }

    let configured_kind = match ConnectorKind::from_str(&m.connector) {
        Ok(k) => k,
        Err(e) => return serde_json::json!({ "id": m.id, "ok": false, "error": short_err(&e.to_string()) }),
    };
    let av = m.anthropic_version.as_deref();

    // ── Step 1: find a working (connector, base_url) ─────────────────────────
    // Probe order: configured kind × base_url variants first, then other kinds × variants.
    let mut candidates: Vec<(ConnectorKind, String)> = Vec::new();
    for base in base_url_candidates(&m.base_url) {
        candidates.push((configured_kind, base));
    }
    for kind in ConnectorKind::all() {
        if kind == configured_kind { continue; }
        for base in base_url_candidates(&m.base_url) {
            candidates.push((kind, base));
        }
    }

    let mut working: Option<(ConnectorKind, String, u64)> = None;
    let mut last_err = String::from("connection test failed");
    // Reachability probes use a tiny max_tokens so an oversized value never masks a reachable endpoint.
    for (kind, base_url) in &candidates {
        match probe_attempt(*kind, base_url, &m.model, &key, av, PING_MAX_TOKENS, http).await {
            Ok(ms) => { working = Some((*kind, base_url.clone(), ms)); break; }
            Err(e) => last_err = e,
        }
    }
    let (work_kind, work_base, latency_ms) = match working {
        Some(w) => w,
        None => return serde_json::json!({ "id": m.id, "ok": false, "error": short_err(&last_err) }),
    };

    // ── Step 2: probe the accepted max output tokens ─────────────────────────
    // Try a large value first. If rejected, parse the real upper bound from the
    // error message (e.g. "valid range of max_tokens is [1, 393216]") and re-probe
    // with it to confirm. If nothing is accepted, leave the existing default untouched.
    let mut max_tokens: Option<u32> = None;
    match probe_attempt(work_kind, &work_base, &m.model, &key, av, PROBE_MAX_TOKENS, http).await {
        Ok(_) => max_tokens = Some(PROBE_MAX_TOKENS),
        Err(e) => {
            if let Some(limit) = parse_max_tokens_limit(&e, PROBE_MAX_TOKENS) {
                if probe_attempt(work_kind, &work_base, &m.model, &key, av, limit, http).await.is_ok() {
                    max_tokens = Some(limit);
                }
            }
        }
    }

    // ── Persist any change ───────────────────────────────────────────────────
    let kind_changed = work_kind != configured_kind;
    let base_changed = work_base != m.base_url.trim_end_matches('/');
    let prev_max = m.extra.as_deref()
        .and_then(|e| serde_json::from_str::<serde_json::Value>(e).ok())
        .and_then(|v| v.get("default_max_tokens").and_then(|x| x.as_u64()))
        .map(|x| x as u32);
    let max_changed = max_tokens.is_some() && max_tokens != prev_max;

    if kind_changed || base_changed || max_changed {
        let mut row = m.clone();
        row.connector = work_kind.as_str().to_string();
        row.base_url = work_base.clone();
        if let Some(mt) = max_tokens {
            row.extra = Some(extra_with_max_tokens(m.extra.as_deref(), mt));
        }
        if let Err(e) = db.model_upsert(&row).await {
            return serde_json::json!({
                "id": m.id, "ok": false,
                "error": short_err(&format!("probe ok but DB update failed: {e}"))
            });
        }
    }

    let mut out = serde_json::json!({ "id": m.id, "ok": true, "latency_ms": latency_ms });
    if let Some(mt) = max_tokens { out["max_tokens"] = serde_json::json!(mt); }
    if base_changed { out["base_url_fixed"] = serde_json::json!(work_base); }
    if kind_changed { out["connector_fixed"] = serde_json::json!(work_kind.as_str()); }
    out
}

async fn test_one_model(
    State(s): State<AdminState>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    let m = match s.db.model_get(&id).await {
        Ok(Some(m)) => m,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))).into_response(),
        Err(e) => return err_response(e),
    };
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default();
    let result = probe_one(&m, &s.db, &s.enc_key, &http).await;
    Json(result).into_response()
}

async fn test_all_models(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    let models = match s.db.model_list().await {
        Ok(m) => m,
        Err(e) => return err_response(e),
    };

    let enc_key = s.enc_key;
    // Probing uses a no-redirect client so that http→https redirects surface as
    // failures, causing the https candidate to be tried and stored instead.
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default();

    let tasks: Vec<_> = models
        .into_iter()
        .map(|m| {
            let db = s.db.clone();
            let http = http.clone();
            async move { probe_one(&m, &db, &enc_key, &http).await }
        })
        .collect();

    let results = futures::future::join_all(tasks).await;
    Json(results).into_response()
}

// ─── virtual-models + strategies ───────────────────────────────────────────

pub fn vmodels_routes() -> Router<AdminState> {
    Router::new()
        .route(
            "/admin/api/virtual-models",
            get(list_vmodels).post(create_vmodel),
        )
        .route(
            "/admin/api/virtual-models/:name",
            put(update_vmodel).delete(delete_vmodel),
        )
        .route("/admin/api/strategies", get(list_strategies))
}

async fn list_vmodels(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match s.db.vmodel_list().await {
        Ok(rows) => {
            let mut out = Vec::new();
            for vm in rows {
                let members = s.db.vmodel_members(&vm.name).await.unwrap_or_default();
                let params: Value =
                    serde_json::from_str(&vm.params).unwrap_or(Value::Null);
                out.push(serde_json::json!({
                    "name": vm.name,
                    "strategy": vm.strategy,
                    "params": params,
                    "members": members,
                }));
            }
            Json(out).into_response()
        }
        Err(e) => err_response(e),
    }
}

/// body: { name, strategy, params(object), members:[id...] }
async fn upsert_vmodel(
    s: &AdminState,
    body: &Value,
    name_override: Option<&str>,
) -> Result<(), crate::error::FusionError> {
    use crate::db::virtual_models::VirtualModelRow;
    use crate::error::FusionError;

    let name = name_override
        .map(String::from)
        .or_else(|| body.get("name").and_then(|v| v.as_str()).map(String::from))
        .ok_or_else(|| FusionError::InvalidRequest("name required".into()))?;
    let strategy = body
        .get("strategy")
        .and_then(|v| v.as_str())
        .ok_or_else(|| FusionError::InvalidRequest("strategy required".into()))?
        .to_string();
    if crate::strategy::make_strategy(&strategy).is_none() {
        return Err(FusionError::InvalidRequest(format!(
            "unknown strategy '{strategy}'"
        )));
    }
    let params = body
        .get("params")
        .cloned()
        .unwrap_or(serde_json::json!({}))
        .to_string();
    let members: Vec<String> = body
        .get("members")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Validate that member models must exist (design §6.3 fail-fast)
    for id in members.iter() {
        if s.db.model_get(id).await?.is_none() {
            return Err(FusionError::InvalidRequest(format!(
                "member model '{id}' not found"
            )));
        }
    }
    // Validate that routed models referenced in params exist
    let params_val: Value = serde_json::from_str(&params).unwrap_or(Value::Null);
    for key in ["judge", "web_search", "image_generation", "tool_search", "image_query"] {
        if let Some(mid) = params_val.get(key).and_then(|v| v.as_str()) {
            if s.db.model_get(mid).await?.is_none() {
                return Err(FusionError::InvalidRequest(format!(
                    "{key} model '{mid}' not found"
                )));
            }
        }
    }
    s.db.vmodel_upsert(&VirtualModelRow { name, strategy, params }, &members)
        .await
}

async fn create_vmodel(
    State(s): State<AdminState>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match upsert_vmodel(&s, &body, None).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}

async fn update_vmodel(
    State(s): State<AdminState>,
    h: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match upsert_vmodel(&s, &body, Some(&name)).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}

async fn delete_vmodel(
    State(s): State<AdminState>,
    h: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match s.db.vmodel_delete(&name).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}

async fn list_strategies(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    let names = [
        "failover",
        "speed",
        "cheapest",
        "synthesize",
        "best-of-n",
        "multimodal",
    ];
    let out: Vec<Value> = names
        .iter()
        .map(|n| {
            serde_json::json!({
                "name": n,
                "params_schema": crate::strategy::params_schema(n),
            })
        })
        .collect();
    Json(out).into_response()
}

// ─── keys ───────────────────────────────────────────────────────────────────

pub fn keys_routes() -> Router<AdminState> {
    Router::new()
        .route("/admin/api/keys", get(list_keys).post(create_key))
        .route("/admin/api/keys/:id", patch(patch_key).delete(delete_key))
        .route("/admin/api/keys/:id/acl", put(set_acl))
}

async fn list_keys(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match s.db.key_list().await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => err_response(e),
    }
}

/// body: { label? }; returns plaintext once (design §5.3)
async fn create_key(
    State(s): State<AdminState>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    let label = body.get("label").and_then(|v| v.as_str());
    let plaintext = random_key();
    let now = now_secs();
    match s.db.key_insert(&plaintext, label, now).await {
        Ok(id) => Json(serde_json::json!({"id": id, "key": plaintext})).into_response(),
        Err(e) => err_response(e),
    }
}

async fn patch_key(
    State(s): State<AdminState>,
    h: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    let enabled = body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    let label = body.get("label").and_then(|v| v.as_str());
    match s.db.key_set_enabled_label(id, enabled, label).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}

async fn delete_key(
    State(s): State<AdminState>,
    h: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match s.db.key_delete(id).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}

/// body: { acl_all: bool, names: [String] }
async fn set_acl(
    State(s): State<AdminState>,
    h: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    let acl_all = body.get("acl_all").and_then(|v| v.as_bool()).unwrap_or(false);
    let names: Vec<String> = body
        .get("names")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    match s.db.key_set_acl(id, acl_all, &names).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => err_response(e),
    }
}

// Current Unix seconds
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ingress key: generate 24 bytes of random entropy using CSPRNG (unpredictable)
fn random_key() -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "sk-lf-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

// ─── stats ──────────────────────────────────────────────────────────────────

pub fn stats_routes() -> Router<AdminState> {
    Router::new()
        .route("/admin/api/stats/usage", get(stats_usage))
        .route("/admin/api/stats/usage/summary", get(stats_usage_summary))
        .route("/admin/api/stats/prices", get(stats_prices))
        .route("/admin/api/stats/latency", get(stats_latency))
        .route("/admin/api/stats/requests", get(stats_requests))
}

async fn stats_usage(
    State(s): State<AdminState>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    let scope = q.get("scope").map(String::as_str).unwrap_or("total");
    let name = q.get("name").map(String::as_str);
    let from: i64 = q.get("from").and_then(|v| v.parse().ok()).unwrap_or(0);
    let to: i64 = q
        .get("to")
        .and_then(|v| v.parse().ok())
        .unwrap_or(i64::MAX);
    match s.db.usage_query(scope, name, from, to).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => err_response(e),
    }
}

async fn stats_usage_summary(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match s.db.usage_query("total", None, 0, i64::MAX).await {
        Ok(rows) => {
            let req: i64 = rows.iter().map(|r| r.requests).sum();
            let inp: i64 = rows.iter().map(|r| r.input_tokens).sum();
            let out: i64 = rows.iter().map(|r| r.output_tokens).sum();
            let cost: f64 = rows.iter().map(|r| r.cost).sum();
            Json(serde_json::json!({
                "requests": req,
                "input_tokens": inp,
                "output_tokens": out,
                "total_tokens": inp + out,
                "cost": cost,
            }))
            .into_response()
        }
        Err(e) => err_response(e),
    }
}

async fn stats_prices(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    match s.db.price_list().await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => err_response(e),
    }
}

async fn stats_latency(
    State(s): State<AdminState>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    // model optional: if specified returns single model, otherwise returns array of all models
    let models: Vec<String> = match q.get("model").filter(|m| !m.is_empty()) {
        Some(m) => vec![m.clone()],
        None => match s.db.model_list().await {
            Ok(rows) => rows.into_iter().map(|m| m.id).collect(),
            Err(e) => return err_response(e),
        },
    };
    let mut out = Vec::with_capacity(models.len());
    for model_id in models {
        let avg = match s.db.latency_avg_recent(&model_id, 10).await {
            Ok(v) => v,
            Err(e) => return err_response(e),
        };
        let count = match s.db.latency_sample_count(&model_id, 10).await {
            Ok(v) => v,
            Err(e) => return err_response(e),
        };
        out.push(serde_json::json!({
            "model_id": model_id,
            "avg_throughput": avg.unwrap_or(0.0),
            "sample_count": count,
        }));
    }
    Json(out).into_response()
}

async fn stats_requests(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    // last 200 rows from request_log (v1 full scan, pagination to be added later)
    match sqlx::query_as::<
        _,
        (
            i64,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<f64>,
            i64,
        ),
    >(
        "SELECT id, virtual_name, strategy, status, total_tokens, cost, created_at \
         FROM request_log ORDER BY id DESC LIMIT 200",
    )
    .fetch_all(&s.db.pool)
    .await
    {
        Ok(rows) => {
            let out: Vec<Value> = rows
                .into_iter()
                .map(|(id, vn, st, status, tok, cost, ts)| {
                    serde_json::json!({
                        "id": id,
                        "virtual_name": vn,
                        "strategy": st,
                        "status": status,
                        "total_tokens": tok,
                        "cost": cost,
                        "created_at": ts,
                    })
                })
                .collect();
            Json(out).into_response()
        }
        Err(e) => err_response(e.into()),
    }
}

// ─── playground ───────────────────────────────────────────────────────────────

fn err_json(code: StatusCode, msg: &str) -> Response {
    (code, Json(serde_json::json!({"error": msg}))).into_response()
}

pub fn playground_routes() -> Router<AdminState> {
    Router::new().route("/admin/api/playground", post(playground_handler))
}

/// body: { virtual_name, prompt? } — simplified debug call (no full trace written to DB)
async fn playground_handler(
    State(s): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if let Err(r) = require_admin(&s, &headers).await {
        return r;
    }
    let virtual_name = match body.get("virtual_name").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => return err_json(StatusCode::BAD_REQUEST, "virtual_name required"),
    };
    let prompt = body
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let req = crate::unified::UnifiedRequest {
        items: vec![crate::unified::Item::Message {
            role: crate::unified::Role::User,
            content: vec![crate::unified::ContentBlock::Text(prompt)],
        }],
        tools: vec![],
        max_tokens: Some(1024),
        temperature: None,
        stream: false,
        raw_extra: serde_json::Value::Null,
    };
    let strategy_name = match s.db.vmodel_get(&virtual_name).await {
        Ok(Some(vm)) => vm.strategy,
        Ok(None) => return err_json(StatusCode::NOT_FOUND, "unknown virtual model"),
        Err(e) => return err_response(e),
    };
    let router = crate::router::Router::new(s.db.clone(), s.enc_key);
    let recorder = crate::unified::CallRecorder::default();
    let trace = crate::unified::StrategyTrace::default();
    match router
        .dispatch(&virtual_name, req, false, &recorder, Some(&trace))
        .await
    {
        Ok(crate::strategy::StrategyOutput::Full(resp)) => {
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
            let calls = recorder.drain();
            Json(serde_json::json!({
                "final": text,
                "strategy": strategy_name,
                "calls": calls,
                "detail": trace.snapshot(),
            }))
            .into_response()
        }
        Ok(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "unexpected stream"),
        Err(e) => {
            let calls = recorder.drain();
            Json(serde_json::json!({
                "error": e.to_string(),
                "strategy": strategy_name,
                "calls": calls,
                "detail": trace.snapshot(),
            }))
            .into_response()
        }
    }
}

// ─── logging settings ───────────────────────────────────────────────────────
pub fn logging_routes() -> Router<AdminState> {
    Router::new().route(
        "/admin/api/settings/logging",
        get(get_logging).put(put_logging),
    )
}

async fn get_logging(State(s): State<AdminState>, h: HeaderMap) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    let level = s
        .db
        .setting_get_or("log_level", "info")
        .await
        .unwrap_or_else(|_| "info".into());
    let file = s
        .db
        .setting_get_or("log_file", "")
        .await
        .unwrap_or_default();
    let stdout = s
        .db
        .setting_get_or("log_to_stdout", "true")
        .await
        .unwrap_or_else(|_| "true".into());
    Json(serde_json::json!({
        "log_level": level,
        "log_file": file,
        "log_to_stdout": stdout == "true",
    }))
    .into_response()
}

/// body: { log_level?, log_file?, log_to_stdout? }
/// log_level hot-reloaded; log_file / log_to_stdout written to DB, takes effect after restart
async fn put_logging(
    State(s): State<AdminState>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    if let Some(level) = body.get("log_level").and_then(|v| v.as_str()) {
        if let Err(e) = s.log.set_level(level) {
            return err_response(e);
        }
        if let Err(e) = s.db.setting_set("log_level", level).await {
            return err_response(e);
        }
    }
    if let Some(f) = body.get("log_file").and_then(|v| v.as_str()) {
        if let Err(e) = s.db.setting_set("log_file", f).await {
            return err_response(e);
        }
    }
    if let Some(b) = body.get("log_to_stdout").and_then(|v| v.as_bool()) {
        let val = if b { "true" } else { "false" };
        if let Err(e) = s.db.setting_set("log_to_stdout", val).await {
            return err_response(e);
        }
    }
    Json(serde_json::json!({"ok": true, "note": "log_file/log_to_stdout requires restart to take effect"}))
        .into_response()
}


#[cfg(test)]
mod tests {
    use super::{
        base_url_candidates, extra_with_max_tokens, parse_max_tokens_limit,
        response_looks_like_completion, validate_base_url,
    };
    use serde_json::Value;

    #[test]
    fn candidate_adds_v1_when_missing() {
        assert_eq!(base_url_candidates("https://x.com"), vec!["https://x.com", "https://x.com/v1"]);
        assert_eq!(base_url_candidates("https://x.com/"), vec!["https://x.com", "https://x.com/v1"]);
    }

    #[test]
    fn candidate_removes_v1_when_present() {
        assert_eq!(base_url_candidates("https://x.com/v1"), vec!["https://x.com/v1", "https://x.com"]);
        assert_eq!(base_url_candidates("https://x.com/v1/"), vec!["https://x.com/v1", "https://x.com"]);
    }

    #[test]
    fn candidate_http_adds_https_variants() {
        assert_eq!(
            base_url_candidates("http://x.com"),
            vec!["http://x.com", "https://x.com", "http://x.com/v1", "https://x.com/v1"],
        );
        assert_eq!(
            base_url_candidates("http://x.com/v1/"),
            vec!["http://x.com/v1", "https://x.com/v1", "http://x.com", "https://x.com"],
        );
    }

    #[test]
    fn extra_sets_max_tokens_on_empty() {
        let out = extra_with_max_tokens(None, 200_000);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["default_max_tokens"], 200_000);
    }

    #[test]
    fn extra_preserves_other_keys_and_overwrites_max_tokens() {
        let prev = r#"{"timeout":30,"default_max_tokens":8}"#;
        let out = extra_with_max_tokens(Some(prev), 1_000_000);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["timeout"], 30);
        assert_eq!(v["default_max_tokens"], 1_000_000);
    }

    #[test]
    fn extra_handles_malformed_input() {
        // Non-JSON or non-object input is replaced with a fresh object.
        let out = extra_with_max_tokens(Some("not json"), 200_000);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["default_max_tokens"], 200_000);
    }

    #[test]
    fn parse_limit_picks_largest_integer() {
        // DeepSeek-style rejection: "[1, 393216]" → 393216 is the real output cap.
        assert_eq!(
            parse_max_tokens_limit("the valid range of max_tokens is [1, 393216]", 1_000_000),
            Some(393216)
        );
    }

    #[test]
    fn parse_limit_ignores_small_or_absent() {
        // No plausible (>=1024) integer present.
        assert_eq!(parse_max_tokens_limit("model not found", 1_000_000), None);
        assert_eq!(parse_max_tokens_limit("range is [1, 8]", 1_000_000), None);
    }

    #[test]
    fn parse_limit_excludes_echoed_sent_value() {
        // Provider echoes the rejected request value: "max_tokens 1000000 exceeds the maximum of 4096".
        // The largest integer is the echoed 1000000 — excluding `sent` lets the real cap (4096) win.
        assert_eq!(
            parse_max_tokens_limit("max_tokens 1000000 exceeds the maximum of 4096", 1_000_000),
            Some(4096)
        );
        // "you requested 1000000, limit is 128000"
        assert_eq!(
            parse_max_tokens_limit("you requested 1000000, limit is 128000", 1_000_000),
            Some(128000)
        );
        // If the ONLY integer present is the echoed sent value, there is nothing usable to parse.
        assert_eq!(
            parse_max_tokens_limit("max_tokens 1000000 is too large", 1_000_000),
            None
        );
    }

    #[test]
    fn validate_base_url_accepts_http_https_and_local() {
        assert!(validate_base_url("https://api.openai.com/v1").is_ok());
        assert!(validate_base_url("http://api.openai.com").is_ok());
        // Local/LAN model servers are a primary use case — must be allowed.
        assert!(validate_base_url("http://127.0.0.1:11434").is_ok());
        assert!(validate_base_url("http://localhost:8080/v1").is_ok());
        assert!(validate_base_url("http://192.168.1.50:1234").is_ok());
    }

    #[test]
    fn validate_base_url_rejects_bad_scheme_and_garbage() {
        assert!(validate_base_url("ftp://x.com").is_err());
        assert!(validate_base_url("file:///etc/passwd").is_err());
        assert!(validate_base_url("not a url").is_err());
    }

    #[test]
    fn validate_base_url_rejects_link_local_metadata() {
        // The cloud metadata endpoint and its range.
        assert!(validate_base_url("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(validate_base_url("http://169.254.0.1").is_err());
        // IPv6 link-local.
        assert!(validate_base_url("http://[fe80::1]").is_err());
    }

    #[test]
    fn completion_check_accepts_text_or_usage() {
        use crate::unified::*;
        let with_text = UnifiedResponse {
            items: vec![Item::Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text("pong".into())],
            }],
            usage: Usage { input_tokens: 0, output_tokens: 0 },
            model_id: "m".into(),
            calls: vec![],
        };
        assert!(response_looks_like_completion(&with_text));

        let with_usage = UnifiedResponse {
            items: vec![Item::Message { role: Role::Assistant, content: vec![] }],
            usage: Usage { input_tokens: 3, output_tokens: 1 },
            model_id: "m".into(),
            calls: vec![],
        };
        assert!(response_looks_like_completion(&with_usage));
    }

    #[test]
    fn completion_check_rejects_empty_200() {
        use crate::unified::*;
        // A gateway/health 200 parses into an empty message with no usage — must be rejected
        // so the wrong connector is never persisted.
        let empty = UnifiedResponse {
            items: vec![Item::Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text(String::new())],
            }],
            usage: Usage { input_tokens: 0, output_tokens: 0 },
            model_id: "m".into(),
            calls: vec![ModelUsage {
                model_id: "m".into(),
                role: CallRole::Member,
                input_tokens: 0,
                output_tokens: 0,
                cost: 0.0,
                status: CallStatus::Ok,
                estimated: true,
                latency_secs: 0.0,
            }],
        };
        assert!(!response_looks_like_completion(&empty));
    }
}
