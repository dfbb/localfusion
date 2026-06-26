use crate::crypto::sha256_hex;
use crate::db::Db;
use crate::error::FusionError;

/// ingress_keys 表行结构体（不含 key_hash 明文）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct KeyRow {
    pub id: i64,
    pub label: Option<String>,
    pub enabled: bool,
    pub acl_all: bool,
    pub created_at: i64,
}

impl Db {
    /// 列出所有 ingress key（按 id 排序）。
    pub async fn key_list(&self) -> Result<Vec<KeyRow>, FusionError> {
        let rows: Vec<(i64, Option<String>, i64, i64, i64)> = sqlx::query_as(
            "SELECT id, label, enabled, acl_all, created_at FROM ingress_keys ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, label, enabled, acl_all, created_at)| KeyRow {
                id,
                label,
                enabled: enabled != 0,
                acl_all: acl_all != 0,
                created_at,
            })
            .collect())
    }

    /// 插入新 ingress key（存 SHA-256 哈希，不存明文）。
    /// 返回新行的 id。
    pub async fn key_insert(
        &self,
        plaintext: &str,
        label: Option<&str>,
        created_at: i64,
    ) -> Result<i64, FusionError> {
        let hash = sha256_hex(plaintext);
        let r = sqlx::query(
            "INSERT INTO ingress_keys(key_hash, label, created_at) VALUES(?,?,?)",
        )
        .bind(hash)
        .bind(label)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(r.last_insert_rowid())
    }

    /// 更新 enabled 状态和 label（label 传 None 时保持原值）。
    pub async fn key_set_enabled_label(
        &self,
        id: i64,
        enabled: bool,
        label: Option<&str>,
    ) -> Result<(), FusionError> {
        sqlx::query(
            "UPDATE ingress_keys SET enabled = ?, label = COALESCE(?, label) WHERE id = ?",
        )
        .bind(enabled as i64)
        .bind(label)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 删除指定 ingress key（关联的 ACL 行会级联删除）。
    pub async fn key_delete(&self, id: i64) -> Result<(), FusionError> {
        sqlx::query("DELETE FROM ingress_keys WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 设置 ACL：acl_all=true 表示允许所有虚拟模型；否则使用 names 白名单。
    /// 事务内完成：更新 acl_all 标志、清空旧白名单、写入新白名单。
    pub async fn key_set_acl(
        &self,
        id: i64,
        acl_all: bool,
        names: &[String],
    ) -> Result<(), FusionError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE ingress_keys SET acl_all = ? WHERE id = ?")
            .bind(acl_all as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM ingress_key_acl WHERE key_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for name in names {
            sqlx::query(
                "INSERT INTO ingress_key_acl(key_id, virtual_name) VALUES(?,?)",
            )
            .bind(id)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// 获取指定 key 的 ACL 白名单虚拟模型名列表（按名称排序）。
    pub async fn key_acl_names(&self, id: i64) -> Result<Vec<String>, FusionError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT virtual_name FROM ingress_key_acl WHERE key_id = ? ORDER BY virtual_name",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// 鉴权：传入明文 key 和目标虚拟模型名，返回是否允许访问。
    /// 规则：key 不存在 → false；disabled → false；acl_all=1 → true；否则查白名单。
    pub async fn key_authorize(
        &self,
        plaintext: &str,
        virtual_name: &str,
    ) -> Result<bool, FusionError> {
        let hash = sha256_hex(plaintext);
        let row: Option<(i64, i64, i64)> = sqlx::query_as(
            "SELECT id, enabled, acl_all FROM ingress_keys WHERE key_hash = ?",
        )
        .bind(&hash)
        .fetch_optional(&self.pool)
        .await?;
        let Some((id, enabled, acl_all)) = row else {
            return Ok(false);
        };
        if enabled == 0 {
            return Ok(false);
        }
        if acl_all != 0 {
            return Ok(true);
        }
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM ingress_key_acl WHERE key_id = ? AND virtual_name = ?",
        )
        .bind(id)
        .bind(virtual_name)
        .fetch_one(&self.pool)
        .await?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::{models::ModelRow, virtual_models::VirtualModelRow, Db};

    /// 辅助：插入一个模型和虚拟模型（用于 ACL 测试）。
    async fn seed_vm(db: &Db, name: &str) {
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
        .ok();
        db.vmodel_upsert(
            &VirtualModelRow {
                name: name.into(),
                strategy: "failover".into(),
                params: "{}".into(),
            },
            &["m".into()],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn insert_list_no_plaintext_and_patch() {
        let db = Db::open_memory().await.unwrap();
        let id = db.key_insert("sk-plain", Some("ci"), 1000).await.unwrap();
        let rows = db.key_list().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label.as_deref(), Some("ci"));
        assert!(rows[0].enabled);
        db.key_set_enabled_label(id, false, Some("disabled"))
            .await
            .unwrap();
        let rows = db.key_list().await.unwrap();
        assert!(!rows[0].enabled);
        assert_eq!(rows[0].label.as_deref(), Some("disabled"));
    }

    #[tokio::test]
    async fn authorize_respects_enabled_and_acl() {
        let db = Db::open_memory().await.unwrap();
        seed_vm(&db, "vf").await;
        seed_vm(&db, "other").await;
        let id = db.key_insert("sk-1", None, 0).await.unwrap();
        // 默认 acl_all=0，无白名单 → 拒绝
        assert!(!db.key_authorize("sk-1", "vf").await.unwrap());
        // 加入白名单 vf → 允许 vf，拒绝 other
        db.key_set_acl(id, false, &["vf".into()]).await.unwrap();
        assert!(db.key_authorize("sk-1", "vf").await.unwrap());
        assert!(!db.key_authorize("sk-1", "other").await.unwrap());
        // acl_all=true → 允许所有
        db.key_set_acl(id, true, &[]).await.unwrap();
        assert!(db.key_authorize("sk-1", "other").await.unwrap());
        // 错误 key → 拒绝
        assert!(!db.key_authorize("sk-wrong", "vf").await.unwrap());
        // disabled → 拒绝
        db.key_set_enabled_label(id, false, None).await.unwrap();
        assert!(!db.key_authorize("sk-1", "vf").await.unwrap());
    }

    #[tokio::test]
    async fn acl_cascade_on_vmodel_delete() {
        let db = Db::open_memory().await.unwrap();
        seed_vm(&db, "vf").await;
        let id = db.key_insert("sk-1", None, 0).await.unwrap();
        db.key_set_acl(id, false, &["vf".into()]).await.unwrap();
        assert_eq!(db.key_acl_names(id).await.unwrap(), vec!["vf"]);
        // 删除虚拟模型后，ACL 行应级联删除
        db.vmodel_delete("vf").await.unwrap();
        assert!(db.key_acl_names(id).await.unwrap().is_empty());
    }
}
