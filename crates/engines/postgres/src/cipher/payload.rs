// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cipher payload 自描述编解码。
//!
//! 格式：`kid:<key_id>:v<version>:<base64(nonce || ciphertext_with_tag)>`
//!
//! VRL 内置 `encrypt(value, key_id)` 用这个编码 → 列里只存这条字符串；
//! `decrypt(value)` 反编码后查 cipher_keys 表拿 raw_key → AES-GCM open。

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};

use crate::shared::{Error, Result};

pub struct CipherPayload {
    pub key_id: String,
    pub version: i32,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

pub fn payload_encode(key_id: &str, version: i32, nonce: &[u8], ciphertext: &[u8]) -> String {
    use base64::Engine as _;
    let mut buf = Vec::with_capacity(nonce.len() + ciphertext.len());
    buf.extend_from_slice(nonce);
    buf.extend_from_slice(ciphertext);
    format!(
        "kid:{}:v{}:{}",
        key_id,
        version,
        base64::engine::general_purpose::STANDARD.encode(&buf)
    )
}

pub fn payload_decode(s: &str) -> Result<CipherPayload> {
    use base64::Engine as _;
    let parts: Vec<&str> = s.splitn(4, ':').collect();
    if parts.len() != 4 || parts[0] != "kid" || !parts[2].starts_with('v') {
        return Err(Error::invalid("cipher payload: bad prefix"));
    }
    let key_id = parts[1].to_string();
    let version: i32 = parts[2][1..]
        .parse()
        .map_err(|e| Error::invalid(format!("cipher payload version: {e}")))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(parts[3])
        .map_err(|e| Error::invalid(format!("cipher payload b64: {e}")))?;
    if bytes.len() < 12 + 16 {
        return Err(Error::invalid(format!(
            "cipher payload too short: {}",
            bytes.len()
        )));
    }
    let (nonce, ciphertext) = bytes.split_at(12);
    Ok(CipherPayload {
        key_id,
        version,
        nonce: nonce.to_vec(),
        ciphertext: ciphertext.to_vec(),
    })
}

/// VRL `encrypt(value, key)` 的执行函数：raw_key (32B) + plaintext → payload string。
pub fn encrypt_with_raw(
    key_id: &str,
    version: i32,
    raw_key: &[u8],
    plaintext: &[u8],
) -> Result<String> {
    use rand::TryRng as _;
    if raw_key.len() != 32 {
        return Err(Error::invalid("raw key must be 32 bytes"));
    }
    let cipher = Aes256Gcm::new(aes_gcm::Key::<Aes256Gcm>::from_slice(raw_key));
    let mut nonce_bytes = [0u8; 12];
    rand::rngs::SysRng
        .try_fill_bytes(&mut nonce_bytes)
        .map_err(|e| Error::internal(format!("rng: {e}")))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| Error::internal(format!("encrypt: {e}")))?;
    Ok(payload_encode(key_id, version, &nonce_bytes, &ct))
}

pub fn decrypt_with_raw(payload: &CipherPayload, raw_key: &[u8]) -> Result<Vec<u8>> {
    if raw_key.len() != 32 {
        return Err(Error::invalid("raw key must be 32 bytes"));
    }
    let cipher = Aes256Gcm::new(aes_gcm::Key::<Aes256Gcm>::from_slice(raw_key));
    let nonce = Nonce::from_slice(&payload.nonce);
    cipher
        .decrypt(nonce, payload.ciphertext.as_ref())
        .map_err(|e| Error::Unauthorized(format!("decrypt: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let raw_key = [3u8; 32];
        let s = encrypt_with_raw("kid-1", 2, &raw_key, b"secret").unwrap();
        let p = payload_decode(&s).unwrap();
        assert_eq!(p.key_id, "kid-1");
        assert_eq!(p.version, 2);
        let pt = decrypt_with_raw(&p, &raw_key).unwrap();
        assert_eq!(pt, b"secret");
    }

    #[test]
    fn bad_prefix_rejected() {
        assert!(payload_decode("not-a-payload").is_err());
        assert!(payload_decode("kid:abc:vBAD:xx").is_err());
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let raw_key = [3u8; 32];
        let s = encrypt_with_raw("k", 1, &raw_key, b"hi").unwrap();
        let p = payload_decode(&s).unwrap();
        assert!(decrypt_with_raw(&p, &[9u8; 32]).is_err());
    }
}
