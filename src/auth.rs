//! 鉴权模块
//!
//! - `extract_bearer`：从请求头提取 Bearer token 或 x-api-key
//! - `authorize_ingress`：校验 ingress key + ACL
//! - `verify_admin`：校验 admin token（hash 比对）

use axum::http::HeaderMap;

use crate::crypto::sha256_hex;
use crate::db::Db;
use crate::error::FusionError;

/// 从请求头提取明文 token。
///
/// 优先读取 `Authorization: Bearer <token>`，回退到 `x-api-key` 头。
pub fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(rest) = v.strip_prefix("Bearer ") {
            return Some(rest.trim().to_string());
        }
    }
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
}

/// 校验 ingress 请求的 key 及 ACL。
///
/// 1. 提取 Bearer token，缺失返回 401。
/// 2. 调用 `Db::key_authorize`（内部对明文做 sha256），ACL 拒绝返回 401。
pub async fn authorize_ingress(
    db: &Db,
    headers: &HeaderMap,
    virtual_name: &str,
) -> Result<(), FusionError> {
    let key = extract_bearer(headers)
        .ok_or_else(|| FusionError::Unauthorized("missing API key".into()))?;
    if db.key_authorize(&key, virtual_name).await? {
        Ok(())
    } else {
        Err(FusionError::Unauthorized(format!(
            "key not allowed for model '{virtual_name}'"
        )))
    }
}

/// 校验 admin token。
///
/// `db_token_hash`：数据库中存储的 sha256_hex(admin_token)。
/// 提取请求头 token 后做 sha256 比对，不匹配返回 401。
pub fn verify_admin(db_token_hash: &str, headers: &HeaderMap) -> Result<(), FusionError> {
    let token = extract_bearer(headers)
        .ok_or_else(|| FusionError::Unauthorized("missing admin token".into()))?;
    if sha256_hex(&token) == db_token_hash {
        Ok(())
    } else {
        Err(FusionError::Unauthorized("bad admin token".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn extract_from_bearer_and_xapikey() {
        let mut h = HeaderMap::new();
        h.insert("authorization", "Bearer sk-1".parse().unwrap());
        assert_eq!(extract_bearer(&h), Some("sk-1".into()));
        let mut h2 = HeaderMap::new();
        h2.insert("x-api-key", "sk-2".parse().unwrap());
        assert_eq!(extract_bearer(&h2), Some("sk-2".into()));
    }

    #[test]
    fn verify_admin_matches_hash() {
        let hash = crate::crypto::sha256_hex("admintok");
        let mut h = HeaderMap::new();
        h.insert("authorization", "Bearer admintok".parse().unwrap());
        assert!(verify_admin(&hash, &h).is_ok());
        let mut bad = HeaderMap::new();
        bad.insert("authorization", "Bearer wrong".parse().unwrap());
        assert!(verify_admin(&hash, &bad).is_err());
    }

    #[tokio::test]
    async fn authorize_ingress_flow() {
        use crate::db::{models::ModelRow, virtual_models::VirtualModelRow, Db};
        let db = Db::open_memory().await.unwrap();
        db.model_upsert(&ModelRow {
            id: "m".into(),
            connector: "chat".into(),
            base_url: "u".into(),
            api_key_enc: None,
            api_key_env: Some("E".into()),
            model: "m".into(),
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
        db.key_set_acl(id, false, &["vf".into()]).await.unwrap();
        let mut h = HeaderMap::new();
        h.insert("authorization", "Bearer sk-1".parse().unwrap());
        assert!(authorize_ingress(&db, &h, "vf").await.is_ok());
        assert!(authorize_ingress(&db, &HeaderMap::new(), "vf")
            .await
            .is_err());
    }
}
