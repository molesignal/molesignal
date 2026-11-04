// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! S3 egress: each batch becomes one JSON Lines object under the connector's prefix.

use std::time::Duration;

use object_store::{
    ClientOptions, ObjectStoreExt, PutPayload, RetryConfig, aws::AmazonS3Builder, path::Path,
};
use serde_json::Value;

use super::to_json_lines;
use crate::{
    infra::connectors::{
        aws::{CredentialSource, credential_provider},
        config::{
            self,
            aws::{AccessKey, AwsAuth},
        },
        repo::Connector,
    },
    shared::{Error, Result, ids::Id},
};

/// The object a connection test writes and then removes, when the policy allows removing it.
const TEST_OBJECT: &str = ".molesignal-connection-test";

/// 规整对象 key 前缀：去首部 `/`，非空时确保末尾 `/`。
pub(crate) fn normalize_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim().trim_start_matches('/');

    if trimmed.is_empty() {
        String::new()
    } else if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{trimmed}/")
    }
}

/// The S3 client a connector's configuration describes, short of its credentials, and how it
/// authenticates.
fn store(connector: &Connector) -> Result<(AmazonS3Builder, AwsAuth)> {
    let settings = &connector.config_json;
    let bucket = config::text(settings, "bucket");

    if bucket.is_empty() {
        return Err(Error::invalid("s3 connector requires `bucket`"));
    }

    let mut builder = AmazonS3Builder::new().with_bucket_name(bucket);
    let region = config::text(settings, "region");

    if !region.is_empty() {
        builder = builder.with_region(region);
    }

    let endpoint = config::text(settings, "endpoint");

    if !endpoint.is_empty() {
        builder = builder.with_endpoint(endpoint).with_allow_http(true);
    }

    // Some S3-compatible stores address a bucket as a host name and refuse it in the path.
    if config::flag(settings, "virtualHostedStyle") {
        builder = builder.with_virtual_hosted_style_request(true);
    }

    Ok((builder, AwsAuth::parse(settings)?))
}

fn with_key(builder: AmazonS3Builder, key: AccessKey) -> AmazonS3Builder {
    let builder = builder
        .with_access_key_id(key.access_key)
        .with_secret_access_key(key.secret_key);

    // A temporary access key is only valid together with the token it was issued with.
    match key.session_token {
        Some(token) => builder.with_token(token),
        None => builder,
    }
}

/// The S3 client a connector's configuration describes, short of being built.
pub(crate) fn builder(connector: &Connector, source: CredentialSource) -> Result<AmazonS3Builder> {
    let (builder, auth) = store(connector)?;
    let region = config::text(&connector.config_json, "region");

    match auth {
        AwsAuth::Key(key) => Ok(with_key(builder, key)),
        auth => {
            Ok(builder.with_credentials(credential_provider(connector, &auth, region, source)?))
        }
    }
}

pub async fn dispatch(
    connector: &Connector,
    events: &[Value],
    source: CredentialSource,
) -> Result<()> {
    let store = builder(connector, source)?
        .build()
        .map_err(|e| Error::internal(format!("s3 egress build: {e}")))?;

    let prefix = normalize_prefix(config::text(&connector.config_json, "prefix"));
    let key = format!("{prefix}{}.jsonl", Id::new().0);
    let body = to_json_lines(events).into_bytes();

    store
        .put(&Path::from(key), PutPayload::from(body))
        .await
        .map_err(|e| Error::internal(format!("s3 egress put: {e}")))?;

    Ok(())
}

/// Writes a small object where the deliveries go, which needs the same `s3:PutObject` they do,
/// then removes it if the policy allows that too: a policy that only allows writing keeps it,
/// and the next test writes over it.
///
/// A role's or the node's credentials are fetched first, so that a node without an identity, or
/// a role that would not be assumed, is told apart from a bucket that refuses the request.
pub async fn check(connector: &Connector) -> Result<()> {
    let (builder, auth) = store(connector)?;
    let region = config::text(&connector.config_json, "region");

    let builder = match auth {
        AwsAuth::Key(key) => with_key(builder, key),
        auth => {
            let provider = credential_provider(connector, &auth, region, CredentialSource::Fresh)?;

            provider
                .get_credential()
                .await
                .map_err(|e| Error::unauthorized(format!("aws credentials: {e}")))?;

            builder.with_credentials(provider)
        }
    };

    let store = builder
        .with_retry(RetryConfig {
            max_retries: 1,
            retry_timeout: Duration::from_secs(10),
            ..RetryConfig::default()
        })
        .with_client_options(
            ClientOptions::new()
                .with_timeout(Duration::from_secs(10))
                .with_connect_timeout(Duration::from_secs(5))
                .with_allow_http(true),
        )
        .build()
        .map_err(|e| Error::invalid(format!("s3: {e}")))?;

    let prefix = normalize_prefix(config::text(&connector.config_json, "prefix"));
    let path = Path::from(format!("{prefix}{TEST_OBJECT}"));

    store
        .put(
            &path,
            PutPayload::from_static(b"MoleSignal connection test\n"),
        )
        .await
        .map_err(|e| Error::unavailable(format!("s3 put: {e}")))?;

    let _ = store.delete(&path).await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use object_store::aws::AmazonS3ConfigKey;

    use super::*;

    #[test]
    fn prefix_normalization() {
        assert_eq!(normalize_prefix(""), "");
        assert_eq!(normalize_prefix("  "), "");
        assert_eq!(normalize_prefix("logs"), "logs/");
        assert_eq!(normalize_prefix("/logs/"), "logs/");
        assert_eq!(normalize_prefix("a/b"), "a/b/");
    }

    fn s3_connector(config: Value) -> Connector {
        use crate::shared::time::TimestampMicros;

        Connector {
            id: Id::new(),
            org_id: Id::new(),
            name: "archive".into(),
            kind: "s3".into(),
            config_json: config,
            enabled: true,
            last_run_at: None,
            read_state: serde_json::Value::Null,
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    fn fresh(config: Value) -> Result<AmazonS3Builder> {
        builder(&s3_connector(config), CredentialSource::Fresh)
    }

    #[test]
    fn s3_client_takes_a_temporary_key_with_its_token() {
        let builder = fresh(serde_json::json!({
            "bucket": "logs",
            "region": "eu-west-1",
            "auth": "temporary_key",
            "accessKey": "ASIAEXAMPLE",
            "secretKey": "secret",
            "sessionToken": "issued-with-the-key",
        }))
        .unwrap();

        let value = |key| builder.get_config_value(&key);
        assert_eq!(value(AmazonS3ConfigKey::Bucket).as_deref(), Some("logs"));
        assert_eq!(
            value(AmazonS3ConfigKey::Region).as_deref(),
            Some("eu-west-1")
        );
        assert_eq!(
            value(AmazonS3ConfigKey::AccessKeyId).as_deref(),
            Some("ASIAEXAMPLE")
        );
        assert_eq!(
            value(AmazonS3ConfigKey::Token).as_deref(),
            Some("issued-with-the-key")
        );
    }

    #[test]
    fn s3_client_without_a_key_is_left_to_the_node_identity() {
        let builder = fresh(serde_json::json!({ "bucket": "logs" })).unwrap();

        assert_eq!(
            builder.get_config_value(&AmazonS3ConfigKey::AccessKeyId),
            None
        );
        assert_eq!(builder.get_config_value(&AmazonS3ConfigKey::Token), None);
        assert!(fresh(serde_json::json!({ "region": "eu-west-1" })).is_err());
    }

    #[test]
    fn a_role_is_signed_for_by_a_provider_not_a_key() {
        let builder = fresh(serde_json::json!({
            "bucket": "logs",
            "auth": "assume_role",
            "roleArn": "arn:aws:iam::123456789012:role/delivery",
        }))
        .unwrap();

        // The assumed role's key is fetched when the first request is signed.
        assert_eq!(
            builder.get_config_value(&AmazonS3ConfigKey::AccessKeyId),
            None
        );
        assert!(builder.build().is_ok());
    }

    #[test]
    fn s3_compatible_store_is_addressed_as_it_asks() {
        let builder = fresh(serde_json::json!({
            "bucket": "logs",
            "endpoint": "https://oss-cn-hangzhou.aliyuncs.com",
            "virtualHostedStyle": true,
        }))
        .unwrap();

        assert_eq!(
            builder
                .get_config_value(&AmazonS3ConfigKey::Endpoint)
                .as_deref(),
            Some("https://oss-cn-hangzhou.aliyuncs.com")
        );
        assert_eq!(
            builder
                .get_config_value(&AmazonS3ConfigKey::VirtualHostedStyleRequest)
                .as_deref(),
            Some("true")
        );

        // A form may send the switch as text; anything else leaves path-style addressing.
        let text = serde_json::json!({ "bucket": "b", "virtualHostedStyle": "true" });
        let off = serde_json::json!({ "bucket": "b", "virtualHostedStyle": "no" });
        assert!(config::flag(&text, "virtualHostedStyle"));
        assert!(!config::flag(&off, "virtualHostedStyle"));
        assert!(!config::flag(&off, "absent"));
    }
}
