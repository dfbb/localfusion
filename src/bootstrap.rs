use base64::{engine::general_purpose::STANDARD, Engine};

use crate::crypto::{derive_key, random_salt, sha256_hex};
use crate::db::Db;
use crate::error::FusionError;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn random_token() -> String {
    let mut b = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut b);
    format!("lfadm-{}", STANDARD.encode(b))
}

/// Idempotent: on first run, generates enc_salt + admin token (printed directly, design §3/§9) + default bind; returns enc_key.
pub async fn ensure_initialized(db: &Db) -> Result<[u8; 32], FusionError> {
    // enc_salt
    let salt_b64 = match db.setting_get("enc_salt").await? {
        Some(s) => s,
        None => {
            let salt = random_salt();
            let b64 = STANDARD.encode(salt);
            db.setting_set("enc_salt", &b64).await?;
            b64
        }
    };
    let salt = STANDARD
        .decode(&salt_b64)
        .map_err(|e| FusionError::Internal(format!("salt b64: {e}")))?;
    let enc_key = derive_key(&salt)?;

    // admin token (first run only)
    if db.setting_get("admin_token_hash").await?.is_none() {
        let token = random_token();
        db.setting_set("admin_token_hash", &sha256_hex(&token)).await?;
        crate::logging::print_admin_token_once(&token); // direct println!, not via tracing
    }
    // default bind
    if db.setting_get("inference_bind").await?.is_none() {
        db.setting_set("inference_bind", "127.0.0.1:8787").await?;
    }
    if db.setting_get("admin_bind").await?.is_none() {
        db.setting_set("admin_bind", "127.0.0.1:8788").await?;
    }
    let _ = now_secs(); // reserved
    Ok(enc_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    #[tokio::test]
    async fn first_run_sets_salt_and_token_and_binds() {
        let db = Db::open_memory().await.unwrap();
        let _key = ensure_initialized(&db).await.unwrap();
        assert!(db.setting_get("enc_salt").await.unwrap().is_some());
        assert!(db.setting_get("admin_token_hash").await.unwrap().is_some());
        assert_eq!(
            db.setting_get_or("inference_bind", "").await.unwrap(),
            "127.0.0.1:8787"
        );
        assert_eq!(
            db.setting_get_or("admin_bind", "").await.unwrap(),
            "127.0.0.1:8788"
        );
        // second call does not reset token (idempotent)
        let hash1 = db.setting_get("admin_token_hash").await.unwrap();
        let _ = ensure_initialized(&db).await.unwrap();
        assert_eq!(db.setting_get("admin_token_hash").await.unwrap(), hash1);
    }
}
