mod anthropic;
mod chat;
mod responses;
pub mod sse;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::str::FromStr;

use crate::db::models::ModelRow;
use crate::unified::{ConnError, UnifiedRequest, UnifiedResponse, UnifiedStream};

/// Connector type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorKind {
    Chat,
    Anthropic,
    Responses,
}

impl FromStr for ConnectorKind {
    type Err = ConnError;
    fn from_str(s: &str) -> Result<Self, ConnError> {
        match s {
            "chat" => Ok(ConnectorKind::Chat),
            "anthropic" => Ok(ConnectorKind::Anthropic),
            "responses" => Ok(ConnectorKind::Responses),
            other => Err(ConnError::HardFail(format!("unknown connector '{other}'"))),
        }
    }
}

impl ConnectorKind {
    /// Canonical string name (matches the `connector` column values).
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectorKind::Chat => "chat",
            ConnectorKind::Anthropic => "anthropic",
            ConnectorKind::Responses => "responses",
        }
    }

    /// The auth scheme each API format expects.
    pub fn auth_kind(self) -> AuthKind {
        match self {
            ConnectorKind::Anthropic => AuthKind::XApiKey,
            _ => AuthKind::Bearer,
        }
    }

    /// All connector kinds, for probe-based auto-detection.
    pub fn all() -> [ConnectorKind; 3] {
        [ConnectorKind::Chat, ConnectorKind::Anthropic, ConnectorKind::Responses]
    }
}

/// Authentication method enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    /// OpenAI style: Authorization: Bearer <key>
    Bearer,
    /// Anthropic style: x-api-key: <key>
    XApiKey,
}

/// Egress request context containing all configuration needed for the connector to make requests
pub struct EgressCtx {
    pub base_url: String,
    pub model: String,
    pub auth: AuthKind,
    pub key: Option<String>,
    pub anthropic_version: Option<String>,
    pub default_max_tokens: Option<u32>,
    pub http: reqwest::Client,
}

/// Egress adapter trait; implemented once per API format
#[async_trait]
pub trait Connector: Send + Sync {
    async fn complete(
        &self,
        req: &UnifiedRequest,
        ctx: &EgressCtx,
    ) -> Result<UnifiedResponse, ConnError>;

    async fn stream(
        &self,
        req: &UnifiedRequest,
        ctx: &EgressCtx,
    ) -> Result<UnifiedStream, ConnError>;
}

/// Create the corresponding Connector instance based on ConnectorKind
pub fn make_connector(kind: ConnectorKind) -> Box<dyn Connector> {
    match kind {
        ConnectorKind::Chat => Box::new(chat::ChatConnector),
        ConnectorKind::Anthropic => Box::new(anthropic::AnthropicConnector),
        ConnectorKind::Responses => Box::new(responses::ResponsesConnector),
    }
}

/// API path suffix for each connector type
fn default_path(kind: ConnectorKind) -> &'static str {
    match kind {
        ConnectorKind::Chat => "/chat/completions",
        ConnectorKind::Anthropic => "/v1/messages",
        ConnectorKind::Responses => "/responses",
    }
}

/// Build egress URL: strip trailing slash from base_url then append the path suffix
pub fn egress_url(base_url: &str, kind: ConnectorKind) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), default_path(kind))
}

/// Build request headers: includes auth header, Content-Type, and Anthropic version header (XApiKey mode)
pub fn build_headers(
    auth: AuthKind,
    key: Option<&str>,
    anthropic_version: Option<&str>,
) -> Result<HeaderMap, ConnError> {
    let mut h = HeaderMap::new();
    match auth {
        AuthKind::Bearer => {
            let k = key.ok_or_else(|| ConnError::HardFail("missing API key".into()))?;
            h.insert(
                reqwest::header::AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {k}"))
                    .map_err(|e| ConnError::HardFail(e.to_string()))?,
            );
        }
        AuthKind::XApiKey => {
            let k = key.ok_or_else(|| ConnError::HardFail("missing API key".into()))?;
            h.insert(
                HeaderName::from_static("x-api-key"),
                HeaderValue::from_str(k).map_err(|e| ConnError::HardFail(e.to_string()))?,
            );
            let ver = anthropic_version.unwrap_or("2023-06-01");
            h.insert(
                HeaderName::from_static("anthropic-version"),
                HeaderValue::from_str(ver).map_err(|e| ConnError::HardFail(e.to_string()))?,
            );
        }
    }
    h.insert(
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Ok(h)
}

/// Resolve API key: prefers the encrypted api_key_enc, falls back to reading from environment variable
pub fn resolve_key(m: &ModelRow, enc_key: &[u8; 32]) -> Result<Option<String>, ConnError> {
    if let Some(enc) = &m.api_key_enc {
        let pt = crate::crypto::decrypt(enc_key, enc)
            .map_err(|_| ConnError::HardFail("api_key decrypt failed".into()))?;
        return Ok(Some(pt));
    }
    if let Some(env) = &m.api_key_env {
        if let Ok(v) = std::env::var(env) {
            if !v.is_empty() {
                return Ok(Some(v));
            }
        }
    }
    Ok(None)
}

/// Log outgoing request and incoming response body at DEBUG level.
/// No-op unless the debug log level is active, so zero overhead in production.
/// API keys in Authorization headers are redacted.
pub fn log_http_exchange(method: &str, url: &str, req_body: &serde_json::Value, status: u16, resp_body: &str) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    tracing::debug!(
        method,
        url,
        status,
        req_body = %req_body,
        resp_body = %resp_body,
        "upstream HTTP exchange"
    );
}

/// Sanitized wrapper for upstream HTTP errors (design §5.3 "vendor raw errors may be truncated and sanitized").
///
/// Truncates the upstream response body to at most `MAX` characters (on character boundaries, UTF-8 safe),
/// preventing vendor internal details / rate-limit messages / potentially sensitive fragments from being
/// forwarded verbatim to the client. An empty body omits everything after the colon.
pub fn upstream_error(status: reqwest::StatusCode, body: &str) -> ConnError {
    const MAX: usize = 200;
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return ConnError::Http(format!("upstream {status}"));
    }
    let mut snippet: String = trimmed.chars().take(MAX).collect();
    if trimmed.chars().count() > MAX {
        snippet.push('…');
    }
    ConnError::Http(format!("upstream {status}: {snippet}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_and_path() {
        assert_eq!(
            egress_url("https://api.openai.com/v1/", ConnectorKind::Chat),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            egress_url("https://api.anthropic.com", ConnectorKind::Anthropic),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn upstream_error_truncates_and_handles_empty() {
        use reqwest::StatusCode;
        // long error body is truncated to 200 characters with ellipsis, preventing full upstream text from being forwarded
        let long = "x".repeat(500);
        let e = upstream_error(StatusCode::BAD_GATEWAY, &long);
        let msg = e.to_string();
        assert!(msg.contains('…'));
        // truncated body contains no more than 200 'x' characters
        assert_eq!(msg.matches('x').count(), 200);
        // empty body omits content after colon (only the colon from the Display prefix "connector http:" remains)
        let empty = upstream_error(StatusCode::INTERNAL_SERVER_ERROR, "   ");
        assert_eq!(empty.to_string().matches(':').count(), 1);
    }

    #[test]
    fn bearer_header() {
        let h = build_headers(AuthKind::Bearer, Some("k"), None).unwrap();
        assert_eq!(h.get("authorization").unwrap(), "Bearer k");
        assert_eq!(h.get("content-type").unwrap(), "application/json");
    }

    #[test]
    fn xapikey_header() {
        let h = build_headers(AuthKind::XApiKey, Some("k"), Some("2023-06-01")).unwrap();
        assert_eq!(h.get("x-api-key").unwrap(), "k");
        assert_eq!(h.get("anthropic-version").unwrap(), "2023-06-01");
    }

    #[test]
    fn resolve_key_prefers_enc_then_env() {
        let key = crate::crypto::derive_key(&[5u8; 16]).unwrap();
        let enc = crate::crypto::encrypt(&key, "secret-enc").unwrap();
        let m = crate::db::models::ModelRow {
            id: "x".into(),
            connector: "chat".into(),
            base_url: "u".into(),
            api_key_enc: Some(enc),
            api_key_env: None,
            model: "x".into(),
            anthropic_version: None,
            extra: None,
        };
        assert_eq!(resolve_key(&m, &key).unwrap(), Some("secret-enc".into()));
    }
}
