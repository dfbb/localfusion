# P1-T03 加密 / 哈希 / 派生

**阶段:** 1 基础层 · **前置:** P1-T02 · 见全局约束: `00-index.md`

**Goal:** ChaCha20-Poly1305 加解密 + machine-id HKDF 派生 + SHA-256 哈希（设计 §5.1）。

**Files:** Modify: `src/crypto.rs`

**Produces:** `derive_key(salt)->Result<[u8;32]>`、`encrypt(key,pt)->Result<String>`、`decrypt(key,b64)->Result<String>`、`random_salt()->[u8;16]`、`sha256_hex(s)->String`。

- [ ] **Step 1: 写失败测试**

```rust
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
        assert_eq!(sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
    #[test]
    fn derive_key_deterministic_for_same_salt() {
        let salt = [3u8; 16];
        assert_eq!(derive_key(&salt).unwrap(), derive_key(&salt).unwrap());
    }
}
```

- [ ] **Step 2: 运行确认失败** — Run: `cargo test --lib crypto` → FAIL。

- [ ] **Step 3: 实现（哈希/salt/派生）**

```rust
use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};

use crate::error::FusionError;

fn machine_id() -> Result<String, FusionError> {
    machine_uid::get().map_err(|e| FusionError::Internal(format!("machine-id: {e}")))
}

pub fn random_salt() -> [u8; 16] {
    let mut s = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut s);
    s
}

pub fn sha256_hex(input: &str) -> String {
    let mut h = Sha256::new();
    h.update(input.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn derive_key(salt: &[u8]) -> Result<[u8; 32], FusionError> {
    let ikm = machine_id()?;
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(b"localfusion-provider-key", &mut okm)
        .map_err(|e| FusionError::Internal(format!("hkdf: {e}")))?;
    Ok(okm)
}
```

- [ ] **Step 4: 实现（encrypt/decrypt）**

```rust
pub fn encrypt(key: &[u8; 32], plaintext: &str) -> Result<String, FusionError> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce_bytes = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
    let ct = cipher.encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
        .map_err(|e| FusionError::Internal(format!("encrypt: {e}")))?;
    let mut blob = Vec::with_capacity(12 + ct.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ct);
    Ok(STANDARD.encode(blob))
}

pub fn decrypt(key: &[u8; 32], b64: &str) -> Result<String, FusionError> {
    let blob = STANDARD.decode(b64).map_err(|e| FusionError::Internal(format!("decrypt b64: {e}")))?;
    if blob.len() < 12 {
        return Err(FusionError::Internal("decrypt: blob too short".into()));
    }
    let (nonce_bytes, ct) = blob.split_at(12);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let pt = cipher.decrypt(Nonce::from_slice(nonce_bytes), ct)
        .map_err(|_| FusionError::Internal("decrypt: auth failed".into()))?;
    String::from_utf8(pt).map_err(|e| FusionError::Internal(format!("decrypt utf8: {e}")))
}
```

- [ ] **Step 5: 运行确认通过** — Run: `cargo test --lib crypto` → PASS（5 个）。

- [ ] **Step 6: clippy + 提交**

```bash
cargo clippy --lib
git add src/crypto.rs
git commit -m "feat: ChaCha20-Poly1305 加密 + machine-id 派生 + sha256 哈希"
```
