//! 加密 / 哈希 / 密钥派生模块
//!
//! - 加密算法：ChaCha20-Poly1305
//! - 密钥派生：HKDF-SHA256(ikm=machine-id, salt=enc_salt)
//! - 密文格式：base64(nonce[12] || ct || tag)
//! - 哈希：SHA-256（用于 ingress key / admin token 存储）
//!
//! **安全约定**：日志中绝不出现明文密钥；错误信息不泄露密钥材料。

use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};

use crate::error::FusionError;

/// 获取当前机器 ID（作为 HKDF 的 IKM）。
fn machine_id() -> Result<String, FusionError> {
    machine_uid::get().map_err(|e| FusionError::Internal(format!("machine-id: {e}")))
}

/// 生成 16 字节随机 salt，用于 derive_key。
pub fn random_salt() -> [u8; 16] {
    let mut s = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut s);
    s
}

/// 计算输入字符串的 SHA-256 十六进制摘要。
/// 用于存储 ingress key 和 admin token 的哈希。
pub fn sha256_hex(input: &str) -> String {
    let mut h = Sha256::new();
    h.update(input.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 通过 HKDF-SHA256 从 machine-id 派生 32 字节加密密钥。
///
/// `salt`：存储在数据库 enc_salt 列，每个 provider 独立。
pub fn derive_key(salt: &[u8]) -> Result<[u8; 32], FusionError> {
    let ikm = machine_id()?;
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(b"localfusion-provider-key", &mut okm)
        .map_err(|e| FusionError::Internal(format!("hkdf: {e}")))?;
    Ok(okm)
}

/// 使用 ChaCha20-Poly1305 加密明文字符串。
///
/// 返回：base64(nonce[12] || ciphertext || tag)
pub fn encrypt(key: &[u8; 32], plaintext: &str) -> Result<String, FusionError> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce_bytes = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
        .map_err(|e| FusionError::Internal(format!("encrypt: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ct);
    Ok(STANDARD.encode(blob))
}

/// 解密 base64(nonce[12] || ciphertext || tag) 格式的密文。
///
/// 认证失败时返回通用错误，不泄露具体原因。
pub fn decrypt(key: &[u8; 32], b64: &str) -> Result<String, FusionError> {
    let blob = STANDARD
        .decode(b64)
        .map_err(|e| FusionError::Internal(format!("decrypt b64: {e}")))?;
    if blob.len() < 12 {
        return Err(FusionError::Internal("decrypt: blob too short".into()));
    }
    let (nonce_bytes, ct) = blob.split_at(12);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ct)
        .map_err(|_| FusionError::Internal("decrypt: auth failed".into()))?;
    String::from_utf8(pt).map_err(|e| FusionError::Internal(format!("decrypt utf8: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = [7u8; 32];
        let ct = encrypt(&key, "sk-secret-123").unwrap();
        assert_ne!(ct, "sk-secret-123");
        assert_eq!(decrypt(&key, &ct).unwrap(), "sk-secret-123");
    }

    #[test]
    fn nonce_is_random_per_call() {
        let key = [9u8; 32];
        assert_ne!(encrypt(&key, "x").unwrap(), encrypt(&key, "x").unwrap());
    }

    #[test]
    fn wrong_key_fails() {
        let ct = encrypt(&[1u8; 32], "secret").unwrap();
        assert!(decrypt(&[2u8; 32], &ct).is_err());
    }

    #[test]
    fn sha256_hex_known_vector() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn derive_key_deterministic_for_same_salt() {
        let salt = [3u8; 16];
        assert_eq!(derive_key(&salt).unwrap(), derive_key(&salt).unwrap());
    }
}
