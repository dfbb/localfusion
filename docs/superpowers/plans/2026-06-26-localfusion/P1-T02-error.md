# P1-T02 FusionError 统一错误类型

**阶段:** 1 基础层 · **前置:** P1-T01 · 见全局约束: `00-index.md`

**Goal:** 统一错误类型 + HTTP 状态映射（设计 §10）。

**Files:** Modify: `src/error.rs`

**Produces:** `pub enum FusionError { InvalidRequest(String), Unauthorized(String), UpstreamError{status:u16,message:String}, AllMembersFailed(String), StrategyError(String), Internal(String) }`；`fn http_status(&self)->u16`；`From<sqlx::Error>`。

- [ ] **Step 1: 写失败测试（追加到 error.rs 末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_mapping() {
        assert_eq!(FusionError::InvalidRequest("x".into()).http_status(), 400);
        assert_eq!(FusionError::Unauthorized("x".into()).http_status(), 401);
        assert_eq!(FusionError::UpstreamError { status: 502, message: "x".into() }.http_status(), 502);
        assert_eq!(FusionError::AllMembersFailed("x".into()).http_status(), 502);
        assert_eq!(FusionError::StrategyError("x".into()).http_status(), 502);
        assert_eq!(FusionError::Internal("x".into()).http_status(), 500);
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib error` → FAIL。

- [ ] **Step 3: 实现（置于 error.rs 顶部）**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FusionError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("upstream error ({status}): {message}")]
    UpstreamError { status: u16, message: String },
    #[error("all members failed: {0}")]
    AllMembersFailed(String),
    #[error("strategy error: {0}")]
    StrategyError(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl FusionError {
    /// 映射 HTTP 状态（设计 §10）。Unauthorized 默认 401（ACL 拒绝场景调用方可改 403）。
    pub fn http_status(&self) -> u16 {
        match self {
            FusionError::InvalidRequest(_) => 400,
            FusionError::Unauthorized(_) => 401,
            FusionError::UpstreamError { .. }
            | FusionError::AllMembersFailed(_)
            | FusionError::StrategyError(_) => 502,
            FusionError::Internal(_) => 500,
        }
    }
}

impl From<sqlx::Error> for FusionError {
    fn from(e: sqlx::Error) -> Self {
        FusionError::Internal(format!("db: {e}"))
    }
}
```

- [ ] **Step 4: 运行确认通过** — Run: `cargo test --lib error` → PASS。

- [ ] **Step 5: 提交**

```bash
git add src/error.rs
git commit -m "feat: FusionError 统一错误类型与 HTTP 状态映射"
```
