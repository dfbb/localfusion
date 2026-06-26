use crate::db::Db;

/// 模型解析器占位；P3-T08 完整实现
pub struct ModelResolver {
    #[allow(dead_code)]
    db: Db,
    #[allow(dead_code)]
    enc_key: [u8; 32],
}

impl ModelResolver {
    pub fn new(db: Db, enc_key: [u8; 32]) -> Self {
        ModelResolver { db, enc_key }
    }
}
