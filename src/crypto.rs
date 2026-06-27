//! Encryption / hashing / key derivation module
//!
//! - Cipher: ChaCha20-Poly1305
//! - Key derivation: HKDF-SHA256(ikm=machine-id, salt=enc_salt)
//! - Ciphertext format: base64(nonce[12] || ct || tag)
//! - Hash: SHA-256 (used for ingress key / admin token storage)
//!
//! **Security contract**: plaintext keys must never appear in logs; error messages must not leak key material.

use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};

use crate::error::FusionError;

/// Get the current machine ID (used as the IKM for HKDF).
fn machine_id() -> Result<String, FusionError> {
    machine_uid::get().map_err(|e| FusionError::Internal(format!("machine-id: {e}")))
}

/// Generate a 16-byte random salt for use with derive_key.
pub fn random_salt() -> [u8; 16] {
    let mut s = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut s);
    s
}

/// Compute the SHA-256 hex digest of the input string.
/// Used to store hashes of ingress keys and admin tokens.
pub fn sha256_hex(input: &str) -> String {
    let mut h = Sha256::new();
    h.update(input.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Derive a 32-byte encryption key from the machine ID via HKDF-SHA256.
///
/// `salt`: stored in the database enc_salt column, unique per provider.
pub fn derive_key(salt: &[u8]) -> Result<[u8; 32], FusionError> {
    let ikm = machine_id()?;
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(b"localfusion-provider-key", &mut okm)
        .map_err(|e| FusionError::Internal(format!("hkdf: {e}")))?;
    Ok(okm)
}

/// Encrypt a plaintext string using ChaCha20-Poly1305.
///
/// Returns: base64(nonce[12] || ciphertext || tag)
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

/// Decrypt a ciphertext in base64(nonce[12] || ciphertext || tag) format.
///
/// Returns a generic error on authentication failure to avoid leaking details.
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
