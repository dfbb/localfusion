use std::str::FromStr;

use crate::connector::{make_connector, resolve_key, AuthKind, ConnectorKind, EgressCtx};
use crate::db::Db;
use crate::error::FusionError;
use crate::strategy::{make_strategy, MemberHandle, StrategyCtx, StrategyOutput};
use crate::unified::{CallRecorder, StrategyTrace, UnifiedRequest};

/// 测试用 mock 闭包类型别名
#[cfg(test)]
type MockFn = Box<dyn Fn(&str) -> MemberHandle + Send + Sync>;

/// 模型解析器：把 model_id 解析成可调用的 MemberHandle（解密 key + connector + egress）
pub struct ModelResolver {
    db: Db,
    enc_key: [u8; 32],
    http: reqwest::Client,
    #[cfg(test)]
    mock: Option<MockFn>,
}

impl ModelResolver {
    /// 生产构造，使用真实 DB 和加密 key
    pub fn new(db: Db, enc_key: [u8; 32]) -> Self {
        ModelResolver {
            db,
            enc_key,
            http: reqwest::Client::new(),
            #[cfg(test)]
            mock: None,
        }
    }

    /// 测试构造，注入 mock 闭包，跳过 DB + 解密
    #[cfg(test)]
    pub fn with_mock(db: Db, f: impl Fn(&str) -> MemberHandle + Send + Sync + 'static) -> Self {
        ModelResolver {
            db,
            enc_key: [0u8; 32],
            http: reqwest::Client::new(),
            mock: Some(Box::new(f)),
        }
    }

    /// 根据 model_id 解析出 MemberHandle；测试环境下优先使用 mock
    pub async fn resolve(&self, model_id: &str) -> Result<MemberHandle, FusionError> {
        #[cfg(test)]
        if let Some(f) = &self.mock {
            return Ok(f(model_id));
        }

        let m = self.db.model_get(model_id).await?
            .ok_or_else(|| FusionError::InvalidRequest(format!("unknown model '{model_id}'")))?;

        let kind = ConnectorKind::from_str(&m.connector)
            .map_err(|e| FusionError::InvalidRequest(e.to_string()))?;

        // Anthropic 使用 x-api-key 头，其余使用 Bearer
        let auth = match kind {
            ConnectorKind::Anthropic => AuthKind::XApiKey,
            _ => AuthKind::Bearer,
        };

        let key = resolve_key(&m, &self.enc_key)?;

        // 从 extra JSON 读取可选的 default_max_tokens
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

/// Router：把虚拟模型名 → 策略执行 → StrategyOutput
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

    /// 调度：解析虚拟模型 → 加载策略 → 解析成员列表 → 执行策略
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
