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
    Router::new().route("/admin/api/models/test-all", post(test_all_models))
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
    let alt = if trimmed.ends_with("/v1") {
        trimmed.trim_end_matches("/v1").to_string()
    } else {
        format!("{trimmed}/v1")
    };
    if !alt.is_empty() && alt != trimmed {
        out.push(alt);
    }
    out
}

async fn test_all_models(State(s): State<AdminState>, h: HeaderMap) -> Response {
    use crate::connector::{make_connector, resolve_key, ConnectorKind, EgressCtx};
    use std::str::FromStr;

    if let Err(r) = require_admin(&s, &h).await {
        return r;
    }
    let models = match s.db.model_list().await {
        Ok(m) => m,
        Err(e) => return err_response(e),
    };

    let enc_key = s.enc_key;
    let http = reqwest::Client::new();

    let tasks: Vec<_> = models
        .into_iter()
        .map(|m| {
            let db = s.db.clone();
            let http = http.clone();
            async move {
                // Resolve the API key once (shared across all probe attempts)
                let key = match resolve_key(&m, &enc_key) {
                    Ok(k) => k,
                    Err(e) => {
                        return serde_json::json!({ "id": m.id, "ok": false, "error": short_err(&e.to_string()) });
                    }
                };
                if key.is_none() {
                    return serde_json::json!({ "id": m.id, "ok": false, "error": "key unavailable" });
                }

                let default_max_tokens = m.extra.as_deref()
                    .and_then(|e| serde_json::from_str::<serde_json::Value>(e).ok())
                    .and_then(|v| v.get("default_max_tokens").and_then(|x| x.as_u64()))
                    .map(|x| x as u32);

                let configured_kind = match ConnectorKind::from_str(&m.connector) {
                    Ok(k) => k,
                    Err(e) => {
                        return serde_json::json!({ "id": m.id, "ok": false, "error": short_err(&e.to_string()) });
                    }
                };

                let req = crate::probe::probe_request();

                // Build the probe order: configured (kind, base_url) variants first,
                // then every other connector kind with its base_url variants.
                // Each entry is (ConnectorKind, base_url).
                let mut candidates: Vec<(ConnectorKind, String)> = Vec::new();
                for base in base_url_candidates(&m.base_url) {
                    candidates.push((configured_kind, base));
                }
                for kind in ConnectorKind::all() {
                    if kind == configured_kind {
                        continue;
                    }
                    for base in base_url_candidates(&m.base_url) {
                        candidates.push((kind, base));
                    }
                }

                let mut last_err = String::from("connection test failed");
                for (idx, (kind, base_url)) in candidates.iter().enumerate() {
                    let egress = EgressCtx {
                        base_url: base_url.clone(),
                        model: m.model.clone(),
                        auth: kind.auth_kind(),
                        key: key.clone(),
                        anthropic_version: m.anthropic_version.clone(),
                        default_max_tokens,
                        http: http.clone(),
                    };
                    let connector = make_connector(*kind);
                    let start = std::time::Instant::now();
                    match connector.complete(&req, &egress).await {
                        Ok(_) => {
                            let ms = start.elapsed().as_millis() as u64;
                            // idx==0 is the configured (kind, base_url) — nothing changed
                            if idx == 0 {
                                return serde_json::json!({ "id": m.id, "ok": true, "latency_ms": ms });
                            }
                            // A different (kind and/or base_url) worked — persist it
                            let kind_changed = *kind != configured_kind;
                            let base_changed = base_url != m.base_url.trim_end_matches('/');
                            let persist = if kind_changed {
                                db.model_update_connector_base_url(&m.id, kind.as_str(), base_url).await
                            } else {
                                db.model_update_base_url(&m.id, base_url).await
                            };
                            if let Err(e) = persist {
                                return serde_json::json!({
                                    "id": m.id, "ok": false,
                                    "error": short_err(&format!("detected {} {} but DB update failed: {e}", kind.as_str(), base_url))
                                });
                            }
                            let mut out = serde_json::json!({ "id": m.id, "ok": true, "latency_ms": ms });
                            if base_changed {
                                out["base_url_fixed"] = serde_json::json!(base_url);
                            }
                            if kind_changed {
                                out["connector_fixed"] = serde_json::json!(kind.as_str());
                            }
                            return out;
                        }
                        Err(e) => last_err = e.to_string(),
                    }
                }

                serde_json::json!({ "id": m.id, "ok": false, "error": short_err(&last_err) })
            }
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
    use super::base_url_candidates;

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
}
