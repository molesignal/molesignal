// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 字段级静态加密的载荷编解码 + per-org DEK 句柄。
//!
//! schema 标 `encrypted` 的字段在 ingest 用 **org 的字段 DEK**（data encryption key）加密，
//! 密文复用 [`super::super::payload`] 的自描述格式
//! `kid:<key_id>:v<version>:<base64(nonce||ct)>`
//! （与 VRL `encrypt(value, key_id)` 同格式 → `decrypt(col)` 对两者都管用）。DEK 由 KEK
//! 信封包存于 `cipher_keys` 表、按 org 隔离、可多版本轮换；解析 / 缓存见
//! [`super::service::FieldKeyService`]。
//!
//! 查询端 `decrypt(col)` UDF 在执行期预载该 org 的全部 DEK（id→raw），逐值还原；非 `kid:`
//! 前缀的值（明文 / 其它）原样透传，使 `decrypt(col)` 可安全套于混合列、联邦二次求值幂等。

use std::collections::HashMap;

use super::super::payload::{decrypt_with_raw, encrypt_with_raw, payload_decode};
use crate::shared::Result;

/// 解析出的 org 字段 DEK 句柄（明文 raw key，仅驻内存、不出库）。
#[derive(Debug, Clone)]
pub struct OrgFieldKey {
    /// `cipher_keys` 行 id（= 载荷里的 `key_id`）。
    pub key_id: String,
    pub version: i32,
    /// KEK 解包后的 32B AES-256 raw key。
    pub raw_key: Vec<u8>,
}

/// 用 DEK 加密字段明文 → `kid:<key_id>:v<version>:<base64(nonce||ct)>`。
pub fn encrypt_field(key: &OrgFieldKey, plaintext: &str) -> Result<String> {
    encrypt_with_raw(&key.key_id, key.version, &key.raw_key, plaintext.as_bytes())
}

/// 解密一个存储值：
/// - `kid:` 载荷且 `keys` 含其 `key_id` 的 raw DEK 且解成功 → `Some(明文)`；
/// - `kid:` 但 key 缺失 / 解失败 → `None`（查询端落 NULL）；
/// - 非 `kid:`（未加密 / 其它格式）→ `Some(原值)` 原样透传。
///
/// `keys`：`key_id`(cipher_keys 行 id) → raw DEK，由 `FieldKeyService::decrypt_map` 预载，
/// 含历史版本，故能解轮换前写入的旧密文。
pub fn decrypt_field(keys: &HashMap<String, Vec<u8>>, stored: &str) -> Option<String> {
    if !stored.starts_with("kid:") {
        return Some(stored.to_string());
    }
    let payload = payload_decode(stored).ok()?;
    let raw = keys.get(&payload.key_id)?;
    let pt = decrypt_with_raw(&payload, raw).ok()?;
    String::from_utf8(pt).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dek(id: &str, version: i32, byte: u8) -> OrgFieldKey {
        OrgFieldKey {
            key_id: id.to_string(),
            version,
            raw_key: vec![byte; 32],
        }
    }

    fn map(entries: &[(&str, u8)]) -> HashMap<String, Vec<u8>> {
        entries
            .iter()
            .map(|(id, b)| (id.to_string(), vec![*b; 32]))
            .collect()
    }

    #[test]
    fn encrypt_then_decrypt_roundtrips() {
        let k = dek("key-a", 1, 5);
        let ct = encrypt_field(&k, "alice@example.com").unwrap();
        assert!(
            ct.starts_with("kid:key-a:v1:"),
            "self-describing payload, got {ct}"
        );
        let keys = map(&[("key-a", 5)]);
        assert_eq!(
            decrypt_field(&keys, &ct).as_deref(),
            Some("alice@example.com")
        );
    }

    #[test]
    fn decrypt_reads_old_version_from_key_map() {
        // 轮换后 map 含新旧两个 id；旧密文仍可解。
        let old = dek("key-v1", 1, 1);
        let ct_old = encrypt_field(&old, "secret-old").unwrap();
        let keys = map(&[("key-v1", 1), ("key-v2", 2)]);
        assert_eq!(decrypt_field(&keys, &ct_old).as_deref(), Some("secret-old"));
    }

    #[test]
    fn unmarked_value_passes_through() {
        let keys = map(&[("key-a", 5)]);
        assert_eq!(
            decrypt_field(&keys, "plain text").as_deref(),
            Some("plain text")
        );
    }

    #[test]
    fn missing_key_id_yields_none() {
        let k = dek("key-a", 1, 5);
        let ct = encrypt_field(&k, "x").unwrap();
        let keys = map(&[("other-key", 9)]);
        assert_eq!(decrypt_field(&keys, &ct), None);
    }

    #[test]
    fn wrong_raw_key_yields_none() {
        let k = dek("key-a", 1, 5);
        let ct = encrypt_field(&k, "x").unwrap();
        let keys = map(&[("key-a", 9)]); // 同 id 但 raw 不对 → GCM tag fail
        assert_eq!(decrypt_field(&keys, &ct), None);
    }
}
