// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//!  license verification（迁自原 infra license 模块）。
//!
//! 实现 [`crate::shared::LicenseGate`]，付费版 wire 用 `Arc<SignedLicense>` 替换
//! 默认的 `CommunityLicense`。
//!
//! License 文件格式：
//! ```json
//! {
//!   "payload_b64": "<base64 of LicensePayload JSON>",
//!   "signature_b64": "<base64 of Ed25519 signature over payload_b64 bytes>"
//! }
//! ```
//!
//! 公钥 (`pubkey: &[u8; 32]`) 由 wire 注入；正式发布替换为 issuer 签发的对应 root key。

use std::{
    collections::HashSet,
    sync::atomic::{AtomicI64, Ordering},
};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::shared::{Error, LicenseGate, Result};

/// 开发用 demo Ed25519 公钥占位（32 字节）。生产发布换为正式 issuer 公钥。
pub const DEFAULT_ROOT_PUBKEY: [u8; 32] = [0u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePayload {
    pub issued_to: String,
    #[serde(default)]
    pub expires_at_micros: i64,
    #[serde(default)]
    pub max_ingest_bytes_per_day: i64,
    #[serde(default)]
    pub max_users: i32,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseFile {
    pub payload_b64: String,
    pub signature_b64: String,
}

pub struct SignedLicense {
    payload: LicensePayload,
    feature_set: HashSet<String>,
    ingest_today: AtomicI64,
}

impl SignedLicense {
    /// 从 license file path 加载并验签。
    pub fn load(path: &str, pubkey: &[u8; 32]) -> Result<Self> {
        let file = load_file(path)?;
        Self::verify(&file, pubkey)
    }

    /// 仅验签 + 返 SignedLicense（便于单测注入）。
    pub fn verify(file: &LicenseFile, pubkey: &[u8; 32]) -> Result<Self> {
        use base64::Engine as _;
        let key = VerifyingKey::from_bytes(pubkey)
            .map_err(|e| Error::invalid(format!("bad license pubkey: {e}")))?;
        let payload_bytes = base64::engine::general_purpose::STANDARD
            .decode(&file.payload_b64)
            .map_err(|e| Error::invalid(format!("license payload b64: {e}")))?;
        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(&file.signature_b64)
            .map_err(|e| Error::invalid(format!("license signature b64: {e}")))?;
        let sig = Signature::from_slice(&sig_bytes)
            .map_err(|e| Error::unauthorized(format!("license signature shape: {e}")))?;
        key.verify(file.payload_b64.as_bytes(), &sig)
            .map_err(|e| Error::unauthorized(format!("license signature: {e}")))?;
        let payload: LicensePayload = serde_json::from_slice(&payload_bytes)
            .map_err(|e| Error::invalid(format!("license payload json: {e}")))?;
        let feature_set = payload.features.iter().cloned().collect();
        Ok(Self {
            payload,
            feature_set,
            ingest_today: AtomicI64::new(0),
        })
    }

    pub fn payload(&self) -> &LicensePayload {
        &self.payload
    }

    pub fn verify_active(file: &LicenseFile, pubkey: &[u8; 32], now_micros: i64) -> Result<Self> {
        let license = Self::verify(file, pubkey)?;
        if license.expired(now_micros) {
            return Err(Error::invalid("License version is expired"));
        }
        Ok(license)
    }
}

pub fn load_file(path: &str) -> Result<LicenseFile> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| Error::internal(format!("read license {path}: {error}")))?;
    serde_json::from_str(&raw).map_err(|error| Error::invalid(format!("license json: {error}")))
}

pub fn load_file_from_env() -> Result<Option<LicenseFile>> {
    let Some(path) = std::env::var("MS_LICENSE_FILE")
        .ok()
        .filter(|path| !path.trim().is_empty())
    else {
        return Ok(None);
    };
    load_file(&path).map(Some)
}

impl LicenseGate for SignedLicense {
    fn has_feature(&self, name: &str) -> bool {
        if self.feature_set.is_empty() {
            return false;
        }
        self.feature_set.contains(name)
    }
    fn add_ingest_bytes(&self, n: u64) -> bool {
        if self.payload.max_ingest_bytes_per_day <= 0 {
            return true;
        }
        let new = self.ingest_today.fetch_add(n as i64, Ordering::Relaxed) + n as i64;
        new <= self.payload.max_ingest_bytes_per_day
    }
    fn expired(&self, now_micros: i64) -> bool {
        self.payload.expires_at_micros > 0 && now_micros >= self.payload.expires_at_micros
    }
    fn issued_to(&self) -> &str {
        &self.payload.issued_to
    }
    fn reset_daily(&self) {
        self.ingest_today.store(0, Ordering::Relaxed);
    }
    fn features(&self) -> Vec<String> {
        self.payload.features.clone()
    }
    fn edition(&self) -> &'static str {
        ""
    }
    fn verified(&self) -> bool {
        true
    }
    fn max_ingest_bytes_per_day(&self) -> Option<u64> {
        if self.payload.max_ingest_bytes_per_day > 0 {
            Some(self.payload.max_ingest_bytes_per_day as u64)
        } else {
            None
        }
    }
    fn expires_at_micros(&self) -> Option<i64> {
        if self.payload.expires_at_micros > 0 {
            Some(self.payload.expires_at_micros)
        } else {
            None
        }
    }
}

/// 试图从环境变量 `MS_LICENSE_FILE` 加载；失败或缺失则返 None，调用方
/// 用 `CommunityLicense` 兜底。
pub fn load_from_env(pubkey: &[u8; 32]) -> Option<SignedLicense> {
    let path = std::env::var("MS_LICENSE_FILE").ok()?;
    if path.trim().is_empty() {
        return None;
    }
    match SignedLicense::load(&path, pubkey) {
        Ok(l) => {
            tracing::info!(issued_to = %l.issued_to(), " license loaded");
            Some(l)
        }
        Err(e) => {
            tracing::warn!(error = %e, path = %path, "license load failed; community fallback");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn signed_file(expires_at_micros: i64) -> (LicenseFile, [u8; 32]) {
        let signing_key = SigningKey::from_bytes(&[0x5a; 32]);
        let payload = LicensePayload {
            issued_to: "fixture".into(),
            expires_at_micros,
            max_ingest_bytes_per_day: 1_000,
            max_users: 10,
            features: vec!["sso".into()],
        };
        let payload_b64 = base64::engine::general_purpose::STANDARD
            .encode(serde_json::to_vec(&payload).expect("encode License payload"));
        let signature = signing_key.sign(payload_b64.as_bytes());
        (
            LicenseFile {
                payload_b64,
                signature_b64: base64::engine::general_purpose::STANDARD
                    .encode(signature.to_bytes()),
            },
            signing_key.verifying_key().to_bytes(),
        )
    }

    #[test]
    fn valid_signature_is_accepted_and_tampering_is_rejected() {
        let (file, public_key) = signed_file(10_000);
        let verified =
            SignedLicense::verify_active(&file, &public_key, 1_000).expect("valid active License");
        assert_eq!(verified.issued_to(), "fixture");
        assert!(verified.has_feature("sso"));

        let mut tampered = file;
        tampered.payload_b64.push('A');
        assert!(SignedLicense::verify_active(&tampered, &public_key, 1_000).is_err());
    }

    #[test]
    fn expired_signed_version_is_rejected() {
        let (file, public_key) = signed_file(1_000);
        assert!(SignedLicense::verify_active(&file, &public_key, 999).is_ok());
        assert!(SignedLicense::verify_active(&file, &public_key, 1_000).is_err());
    }

    #[test]
    fn daily_cap_blocks_when_exceeded() {
        let payload = LicensePayload {
            issued_to: "a".into(),
            expires_at_micros: 0,
            max_ingest_bytes_per_day: 100,
            max_users: 0,
            features: vec!["sso".into()],
        };
        let l = SignedLicense {
            payload,
            feature_set: ["sso".to_string()].into_iter().collect(),
            ingest_today: AtomicI64::new(0),
        };
        assert!(l.has_feature("sso"));
        assert!(!l.has_feature("federated_search"));
        assert!(l.add_ingest_bytes(50));
        assert!(l.add_ingest_bytes(50));
        assert!(!l.add_ingest_bytes(1));
        l.reset_daily();
        assert!(l.add_ingest_bytes(50));
    }

    #[test]
    fn expired_after_deadline() {
        let payload = LicensePayload {
            issued_to: "a".into(),
            expires_at_micros: 1000,
            max_ingest_bytes_per_day: 0,
            max_users: 0,
            features: vec![],
        };
        let l = SignedLicense {
            payload,
            feature_set: HashSet::new(),
            ingest_today: AtomicI64::new(0),
        };
        assert!(!l.expired(500));
        assert!(l.expired(1000));
        assert!(l.expired(2000));
    }
}
