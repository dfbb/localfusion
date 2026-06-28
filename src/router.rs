use std::str::FromStr;

use crate::connector::{make_connector, resolve_key, ConnectorKind, EgressCtx};
use crate::db::Db;
use crate::error::FusionError;
use crate::strategy::{make_strategy, MemberHandle, StrategyCtx, StrategyOutput};
use crate::unified::{CallRecorder, StrategyTrace, UnifiedRequest};

/// Type alias for test mock closures
#[cfg(test)]
type MockFn = std::sync::Arc<dyn Fn(&str) -> MemberHandle + Send + Sync>;

/// Build the egress HTTP client used for all upstream inference requests.
///
/// Redirects are disabled: the Anthropic connector authenticates with a custom
/// `x-api-key` header, which reqwest does NOT strip on cross-host redirects (it only
/// strips `Authorization`/`Cookie`/`Proxy-Authorization`). Following a 3xx from a
/// configured `base_url` to an attacker-controlled host would therefore leak the
/// decrypted upstream key. Surfacing redirects as errors prevents that.
fn egress_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

/// Model resolver: parses a model_id into a callable MemberHandle (decrypt key + connector + egress)
#[derive(Clone)]
pub struct ModelResolver {
    db: Db,
    enc_key: [u8; 32],
    http: reqwest::Client,
    #[cfg(test)]
    mock: Option<MockFn>,
}

impl ModelResolver {
    /// Production constructor: uses a real DB and encryption key
    pub fn new(db: Db, enc_key: [u8; 32]) -> Self {
        ModelResolver {
            db,
            enc_key,
            http: egress_client(),
            #[cfg(test)]
            mock: None,
        }
    }

    /// Test constructor: injects a mock closure, bypasses DB and decryption
    #[cfg(test)]
    pub fn with_mock(db: Db, f: impl Fn(&str) -> MemberHandle + Send + Sync + 'static) -> Self {
        ModelResolver {
            db,
            enc_key: [0u8; 32],
            http: egress_client(),
            mock: Some(std::sync::Arc::new(f)),
        }
    }

    /// Resolves a model_id into a MemberHandle; in test mode, the mock takes priority
    pub async fn resolve(&self, model_id: &str) -> Result<MemberHandle, FusionError> {
        #[cfg(test)]
        if let Some(f) = &self.mock {
            return Ok(f(model_id));
        }

        let m = self.db.model_get(model_id).await?
            .ok_or_else(|| FusionError::InvalidRequest(format!("unknown model '{model_id}'")))?;

        let kind = ConnectorKind::from_str(&m.connector)
            .map_err(|e| FusionError::InvalidRequest(e.to_string()))?;

        let auth = kind.auth_kind();

        let key = resolve_key(&m, &self.enc_key)?;

        // Read optional default_max_tokens from extra JSON
        let default_max_tokens = m.extra.as_deref()
            .and_then(|e| serde_json::from_str::<serde_json::Value>(e).ok())
            .and_then(|v| v.get("default_max_tokens").and_then(|x| x.as_u64()))
            .map(|x| x as u32);

        let egress = EgressCtx {
            base_url: m.base_url.clone(),
            model: m.model.clone(),
            auth,
            key,
            anthropic_version: m.anthropic_version.clone(),
            default_max_tokens,
            http: self.http.clone(),
        };

        Ok(MemberHandle {
            model_id: m.id.clone(),
            connector: make_connector(kind),
            egress,
        })
    }
}

/// Router: maps a virtual model name → strategy execution → StrategyOutput
pub struct Router {
    pub db: Db,
    pub resolver: ModelResolver,
}

impl Router {
    pub fn new(db: Db, enc_key: [u8; 32]) -> Self {
        Router {
            db: db.clone(),
            resolver: ModelResolver::new(db, enc_key),
        }
    }

    /// Dispatch: resolve virtual model → load strategy → resolve member list → execute strategy
    pub async fn dispatch(
        &self,
        virtual_name: &str,
        req: UnifiedRequest,
        want_stream: bool,
        recorder: &CallRecorder,
        trace: Option<&StrategyTrace>,
    ) -> Result<StrategyOutput, FusionError> {
        let vm = self.db.vmodel_get(virtual_name).await?
            .ok_or_else(|| FusionError::InvalidRequest(format!("unknown virtual model '{virtual_name}'")))?;

        let strategy = make_strategy(&vm.strategy)
            .ok_or_else(|| FusionError::InvalidRequest(format!("unknown strategy '{}'", vm.strategy)))?;

        let params: serde_json::Value =
            serde_json::from_str(&vm.params).unwrap_or(serde_json::Value::Null);

        let member_ids = self.db.vmodel_members(virtual_name).await?;
        let mut members = Vec::with_capacity(member_ids.len());
        for id in &member_ids {
            members.push(self.resolver.resolve(id).await?);
        }

        let ctx = StrategyCtx {
            req,
            members,
            resolver: &self.resolver,
            params,
            db: &self.db,
            want_stream,
            recorder,
            trace,
        };

        strategy.execute(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{derive_key, encrypt, random_salt};
    use crate::db::models::ModelRow;

    #[tokio::test]
    async fn resolve_builds_member_with_decrypted_key() {
        let db = Db::open_memory().await.unwrap();
        let salt = random_salt();
        let key = derive_key(&salt).unwrap();
        let enc = encrypt(&key, "sk-real").unwrap();
        db.model_upsert(&ModelRow {
            id: "m".into(),
            connector: "chat".into(),
            base_url: "https://x/v1".into(),
            api_key_enc: Some(enc),
            api_key_env: None,
            model: "gpt".into(),
            anthropic_version: None,
            extra: None,
        })
        .await
        .unwrap();
        let resolver = ModelResolver::new(db.clone(), key);
        let member = resolver.resolve("m").await.unwrap();
        assert_eq!(member.model_id, "m");
        assert_eq!(member.egress.key.as_deref(), Some("sk-real"));
    }

    #[tokio::test]
    async fn resolve_unknown_model_errors() {
        let db = Db::open_memory().await.unwrap();
        let resolver = ModelResolver::new(db.clone(), [0u8; 32]);
        assert!(resolver.resolve("nope").await.is_err());
    }
}
