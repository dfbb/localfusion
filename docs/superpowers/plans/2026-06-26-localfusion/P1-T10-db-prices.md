# P1-T10 prices 表读

**阶段:** 1 基础层 · **前置:** P1-T05 · 见全局约束: `00-index.md`

**Goal:** 价格表读（生产由第三方写，本层只读 + 测试 seed 用 upsert）（设计 §4）。

**Files:** Modify: `src/db/prices.rs`

**Produces:** `PriceRow{model_id,price_in,price_out,updated_at}`（`FromRow+Serialize`）；`Db::{price_list,price_get,price_upsert}`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    #[tokio::test]
    async fn upsert_and_get() {
        let db = Db::open_memory().await.unwrap();
        assert!(db.price_get("gpt-4o").await.unwrap().is_none());
        db.price_upsert(&PriceRow { model_id: "gpt-4o".into(), price_in: 2.5, price_out: 10.0, updated_at: 100 }).await.unwrap();
        assert_eq!(db.price_get("gpt-4o").await.unwrap().unwrap().price_out, 10.0);
        assert_eq!(db.price_list().await.unwrap().len(), 1);
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib db::prices` → FAIL。

- [ ] **Step 3: 实现**

```rust
use crate::db::Db;
use crate::error::FusionError;

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct PriceRow {
    pub model_id: String,
    pub price_in: f64,
    pub price_out: f64,
    pub updated_at: i64,
}

impl Db {
    pub async fn price_list(&self) -> Result<Vec<PriceRow>, FusionError> {
        Ok(sqlx::query_as::<_, PriceRow>("SELECT * FROM prices ORDER BY model_id").fetch_all(&self.pool).await?)
    }
    pub async fn price_get(&self, model_id: &str) -> Result<Option<PriceRow>, FusionError> {
        Ok(sqlx::query_as::<_, PriceRow>("SELECT * FROM prices WHERE model_id = ?")
            .bind(model_id).fetch_optional(&self.pool).await?)
    }
    pub async fn price_upsert(&self, p: &PriceRow) -> Result<(), FusionError> {
        sqlx::query("INSERT INTO prices(model_id, price_in, price_out, updated_at) VALUES(?,?,?,?)
             ON CONFLICT(model_id) DO UPDATE SET price_in=excluded.price_in,
               price_out=excluded.price_out, updated_at=excluded.updated_at")
            .bind(&p.model_id).bind(p.price_in).bind(p.price_out).bind(p.updated_at)
            .execute(&self.pool).await?;
        Ok(())
    }
}
```

- [ ] **Step 4: 运行确认通过** — Run: `cargo test --lib db::prices` → PASS。

- [ ] **Step 5: 提交**

```bash
git add src/db/prices.rs
git commit -m "feat: prices 表读 + seed upsert"
```
