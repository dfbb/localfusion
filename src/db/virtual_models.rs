use crate::db::Db;
use crate::error::FusionError;

/// Row struct for the virtual models table.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct VirtualModelRow {
    pub name: String,
    pub strategy: String,
    pub params: String,
}

/// Model reference: records the virtual model name and reference kind (member/judge/web_search, etc.) for a given model.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelRef {
    pub virtual_name: String,
    pub ref_kind: String,
}

impl Db {
    /// List all virtual models (sorted by name).
    pub async fn vmodel_list(&self) -> Result<Vec<VirtualModelRow>, FusionError> {
        Ok(sqlx::query_as::<_, VirtualModelRow>("SELECT * FROM virtual_models ORDER BY name")
            .fetch_all(&self.pool)
            .await?)
    }

    /// Get a single virtual model by name; returns None if it does not exist.
    pub async fn vmodel_get(&self, name: &str) -> Result<Option<VirtualModelRow>, FusionError> {
        Ok(sqlx::query_as::<_, VirtualModelRow>("SELECT * FROM virtual_models WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// Get the list of member models for a virtual model (sorted by position).
    pub async fn vmodel_members(&self, name: &str) -> Result<Vec<String>, FusionError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT model_id FROM virtual_model_members WHERE virtual_name = ? ORDER BY position",
        )
        .bind(name)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Upsert a virtual model and its member list (executed in a transaction).
    /// - First insert or update the virtual model row
    /// - Clear the existing member list
    /// - Re-insert members in order from the members slice, with position starting at 0
    pub async fn vmodel_upsert(
        &self,
        row: &VirtualModelRow,
        members: &[String],
    ) -> Result<(), FusionError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO virtual_models(name, strategy, params) VALUES(?,?,?)
             ON CONFLICT(name) DO UPDATE SET strategy=excluded.strategy, params=excluded.params",
        )
        .bind(&row.name)
        .bind(&row.strategy)
        .bind(&row.params)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM virtual_model_members WHERE virtual_name = ?")
            .bind(&row.name)
            .execute(&mut *tx)
            .await?;
        for (pos, mid) in members.iter().enumerate() {
            sqlx::query(
                "INSERT INTO virtual_model_members(virtual_name, model_id, position) VALUES(?,?,?)",
            )
            .bind(&row.name)
            .bind(mid)
            .bind(pos as i64)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Delete a virtual model (members are cascade-deleted because the foreign key is configured with ON DELETE CASCADE).
    pub async fn vmodel_delete(&self, name: &str) -> Result<(), FusionError> {
        sqlx::query("DELETE FROM virtual_models WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Find all references to a given model.
    /// Returns the virtual model names that reference this model and the reference kind (member/judge/web_search/image_generation/tool_search/image_query).
    pub async fn model_references(&self, model_id: &str) -> Result<Vec<ModelRef>, FusionError> {
        let mut refs = Vec::new();

        // Find virtual models where this model appears as a member
        let member_of: Vec<(String,)> = sqlx::query_as(
            "SELECT virtual_name FROM virtual_model_members WHERE model_id = ?",
        )
        .bind(model_id)
        .fetch_all(&self.pool)
        .await?;
        for (vn,) in member_of {
            refs.push(ModelRef {
                virtual_name: vn,
                ref_kind: "member".into(),
            });
        }

        // Find virtual models where this model appears as a route key in the params JSON
        const ROUTE_KEYS: [&str; 5] = ["judge", "web_search", "image_generation", "tool_search", "image_query"];
        for vm in self.vmodel_list().await? {
            let params: serde_json::Value =
                serde_json::from_str(&vm.params).unwrap_or(serde_json::Value::Null);
            for key in ROUTE_KEYS {
                if params.get(key).and_then(|v| v.as_str()) == Some(model_id) {
                    refs.push(ModelRef {
                        virtual_name: vm.name.clone(),
                        ref_kind: key.into(),
                    });
                }
            }
        }

        Ok(refs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::ModelRow;

    /// Helper function: create a model and insert it into the database.
    async fn seed_model(db: &Db, id: &str) {
        db.model_upsert(&ModelRow {
            id: id.into(),
            connector: "chat".into(),
            base_url: "u".into(),
            api_key_enc: None,
            api_key_env: Some("E".into()),
            model: id.into(),
            anthropic_version: None,
            extra: None,
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn upsert_members_and_order() {
        let db = Db::open_memory().await.unwrap();
        seed_model(&db, "a").await;
        seed_model(&db, "b").await;
        let row = VirtualModelRow {
            name: "vf".into(),
            strategy: "failover".into(),
            params: "{}".into(),
        };
        db.vmodel_upsert(&row, &["a".into(), "b".into()])
            .await
            .unwrap();
        assert_eq!(db.vmodel_members("vf").await.unwrap(), vec!["a", "b"]);
        db.vmodel_upsert(&row, &["b".into(), "a".into()])
            .await
            .unwrap();
        assert_eq!(db.vmodel_members("vf").await.unwrap(), vec!["b", "a"]);
    }

    #[tokio::test]
    async fn delete_cascades_members() {
        let db = Db::open_memory().await.unwrap();
        seed_model(&db, "a").await;
        let row = VirtualModelRow {
            name: "vf".into(),
            strategy: "failover".into(),
            params: "{}".into(),
        };
        db.vmodel_upsert(&row, &["a".into()]).await.unwrap();
        db.vmodel_delete("vf").await.unwrap();
        assert!(db.vmodel_get("vf").await.unwrap().is_none());
        assert!(db.vmodel_members("vf").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn references_detect_member_judge_and_route() {
        let db = Db::open_memory().await.unwrap();
        for id in ["a", "j", "ws"] {
            seed_model(&db, id).await;
        }
        db.vmodel_upsert(
            &VirtualModelRow {
                name: "syn".into(),
                strategy: "synthesize".into(),
                params: r#"{"judge":"j"}"#.into(),
            },
            &["a".into()],
        )
        .await
        .unwrap();
        db.vmodel_upsert(
            &VirtualModelRow {
                name: "mm".into(),
                strategy: "multimodal".into(),
                params: r#"{"web_search":"ws"}"#.into(),
            },
            &["a".into()],
        )
        .await
        .unwrap();
        assert!(db
            .model_references("a")
            .await
            .unwrap()
            .iter()
            .any(|r| r.ref_kind == "member"));
        assert!(db
            .model_references("j")
            .await
            .unwrap()
            .iter()
            .any(|r| r.ref_kind == "judge" && r.virtual_name == "syn"));
        assert!(db
            .model_references("ws")
            .await
            .unwrap()
            .iter()
            .any(|r| r.ref_kind == "web_search" && r.virtual_name == "mm"));
        assert!(db
            .model_references("nonexistent")
            .await
            .unwrap()
            .is_empty());
    }
}
