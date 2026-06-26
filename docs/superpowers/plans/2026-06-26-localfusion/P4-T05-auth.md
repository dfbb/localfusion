# P4-T05 鉴权

**阶段:** 4 装配 · **前置:** P1-T03, P1-T09 · 见全局约束: `00-index.md`

**Goal:** 入口 key+ACL 鉴权 与 admin token 校验（设计 §5.2）。

**Files:** Modify: `Cargo.toml`（加 axum）, `src/lib.rs`（加 `pub mod auth;`）；Create: `src/auth.rs`

**Produces:** `extract_bearer(headers)->Option<String>`、`authorize_ingress(db,headers,virtual_name)->Result<(),FusionError>`、`verify_admin(hash,headers)->Result<(),FusionError>`。

- [ ] **Step 1: 加 axum 到 Cargo.toml**

`[dependencies]` 增加：`axum = { version = "0.7", features = ["macros"] }`、`tower = "0.5"`、`tower-http = { version = "0.6", features = ["cors"] }`、`clap = { version = "4", features = ["derive"] }`。

- [ ] **Step 2: 写失败测试**

```rust
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
        db.model_upsert(&ModelRow{id:"m".into(),connector:"chat".into(),base_url:"u".into(),
            api_key_enc:None,api_key_env:Some("E".into()),model:"m".into(),anthropic_version:None,extra:None}).await.unwrap();
        db.vmodel_upsert(&VirtualModelRow{name:"vf".into(),strategy:"failover".into(),params:"{}".into()}, &["m".into()]).await.unwrap();
        let id = db.key_insert("sk-1", None, 0).await.unwrap();
        db.key_set_acl(id, false, &["vf".into()]).await.unwrap();
        let mut h = HeaderMap::new();
        h.insert("authorization", "Bearer sk-1".parse().unwrap());
        assert!(authorize_ingress(&db, &h, "vf").await.is_ok());
        assert!(authorize_ingress(&db, &HeaderMap::new(), "vf").await.is_err());
    }
}
```

- [ ] **Step 3: 运行确认失败** — Run: `cargo test --lib auth` → FAIL。

- [ ] **Step 4: 实现 auth.rs**

```rust
use axum::http::HeaderMap;

use crate::crypto::sha256_hex;
use crate::db::Db;
use crate::error::FusionError;

pub fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(rest) = v.strip_prefix("Bearer ") { return Some(rest.trim().to_string()); }
    }
    headers.get("x-api-key").and_then(|v| v.to_str().ok()).map(|s| s.trim().to_string())
}

pub async fn authorize_ingress(db: &Db, headers: &HeaderMap, virtual_name: &str) -> Result<(), FusionError> {
    let key = extract_bearer(headers)
        .ok_or_else(|| FusionError::Unauthorized("missing API key".into()))?;
    if db.key_authorize(&key, virtual_name).await? { Ok(()) }
    else { Err(FusionError::Unauthorized(format!("key not allowed for model '{virtual_name}'"))) }
}

pub fn verify_admin(db_token_hash: &str, headers: &HeaderMap) -> Result<(), FusionError> {
    let token = extract_bearer(headers)
        .ok_or_else(|| FusionError::Unauthorized("missing admin token".into()))?;
    if sha256_hex(&token) == db_token_hash { Ok(()) }
    else { Err(FusionError::Unauthorized("bad admin token".into())) }
}
```

- [ ] **Step 5: 运行确认通过 + 提交**

```bash
cargo test --lib auth && cargo clippy --all-targets
git add Cargo.toml src/lib.rs src/auth.rs
git commit -m "feat: 入口鉴权(key+ACL) + admin token 校验"
```
