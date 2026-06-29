use crate::db::Db;
use crate::error::FusionError;

// Per-model price row (USD/million tokens). Read by cost calculation.
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct PriceRow {
    pub model_id: String,
    pub price_in: f64,
    pub price_out: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub updated_at: i64,
}

// Model-id-free default prices (USD/million tokens), as matched from the litellm snapshot.
// The caller attaches a local model_id + updated_at to build a PriceRow.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PriceValues {
    pub price_in: f64,
    pub price_out: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl Db {
    /// Get all price records, ordered by model_id
    pub async fn price_list(&self) -> Result<Vec<PriceRow>, FusionError> {
        Ok(sqlx::query_as::<_, PriceRow>("SELECT * FROM prices ORDER BY model_id")
            .fetch_all(&self.pool)
            .await?)
    }

    /// Get a single price record by model_id; returns None if not found
    pub async fn price_get(&self, model_id: &str) -> Result<Option<PriceRow>, FusionError> {
        Ok(sqlx::query_as::<_, PriceRow>("SELECT * FROM prices WHERE model_id = ?")
            .bind(model_id)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// Insert or update a price row; on model_id conflict, update all four prices + updated_at.
    pub async fn price_upsert(&self, p: &PriceRow) -> Result<(), FusionError> {
        sqlx::query(
            "INSERT INTO prices(model_id, price_in, price_out, cache_read, cache_write, updated_at)
             VALUES(?,?,?,?,?,?)
             ON CONFLICT(model_id) DO UPDATE SET price_in=excluded.price_in,
               price_out=excluded.price_out, cache_read=excluded.cache_read,
               cache_write=excluded.cache_write, updated_at=excluded.updated_at",
        )
        .bind(&p.model_id)
        .bind(p.price_in)
        .bind(p.price_out)
        .bind(p.cache_read)
        .bind(p.cache_write)
        .bind(p.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Replace the entire price_defaults snapshot in one transaction.
    /// `updated_at` is supplied by the caller (Unix seconds) so the function stays pure.
    pub async fn defaults_replace_all(
        &self,
        rows: &[(String, PriceValues)],
        updated_at: i64,
    ) -> Result<(), FusionError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM price_defaults").execute(&mut *tx).await?;
        for (key, v) in rows {
            sqlx::query(
                "INSERT INTO price_defaults(model_key, price_in, price_out, cache_read, cache_write, updated_at)
                 VALUES(?,?,?,?,?,?)",
            )
            .bind(key)
            .bind(v.price_in)
            .bind(v.price_out)
            .bind(v.cache_read)
            .bind(v.cache_write)
            .bind(updated_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Load all (model_key, PriceValues) from the snapshot (model_key lowercased for matching).
    async fn price_defaults_all(&self) -> Result<Vec<(String, PriceValues)>, FusionError> {
        let rows = sqlx::query_as::<_, (String, f64, f64, f64, f64)>(
            "SELECT model_key, price_in, price_out, cache_read, cache_write FROM price_defaults",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(k, pi, po, cr, cw)| {
                (k, PriceValues { price_in: pi, price_out: po, cache_read: cr, cache_write: cw })
            })
            .collect())
    }

    /// Fuzzy-match a model name against the litellm snapshot. Returns model-id-free
    /// PriceValues. Order: exact -> normalized('.'->'-') exact -> substring-contains
    /// (shortest key wins; ties -> lexicographically greatest) -> None. Case-insensitive.
    pub async fn defaults_match(&self, model: &str) -> Result<Option<PriceValues>, FusionError> {
        let all = self.price_defaults_all().await?;
        if all.is_empty() {
            return Ok(None);
        }
        let needle = model.to_lowercase();
        let normalized = needle.replace('.', "-");

        // Build (lowercased_key, original_index) once.
        let lowered: Vec<(String, usize)> =
            all.iter().enumerate().map(|(i, (k, _))| (k.to_lowercase(), i)).collect();

        // 1. exact on raw needle
        if let Some((_, i)) = lowered.iter().find(|(k, _)| *k == needle) {
            return Ok(Some(all[*i].1.clone()));
        }
        // 2. normalized exact
        if let Some((_, i)) = lowered.iter().find(|(k, _)| *k == normalized) {
            return Ok(Some(all[*i].1.clone()));
        }
        // 3. substring-contains (normalized needle inside key); shortest key, ties -> greatest key
        let best = lowered
            .iter()
            .filter(|(k, _)| k.contains(&normalized))
            .min_by(|a, b| {
                a.0.len()
                    .cmp(&b.0.len())
                    .then_with(|| b.0.cmp(&a.0)) // tie: lexicographically greatest first
            });
        if let Some((_, i)) = best {
            return Ok(Some(all[*i].1.clone()));
        }
        // 4. none
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_and_get() {
        let db = Db::open_memory().await.unwrap();
        assert!(db.price_get("gpt-4o").await.unwrap().is_none());
        db.price_upsert(&PriceRow {
            model_id: "gpt-4o".into(),
            price_in: 2.5,
            price_out: 10.0,
            cache_read: 0.0,
            cache_write: 0.0,
            updated_at: 100,
        })
        .await
        .unwrap();
        assert_eq!(
            db.price_get("gpt-4o").await.unwrap().unwrap().price_out,
            10.0
        );
        assert_eq!(db.price_list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn upsert_writes_cache_prices() {
        let db = Db::open_memory().await.unwrap();
        db.price_upsert(&PriceRow {
            model_id: "m".into(), price_in: 1.0, price_out: 2.0,
            cache_read: 0.3, cache_write: 0.5, updated_at: 1,
        }).await.unwrap();
        let got = db.price_get("m").await.unwrap().unwrap();
        assert_eq!(got.cache_read, 0.3);
        assert_eq!(got.cache_write, 0.5);
    }

    #[tokio::test]
    async fn defaults_replace_all_rewrites_snapshot() {
        let db = Db::open_memory().await.unwrap();
        let rows = vec![
            ("a".to_string(), PriceValues { price_in: 1.0, price_out: 2.0, cache_read: 0.0, cache_write: 0.0 }),
            ("b".to_string(), PriceValues { price_in: 3.0, price_out: 4.0, cache_read: 0.1, cache_write: 0.2 }),
        ];
        db.defaults_replace_all(&rows, 100).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM price_defaults")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(n, 2);
        // a second replace with fewer rows fully rewrites (old rows gone)
        db.defaults_replace_all(&rows[..1], 200).await.unwrap();
        let n2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM price_defaults")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(n2, 1);
    }

    async fn seed_defaults(db: &Db) {
        let rows = vec![
            ("gpt-4o".to_string(), PriceValues { price_in: 2.5, price_out: 10.0, cache_read: 0.0, cache_write: 0.0 }),
            ("claude-opus-4-8-20260527".to_string(), PriceValues { price_in: 5.0, price_out: 25.0, cache_read: 0.5, cache_write: 6.0 }),
            ("claude-opus-4-8-20260615".to_string(), PriceValues { price_in: 5.0, price_out: 25.0, cache_read: 0.5, cache_write: 6.0 }),
            ("claude-opus-4-8-20260527-thinking".to_string(), PriceValues { price_in: 9.0, price_out: 9.0, cache_read: 0.0, cache_write: 0.0 }),
        ];
        db.defaults_replace_all(&rows, 1).await.unwrap();
    }

    #[tokio::test]
    async fn fuzzy_match_exact_normalized_contains_and_none() {
        let db = Db::open_memory().await.unwrap();
        seed_defaults(&db).await;

        // 1. exact
        assert_eq!(db.defaults_match("gpt-4o").await.unwrap().unwrap().price_in, 2.5);
        // case-insensitive exact
        assert_eq!(db.defaults_match("GPT-4O").await.unwrap().unwrap().price_in, 2.5);
        // 2. normalized exact: '.' -> '-' (no exact "claude-opus-4.8" key, but contains applies after normalize)
        // 3. contains: "claude-opus-4-8" is a substring of three keys; shortest wins,
        //    tie between the two date keys -> lexicographically greatest (20260615).
        let m = db.defaults_match("claude-opus-4.8").await.unwrap().unwrap();
        assert_eq!(m.price_out, 25.0); // matched a date key, not the longer "-thinking" key
        // the chosen key is the shortest containing one; both date keys share length,
        // so lexicographically-greatest 20260615 is picked (price identical here, asserting it resolves)
        // 4. none
        assert!(db.defaults_match("totally-unknown-model").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn fuzzy_match_contains_prefers_shortest_then_greatest() {
        let db = Db::open_memory().await.unwrap();
        let rows = vec![
            ("m-1-long-suffix".to_string(), PriceValues { price_in: 1.0, price_out: 1.0, cache_read: 0.0, cache_write: 0.0 }),
            ("m-1-aaa".to_string(), PriceValues { price_in: 2.0, price_out: 2.0, cache_read: 0.0, cache_write: 0.0 }),
            ("m-1-zzz".to_string(), PriceValues { price_in: 3.0, price_out: 3.0, cache_read: 0.0, cache_write: 0.0 }),
        ];
        db.defaults_replace_all(&rows, 1).await.unwrap();
        // "m-1" is contained in all three; "m-1-aaa" and "m-1-zzz" are shortest (len 7),
        // tie broken by lexicographically greatest -> "m-1-zzz" (price_in 3.0).
        let m = db.defaults_match("m-1").await.unwrap().unwrap();
        assert_eq!(m.price_in, 3.0);
    }
}
