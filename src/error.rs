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
    /// Maps to HTTP status (design §10). Unauthorized defaults to 401 (callers may use 403 for ACL-denial scenarios).
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
