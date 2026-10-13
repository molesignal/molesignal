// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a connector's `config_json` may hold, and how its secrets are handled at the API edge.
//!
//! The keys are the ones the connectors read: [`super::dispatcher`] for the S3 and Kafka sinks,
//! [`super::runner`] for CloudWatch Logs, and the push receivers for `push_token`.
//!
//! Secrets never leave through the API: a read replaces each with [`SECRET_MASK`]. An update that
//! sends the mask back means "leave it as it is", so a connector can be renamed, switched off or
//! pointed elsewhere without anyone typing its credentials again.

use serde_json::Value;

use super::ConnectorKind;
use crate::shared::{Error, Result};

/// What a secret reads as in an API response.
pub const SECRET_MASK: &str = "***";

/// Keys of `config_json` whose values are secrets.
const SECRET_KEYS: &[&str] = &[
    "access_key",
    "secret_key",
    "session_token",
    "push_token",
    "webhook_url",
    "client_secret",
    "password",
    "sasl_password",
];

/// A push token shorter than this is too easy to guess to stand in for a credential.
const MIN_PUSH_TOKEN_LEN: usize = 16;

/// `config` as an API response shows it: every secret replaced by the mask.
pub fn mask_secrets(mut config: Value) -> Value {
    if let Value::Object(fields) = &mut config {
        for key in SECRET_KEYS {
            if fields.contains_key(*key) {
                fields.insert((*key).to_string(), Value::String(SECRET_MASK.to_string()));
            }
        }
    }

    config
}

/// The secrets `config` holds a value for, by key.
pub fn stored_secret_keys(config: &Value) -> Vec<String> {
    SECRET_KEYS
        .iter()
        .filter(|key| !text(config, key).is_empty())
        .map(|key| (*key).to_string())
        .collect()
}

/// `incoming` with each masked secret replaced by the one in `stored`.
///
/// A masked secret with nothing stored behind it (a new connector, or one whose kind changed) is
/// dropped: the mask itself is never saved as a credential.
pub fn keep_stored_secrets(mut incoming: Value, stored: Option<&Value>) -> Value {
    let Value::Object(fields) = &mut incoming else {
        return incoming;
    };

    for key in SECRET_KEYS {
        if fields.get(*key).and_then(Value::as_str) != Some(SECRET_MASK) {
            continue;
        }

        match stored.map(|config| text(config, key)).unwrap_or_default() {
            "" => fields.remove(*key),
            secret => fields.insert((*key).to_string(), Value::String(secret.to_string())),
        };
    }

    incoming
}

/// Rejects a configuration that could never work, so the mistake shows when it is saved and not
/// as a delivery that fails or a pull that quietly does nothing.
pub fn validate(kind: ConnectorKind, config: &Value) -> Result<()> {
    if !config.is_object() {
        return Err(Error::invalid("connector `config_json` must be an object"));
    }

    match kind {
        ConnectorKind::S3 => {
            require(config, "bucket", "s3")?;
            access_key_pair(config)?;
            endpoint(config)?;
        }
        ConnectorKind::Kafka => {
            require(config, "brokers", "kafka")?;
            require(config, "topic", "kafka")?;
            both_or_neither(config, "sasl_username", "sasl_password")?;
        }
        ConnectorKind::AwsCloudwatchLogs => {
            require(config, "region", "aws_cloudwatch_logs")?;
            require(config, "log_group", "aws_cloudwatch_logs")?;
            access_key_pair(config)?;
        }
        ConnectorKind::AwsKinesisFirehose
        | ConnectorKind::CloudflareLogpush
        | ConnectorKind::HerokuDrain => push_token(config)?,
    }

    Ok(())
}

/// The trimmed string under `key`; empty when it is absent or not a string.
fn text<'a>(config: &'a Value, key: &str) -> &'a str {
    config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
}

fn require(config: &Value, key: &str, kind: &str) -> Result<()> {
    if text(config, key).is_empty() {
        return Err(Error::invalid(format!("{kind} connector requires `{key}`")));
    }

    Ok(())
}

fn both_or_neither(config: &Value, first: &str, second: &str) -> Result<()> {
    if text(config, first).is_empty() != text(config, second).is_empty() {
        return Err(Error::invalid(format!(
            "`{first}` and `{second}` go together: set both or neither"
        )));
    }

    Ok(())
}

/// An AWS access key is an id and a secret; a session token only means something beside them.
fn access_key_pair(config: &Value) -> Result<()> {
    both_or_neither(config, "access_key", "secret_key")?;

    if !text(config, "session_token").is_empty() && text(config, "access_key").is_empty() {
        return Err(Error::invalid(
            "`session_token` belongs to a temporary access key: set `access_key` and `secret_key` too",
        ));
    }

    Ok(())
}

fn endpoint(config: &Value) -> Result<()> {
    let endpoint = text(config, "endpoint");

    if !endpoint.is_empty() && !endpoint.starts_with("http://") && !endpoint.starts_with("https://")
    {
        return Err(Error::invalid(
            "`endpoint` must be an http:// or https:// URL",
        ));
    }

    Ok(())
}

/// A push connector is found by its token, so a short one is a weak lock. One without a token is
/// allowed: it receives nothing until it is given one.
fn push_token(config: &Value) -> Result<()> {
    let token = text(config, "push_token");

    if !token.is_empty() && token.len() < MIN_PUSH_TOKEN_LEN {
        return Err(Error::invalid(format!(
            "`push_token` must be at least {MIN_PUSH_TOKEN_LEN} characters"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_read_masks_every_secret_and_nothing_else() {
        let masked = mask_secrets(json!({
            "bucket": "logs",
            "access_key": "AKIAEXAMPLE",
            "secret_key": "s3cr3t",
            "session_token": "temporary",
            "sasl_password": "",
        }));

        assert_eq!(
            masked,
            json!({
                "bucket": "logs",
                "access_key": "***",
                "secret_key": "***",
                "session_token": "***",
                "sasl_password": "***",
            })
        );
    }

    #[test]
    fn the_secrets_that_are_set_are_named_without_their_values() {
        let config =
            json!({ "access_key": "AKIA", "secret_key": " ", "push_token": "t0ken", "topic": "x" });

        assert_eq!(stored_secret_keys(&config), ["access_key", "push_token"]);
        assert!(stored_secret_keys(&json!("not an object")).is_empty());
    }

    #[test]
    fn a_mask_sent_back_keeps_what_is_stored() {
        let stored =
            json!({ "bucket": "old", "access_key": "AKIAOLD", "secret_key": "old-secret" });
        let incoming = json!({ "bucket": "new", "access_key": "***", "secret_key": "new-secret" });

        assert_eq!(
            keep_stored_secrets(incoming, Some(&stored)),
            json!({ "bucket": "new", "access_key": "AKIAOLD", "secret_key": "new-secret" })
        );
    }

    #[test]
    fn a_mask_with_nothing_behind_it_is_dropped() {
        let incoming =
            json!({ "brokers": "b:9092", "sasl_password": "***", "session_token": "***" });
        let stored = json!({ "sasl_password": "" });

        // A new connector, or one that changed kind, has nothing to keep.
        assert_eq!(
            keep_stored_secrets(incoming.clone(), None),
            json!({ "brokers": "b:9092" })
        );
        assert_eq!(
            keep_stored_secrets(incoming, Some(&stored)),
            json!({ "brokers": "b:9092" })
        );
    }

    #[test]
    fn a_secret_that_was_left_out_is_removed_not_kept() {
        let stored = json!({ "access_key": "AKIA", "secret_key": "s" });

        assert_eq!(
            keep_stored_secrets(json!({ "bucket": "b" }), Some(&stored)),
            json!({ "bucket": "b" })
        );
    }

    #[test]
    fn a_sink_names_where_it_delivers() {
        assert!(validate(ConnectorKind::S3, &json!({ "bucket": "logs" })).is_ok());
        assert!(validate(ConnectorKind::S3, &json!({ "region": "us-east-1" })).is_err());
        assert!(validate(ConnectorKind::S3, &json!({ "bucket": "  " })).is_err());
        assert!(
            validate(
                ConnectorKind::Kafka,
                &json!({ "brokers": "b:9092", "topic": "logs" })
            )
            .is_ok()
        );
        assert!(validate(ConnectorKind::Kafka, &json!({ "brokers": "b:9092" })).is_err());
        assert!(validate(ConnectorKind::S3, &json!(["bucket"])).is_err());
    }

    #[test]
    fn credentials_come_whole_or_not_at_all() {
        let s3 = |extra: Value| {
            let mut config = json!({ "bucket": "logs" });
            config
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            validate(ConnectorKind::S3, &config)
        };

        // No keys: the node's own role.
        assert!(s3(json!({})).is_ok());
        assert!(s3(json!({ "access_key": "AKIA", "secret_key": "s" })).is_ok());
        assert!(
            s3(json!({ "access_key": "AKIA", "secret_key": "s", "session_token": "t" })).is_ok()
        );
        assert!(s3(json!({ "access_key": "AKIA" })).is_err());
        assert!(s3(json!({ "secret_key": "s" })).is_err());
        assert!(s3(json!({ "session_token": "t" })).is_err());

        let kafka = json!({ "brokers": "b:9092", "topic": "logs", "sasl_username": "svc" });
        assert!(validate(ConnectorKind::Kafka, &kafka).is_err());
    }

    #[test]
    fn an_s3_endpoint_is_a_url() {
        let with = |endpoint: &str| {
            validate(
                ConnectorKind::S3,
                &json!({ "bucket": "b", "endpoint": endpoint }),
            )
        };

        assert!(with("https://minio.internal:9000").is_ok());
        assert!(with("http://localhost:9000").is_ok());
        assert!(with("minio.internal:9000").is_err());
    }

    #[test]
    fn a_pull_source_says_what_to_pull() {
        let complete = json!({ "region": "us-east-1", "log_group": "/aws/lambda/fn" });

        // Credentials may follow later; until then the pull does nothing.
        assert!(validate(ConnectorKind::AwsCloudwatchLogs, &complete).is_ok());
        assert!(
            validate(
                ConnectorKind::AwsCloudwatchLogs,
                &json!({ "region": "us-east-1" })
            )
            .is_err()
        );
    }

    #[test]
    fn a_push_token_is_long_enough_to_be_a_credential() {
        for kind in [
            ConnectorKind::AwsKinesisFirehose,
            ConnectorKind::CloudflareLogpush,
            ConnectorKind::HerokuDrain,
        ] {
            assert!(validate(kind, &json!({})).is_ok());
            assert!(validate(kind, &json!({ "push_token": "0123456789abcdef" })).is_ok());
            assert!(validate(kind, &json!({ "push_token": "short" })).is_err());
        }
    }
}
