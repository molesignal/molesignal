// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cipher root key —— envelope KEK 包 user-level cipher_keys 行（spec auth-hardening）。
//!
//! 来源：`MS_CIPHER_KEY` 环境变量，base64-标准编码 32 字节。缺失或长度不
//! 对都返 `CipherRootKeyError`，wire 自决定 fail-fast 或 dev fallback。
//!
//! 命名说明：项目内已有 user-level `CipherKey`（cipher_keys 表行，DEK 语义），
//! 故 root-key 命名为 `CipherRootKey` 以明示双层 envelope。

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CipherRootKeyError {
    #[error("cipher root key not configured: set MS_CIPHER_KEY (base64 32B)")]
    Missing,
    #[error("cipher root key length mismatch: expected 32 bytes after base64, got {0}")]
    BadLen(usize),
    #[error("cipher root key base64 decode failed: {0}")]
    BadEncoding(String),
    #[error("aes-gcm operation failed: {0}")]
    Aead(String),
}

#[derive(Clone)]
pub struct CipherRootKey {
    cipher: Aes256Gcm,
    hmac_key: [u8; 32],
}

impl CipherRootKey {
    /// 从 env 加载 `MS_CIPHER_KEY`。
    pub fn from_env() -> Result<Self, CipherRootKeyError> {
        let v = std::env::var("MS_CIPHER_KEY").map_err(|_| CipherRootKeyError::Missing)?;
        Self::from_base64(&v)
    }

    pub fn from_base64(b64: &str) -> Result<Self, CipherRootKeyError> {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| CipherRootKeyError::BadEncoding(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(CipherRootKeyError::BadLen(bytes.len()));
        }
        let hmac_key: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| CipherRootKeyError::BadLen(bytes.len()))?;
        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&hmac_key);
        Ok(Self {
            cipher: Aes256Gcm::new(key),
            hmac_key,
        })
    }

    /// AES-256-GCM 加密。返回 `(nonce, ciphertext_with_tag)`。
    pub fn seal(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), CipherRootKeyError> {
        use rand::TryRng as _;
        let mut nonce_bytes = [0u8; 12];
        rand::rngs::SysRng
            .try_fill_bytes(&mut nonce_bytes)
            .map_err(|e| CipherRootKeyError::Aead(format!("rng: {e}")))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| CipherRootKeyError::Aead(e.to_string()))?;
        Ok((nonce_bytes.to_vec(), ct))
    }

    pub fn open(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, CipherRootKeyError> {
        if nonce.len() != 12 {
            return Err(CipherRootKeyError::Aead(format!(
                "nonce must be 12 bytes, got {}",
                nonce.len()
            )));
        }
        let nonce = Nonce::from_slice(nonce);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| CipherRootKeyError::Aead(e.to_string()))
    }

    /// 生成组织隔离的确定性 HMAC-SHA-256；根密钥不会离开后端进程。
    pub fn org_hmac_sha256(&self, org_id: &str, value: &[u8]) -> String {
        use aws_lc_rs::hmac;

        let root = hmac::Key::new(hmac::HMAC_SHA256, &self.hmac_key);
        let mut context = b"molesignal/field-masking/v1\0".to_vec();
        context.extend_from_slice(org_id.as_bytes());
        let derived = hmac::sign(&root, &context);
        let organization_key = hmac::Key::new(hmac::HMAC_SHA256, derived.as_ref());
        hex::encode(hmac::sign(&organization_key, value).as_ref())
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::*;

    fn random_b64_key() -> String {
        let bytes = [7u8; 32];
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn seal_open_roundtrip() {
        let k = CipherRootKey::from_base64(&random_b64_key()).unwrap();
        let (n, ct) = k.seal(b"hello").unwrap();
        let pt = k.open(&n, &ct).unwrap();
        assert_eq!(pt, b"hello");
    }

    #[test]
    fn bad_len_rejected() {
        let b64 = base64::engine::general_purpose::STANDARD.encode([1u8; 16]);
        let r = CipherRootKey::from_base64(&b64);
        assert!(matches!(r, Err(CipherRootKeyError::BadLen(16))));
    }

    #[test]
    fn org_hmac_is_stable_and_tenant_scoped() {
        let key = CipherRootKey::from_base64(&random_b64_key()).unwrap();
        let first = key.org_hmac_sha256("org-a", b"alice@example.com");
        assert_eq!(first.len(), 64);
        assert_eq!(first, key.org_hmac_sha256("org-a", b"alice@example.com"));
        assert_ne!(first, key.org_hmac_sha256("org-b", b"alice@example.com"));
    }
}
