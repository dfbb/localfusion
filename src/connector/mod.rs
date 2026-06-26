mod anthropic;
mod chat;
mod responses;
pub mod sse;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::str::FromStr;

use crate::db::models::ModelRow;
use crate::unified::{ConnError, UnifiedRequest, UnifiedResponse, UnifiedStream};

/// 连接器类型枚举
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

/// 鉴权方式枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    /// OpenAI 风格：Authorization: Bearer <key>
    Bearer,
    /// Anthropic 风格：x-api-key: <key>
    XApiKey,
}

/// 出口请求上下文，包含连接器发起请求所需的全部配置
pub struct EgressCtx {
    pub base_url: String,
    pub model: String,
    pub auth: AuthKind,
    pub key: Option<String>,
    pub anthropic_version: Option<String>,
    pub default_max_tokens: Option<u32>,
    pub http: reqwest::Client,
}

/// 出口适配器 trait；每种 API 格式实现一次
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

/// 根据 ConnectorKind 创建对应的 Connector 实例
pub fn make_connector(kind: ConnectorKind) -> Box<dyn Connector> {
    match kind {
        ConnectorKind::Chat => Box::new(chat::ChatConnector),
        ConnectorKind::Anthropic => Box::new(anthropic::AnthropicConnector),
        ConnectorKind::Responses => Box::new(responses::ResponsesConnector),
    }
}

/// 每种连接器对应的 API 路径后缀
fn default_path(kind: ConnectorKind) -> &'static str {
    match kind {
        ConnectorKind::Chat => "/chat/completions",
        ConnectorKind::Anthropic => "/v1/messages",
        ConnectorKind::Responses => "/responses",
    }
}

/// 拼接出口 URL：base_url 去掉尾部斜杠后追加路径后缀
pub fn egress_url(base_url: &str, kind: ConnectorKind) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), default_path(kind))
}

/// 构建请求头：包含鉴权头、Content-Type，以及 Anthropic 版本头（XApiKey 模式）
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

/// 解析 API key：优先使用加密存储的 api_key_enc，其次从环境变量读取
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
