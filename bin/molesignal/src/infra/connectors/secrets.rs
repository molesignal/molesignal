// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A connector's secrets at rest: sealed with the root key (`MS_CIPHER_KEY`, AES-256-GCM) apart
//! from the rest of its settings, the same envelope `notify_connectors` uses.
//!
//! `config_json` keeps everything but the keys of [`config::SECRET_KEYS`]; those go into one
//! sealed JSON object. A push connector is found by its token, so the token's SHA-256 is kept
//! beside it for the lookup. Rotating the root key re-seals the column with the rest
//! (`molesignal rewrap-kek`, see `cipher::kek_rewrap`).

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::config;
use crate::{
    infra::cipher::CipherRootKey,
    shared::{Error, Result},
};

/// A connector's settings as they are stored.
#[derive(Debug, Default)]
pub struct Sealed {
    /// The settings that are not secret.
    pub config_json: Value,
    /// `(nonce, ciphertext)` of the secrets; `None` when it holds none.
    pub envelope: Option<(Vec<u8>, Vec<u8>)>,
    pub push_token_sha256: Option<String>,
}

pub fn seal(cipher: &CipherRootKey, config: &Value) -> Result<Sealed> {
    let mut plain = config.clone();
    let mut secrets = Map::new();

    if let Value::Object(fields) = &mut plain {
        for key in config::SECRET_KEYS {
            if let Some(value) = fields.remove(*key) {
                secrets.insert((*key).to_string(), value);
            }
        }
    }

    let push_token_sha256 = secrets
        .get("pushToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(push_token_digest);

    let envelope = if secrets.is_empty() {
        None
    } else {
        let bytes = serde_json::to_vec(&secrets)
            .map_err(|error| Error::internal(format!("connector secrets: {error}")))?;

        Some(
            cipher
                .seal(&bytes)
                .map_err(|error| Error::internal(format!("connector secrets seal: {error}")))?,
        )
    };

    Ok(Sealed {
        config_json: plain,
        envelope,
        push_token_sha256,
    })
}

/// The whole configuration again: `config_json` with the sealed secrets put back.
///
/// A secret that cannot be opened is an error, not an empty field: a connector shown without its
/// secrets would lose them for good on its next save.
pub fn open(
    cipher: &CipherRootKey,
    mut config_json: Value,
    envelope: Option<(&[u8], &[u8])>,
) -> Result<Value> {
    let Some((nonce, ciphertext)) = envelope else {
        return Ok(config_json);
    };

    let bytes = cipher.open(nonce, ciphertext).map_err(|_| {
        Error::internal("connector secrets could not be decrypted with the root key")
    })?;

    let secrets: Map<String, Value> = serde_json::from_slice(&bytes)
        .map_err(|_| Error::internal("connector secrets are not a JSON object"))?;

    match &mut config_json {
        Value::Object(fields) => fields.extend(secrets),
        other => *other = Value::Object(secrets),
    }

    Ok(config_json)
}

/// What a push token is looked up by.
pub fn push_token_digest(token: &str) -> String {
    hex::encode(&Sha256::digest(token.trim().as_bytes())[..])
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use serde_json::json;

    use super::*;

    fn cipher(byte: u8) -> CipherRootKey {
        CipherRootKey::from_base64(&base64::engine::general_purpose::STANDARD.encode([byte; 32]))
            .unwrap()
    }

    #[test]
    fn secrets_leave_the_plain_settings_and_come_back_whole() {
        let config = json!({
            "bucket": "logs",
            "auth": "access_key",
            "accessKey": "AKIAEXAMPLE",
            "secretKey": "s3cr3t",
        });
        let sealed = seal(&cipher(1), &config).unwrap();

        assert_eq!(
            sealed.config_json,
            json!({ "bucket": "logs", "auth": "access_key" })
        );

        let (nonce, ciphertext) = sealed.envelope.as_ref().unwrap();
        let text = String::from_utf8_lossy(ciphertext);
        assert!(!text.contains("s3cr3t"), "sealed, not stored as it was");

        let opened = open(
            &cipher(1),
            sealed.config_json.clone(),
            Some((nonce.as_slice(), ciphertext.as_slice())),
        )
        .unwrap();
        assert_eq!(opened, config);
    }

    #[test]
    fn settings_without_secrets_have_no_envelope() {
        let sealed = seal(&cipher(1), &json!({ "brokers": "b:9092", "topic": "t" })).unwrap();

        assert!(sealed.envelope.is_none());
        assert!(sealed.push_token_sha256.is_none());
        assert_eq!(
            open(&cipher(1), sealed.config_json, None).unwrap(),
            json!({ "brokers": "b:9092", "topic": "t" })
        );
    }

    #[test]
    fn a_push_token_is_found_by_its_digest() {
        let sealed = seal(
            &cipher(1),
            &json!({ "pushToken": "0123456789abcdef", "targetStream": "cf" }),
        )
        .unwrap();

        assert_eq!(
            sealed.push_token_sha256.as_deref(),
            Some(push_token_digest("0123456789abcdef").as_str())
        );
        assert_eq!(
            push_token_digest(" 0123456789abcdef "),
            push_token_digest("0123456789abcdef")
        );
        assert_eq!(push_token_digest("x").len(), 64);
    }

    #[test]
    fn another_root_key_cannot_open_them() {
        let sealed = seal(&cipher(1), &json!({ "saslPassword": "pw" })).unwrap();
        let (nonce, ciphertext) = sealed.envelope.unwrap();

        assert!(
            open(
                &cipher(2),
                json!({}),
                Some((nonce.as_slice(), ciphertext.as_slice()))
            )
            .is_err()
        );
    }
}
