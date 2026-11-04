// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a connector's `config_json` may hold, and how its secrets are handled at the API edge.
//!
//! Each kind with more than one way of authenticating reads its settings through a typed view
//! ([`aws::AwsAuth`], [`kafka::KafkaSettings`], [`http::HttpSettings`]). Saving and delivering
//! go through the same view, so what is accepted on save is what a delivery can use.
//!
//! Secrets never leave through the API: a read replaces each with [`SECRET_MASK`]. An update that
//! sends the mask back means "leave it as it is", so a connector can be renamed, switched off or
//! pointed elsewhere without anyone typing its credentials again. At rest they are sealed apart
//! from the rest of the settings (see [`super::secrets`]).

use serde_json::Value;

use super::ConnectorKind;
use crate::shared::{Error, Result};

pub mod aws;
pub mod http;
pub mod kafka;

/// What a secret reads as in an API response.
pub const SECRET_MASK: &str = "***";

/// Keys of `config_json` whose values are secrets.
pub(super) const SECRET_KEYS: &[&str] = &[
    "accessKey",
    "secretKey",
    "sessionToken",
    "pushToken",
    "webhookUrl",
    "clientSecret",
    "password",
    "saslPassword",
    "tlsClientKey",
    // An HTTP destination's address often carries its credential (a collector code in the path,
    // an API key in the query), so it is kept like one; `urlOrigin` says where it points.
    "url",
    "bearerToken",
    "basicPassword",
    "headerValue",
    "hmacSecret",
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
            endpoint(config)?;
            aws::AwsAuth::parse(config)?;
        }
        ConnectorKind::Kafka => {
            kafka::KafkaSettings::parse(config)?;
        }
        ConnectorKind::Http => {
            http::HttpSettings::parse(config)?;
        }
        ConnectorKind::AwsCloudwatchLogs => {
            require(config, "region", "aws_cloudwatch_logs")?;
            require(config, "logGroup", "aws_cloudwatch_logs")?;
            aws::AwsAuth::parse(config)?;
        }
        ConnectorKind::KafkaSource => {
            kafka::KafkaSettings::parse(config)?;
            start_from(config)?;
        }
        ConnectorKind::S3Source => {
            require(config, "bucket", "s3")?;
            endpoint(config)?;
            aws::AwsAuth::parse(config)?;
            start_from(config)?;
        }
        ConnectorKind::AwsKinesisFirehose
        | ConnectorKind::CloudflareLogpush
        | ConnectorKind::HerokuDrain => push_token(config)?,
    }

    Ok(())
}

/// Where a new pull source starts reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartFrom {
    /// What arrives from now on.
    Latest,
    /// Everything that is there.
    Earliest,
}

pub fn start_from(config: &Value) -> Result<StartFrom> {
    match text(config, "startFrom") {
        "" | "latest" => Ok(StartFrom::Latest),
        "earliest" => Ok(StartFrom::Earliest),
        other => Err(Error::invalid(format!(
            "`start_from` must be latest or earliest, not `{other}`"
        ))),
    }
}

/// `config` with the settings the server derives from the others filled in: where an HTTP
/// destination points, since its address itself is kept as a secret.
pub fn with_derived(kind: ConnectorKind, mut config: Value) -> Value {
    if kind != ConnectorKind::Http {
        return config;
    }

    let origin = http::HttpSettings::parse(&config)
        .ok()
        .map(|settings| settings.origin());

    if let (Value::Object(fields), Some(origin)) = (&mut config, origin) {
        fields.insert("urlOrigin".to_string(), Value::String(origin));
    }

    config
}

/// The trimmed string under `key`; empty when it is absent or not a string.
pub(super) fn text<'a>(config: &'a Value, key: &str) -> &'a str {
    config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
}

/// A switch in the configuration: `true`, or the string `"true"` a form may have sent.
pub(super) fn flag(config: &Value, key: &str) -> bool {
    match config.get(key) {
        Some(Value::Bool(on)) => *on,
        Some(Value::String(text)) => text.trim().eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// The way of authenticating `config` names under `auth`, if it is one of `modes`; `inferred`
/// when it names none, as a client that predates `auth` sends.
pub(super) fn auth_mode<'a>(
    config: &'a Value,
    modes: &[&'a str],
    inferred: &'a str,
) -> Result<&'a str> {
    match text(config, "auth") {
        "" => Ok(inferred),
        mode if modes.contains(&mode) => Ok(mode),
        other => Err(Error::invalid(format!(
            "`auth` must be one of {}, not `{other}`",
            modes.join(", ")
        ))),
    }
}

/// Settings a way of authenticating has no use for are refused rather than stored unused: a
/// secret kept that nothing reads is one more to leak.
pub(super) fn unused(config: &Value, mode: &str, keys: &[&str]) -> Result<()> {
    match keys.iter().find(|key| !text(config, key).is_empty()) {
        Some(key) => Err(Error::invalid(format!(
            "`{key}` is not used when `auth` is `{mode}`"
        ))),
        None => Ok(()),
    }
}

pub(super) fn require(config: &Value, key: &str, kind: &str) -> Result<()> {
    if text(config, key).is_empty() {
        return Err(Error::invalid(format!("{kind} connector requires `{key}`")));
    }

    Ok(())
}

pub(super) fn both_or_neither(config: &Value, first: &str, second: &str) -> Result<()> {
    if text(config, first).is_empty() != text(config, second).is_empty() {
        return Err(Error::invalid(format!(
            "`{first}` and `{second}` go together: set both or neither"
        )));
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
    let token = text(config, "pushToken");

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
            "accessKey": "AKIAEXAMPLE",
            "secretKey": "s3cr3t",
            "sessionToken": "temporary",
            "saslPassword": "",
            "url": "https://collector.example/receiver/v1/http/c0de",
            "urlOrigin": "https://collector.example",
            "tlsClientCert": "-----BEGIN CERTIFICATE-----",
            "tlsClientKey": "-----BEGIN PRIVATE KEY-----",
        }));

        assert_eq!(
            masked,
            json!({
                "bucket": "logs",
                "accessKey": "***",
                "secretKey": "***",
                "sessionToken": "***",
                "saslPassword": "***",
                "url": "***",
                "urlOrigin": "https://collector.example",
                "tlsClientCert": "-----BEGIN CERTIFICATE-----",
                "tlsClientKey": "***",
            })
        );
    }

    #[test]
    fn the_secrets_that_are_set_are_named_without_their_values() {
        let config =
            json!({ "accessKey": "AKIA", "secretKey": " ", "pushToken": "t0ken", "topic": "x" });

        assert_eq!(stored_secret_keys(&config), ["accessKey", "pushToken"]);
        assert!(stored_secret_keys(&json!("not an object")).is_empty());
    }

    #[test]
    fn a_mask_sent_back_keeps_what_is_stored() {
        let stored = json!({ "bucket": "old", "accessKey": "AKIAOLD", "secretKey": "old-secret" });
        let incoming = json!({ "bucket": "new", "accessKey": "***", "secretKey": "new-secret" });

        assert_eq!(
            keep_stored_secrets(incoming, Some(&stored)),
            json!({ "bucket": "new", "accessKey": "AKIAOLD", "secretKey": "new-secret" })
        );
    }

    #[test]
    fn a_mask_with_nothing_behind_it_is_dropped() {
        let incoming = json!({ "brokers": "b:9092", "saslPassword": "***", "sessionToken": "***" });
        let stored = json!({ "saslPassword": "" });

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
        let stored = json!({ "accessKey": "AKIA", "secretKey": "s" });

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
        assert!(
            validate(
                ConnectorKind::Http,
                &json!({ "url": "https://in.example/logs" })
            )
            .is_ok()
        );
        assert!(validate(ConnectorKind::Http, &json!({})).is_err());
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

        // No keys: the node's own identity.
        assert!(s3(json!({})).is_ok());
        assert!(s3(json!({ "accessKey": "AKIA", "secretKey": "s" })).is_ok());
        assert!(s3(json!({ "accessKey": "AKIA", "secretKey": "s", "sessionToken": "t" })).is_ok());
        assert!(s3(json!({ "accessKey": "AKIA" })).is_err());
        assert!(s3(json!({ "secretKey": "s" })).is_err());
        assert!(s3(json!({ "sessionToken": "t" })).is_err());

        let kafka = json!({ "brokers": "b:9092", "topic": "logs", "saslUsername": "svc" });
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
        let complete = json!({ "region": "us-east-1", "logGroup": "/aws/lambda/fn" });

        // With no key it pulls as the node.
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
    fn a_kafka_or_s3_source_names_what_it_reads_and_where_it_starts() {
        let kafka = |extra: Value| {
            let mut config = json!({ "brokers": "b:9092", "topic": "logs" });
            config
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            validate(ConnectorKind::KafkaSource, &config)
        };

        assert!(kafka(json!({})).is_ok());
        assert!(kafka(json!({ "startFrom": "earliest" })).is_ok());
        assert!(kafka(json!({ "startFrom": "yesterday" })).is_err());
        assert!(validate(ConnectorKind::KafkaSource, &json!({ "topic": "logs" })).is_err());

        assert!(validate(ConnectorKind::S3Source, &json!({ "bucket": "b" })).is_ok());
        assert!(validate(ConnectorKind::S3Source, &json!({})).is_err());
        assert!(
            validate(
                ConnectorKind::S3Source,
                &json!({ "bucket": "b", "endpoint": "minio:9000" })
            )
            .is_err()
        );

        assert_eq!(start_from(&json!({})).unwrap(), StartFrom::Latest);
    }

    #[test]
    fn a_push_token_is_long_enough_to_be_a_credential() {
        for kind in [
            ConnectorKind::AwsKinesisFirehose,
            ConnectorKind::CloudflareLogpush,
            ConnectorKind::HerokuDrain,
        ] {
            assert!(validate(kind, &json!({})).is_ok());
            assert!(validate(kind, &json!({ "pushToken": "0123456789abcdef" })).is_ok());
            assert!(validate(kind, &json!({ "pushToken": "short" })).is_err());
        }
    }

    #[test]
    fn an_auth_mode_outside_the_kinds_list_is_refused() {
        let config = json!({ "bucket": "b", "auth": "sasl_plain" });

        assert!(validate(ConnectorKind::S3, &config).is_err());
        assert_eq!(
            auth_mode(&json!({}), &["none", "basic"], "none").unwrap(),
            "none"
        );
        assert_eq!(
            auth_mode(&json!({ "auth": "basic" }), &["none", "basic"], "none").unwrap(),
            "basic"
        );
    }

    #[test]
    fn an_http_destination_says_where_it_points_without_its_address() {
        let config = with_derived(
            ConnectorKind::Http,
            json!({ "url": "https://collector.example:8443/receiver/v1/http/c0de?x=1" }),
        );

        assert_eq!(config["urlOrigin"], "https://collector.example:8443");
        assert!(
            with_derived(ConnectorKind::S3, json!({ "bucket": "b" }))
                .get("urlOrigin")
                .is_none()
        );
    }
}
