// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The credential provider an AWS connector signs with, kept between uses.
//!
//! The node's identity is what the AWS SDKs would find on it — EKS IRSA or Pod Identity, an ECS
//! task role, the EC2 instance profile — except a static key in the environment: such a key is
//! more likely the platform's own than one meant for every organization's connectors.

use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};

use moka::sync::Cache;
use object_store::{
    ClientOptions, RetryConfig, StaticCredentialProvider,
    aws::{AmazonS3Builder, AmazonS3ConfigKey, AwsCredential, AwsCredentialProvider},
};

use super::{external_id, region_or_default, session_name, sts::AssumeRoleProvider, sts_endpoint};
use crate::{
    infra::connectors::{
        config::aws::{AccessKey, AwsAuth},
        repo::Connector,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// The environment that makes up the node's identity, and the setting each one fills in.
const NODE_IDENTITY_ENV: &[(&str, AmazonS3ConfigKey)] = &[
    (
        "AWS_WEB_IDENTITY_TOKEN_FILE",
        AmazonS3ConfigKey::WebIdentityTokenFile,
    ),
    ("AWS_ROLE_ARN", AmazonS3ConfigKey::RoleArn),
    ("AWS_ROLE_SESSION_NAME", AmazonS3ConfigKey::RoleSessionName),
    (
        "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
        AmazonS3ConfigKey::ContainerCredentialsRelativeUri,
    ),
    (
        "AWS_CONTAINER_CREDENTIALS_FULL_URI",
        AmazonS3ConfigKey::ContainerCredentialsFullUri,
    ),
    (
        "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE",
        AmazonS3ConfigKey::ContainerAuthorizationTokenFile,
    ),
];

/// Providers by connector and the version of its settings. A provider keeps the key it fetched
/// until shortly before it expires, so keeping the provider is what saves a pipeline delivering
/// every few seconds from asking the node or STS for a new key each time.
static PROVIDERS: LazyLock<Cache<String, AwsCredentialProvider>> = LazyLock::new(|| {
    Cache::builder()
        .max_capacity(4096)
        .time_to_idle(Duration::from_secs(3600))
        .build()
});

/// Whether a provider may be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    /// The one kept for this connector, as long as its settings have not changed since.
    Kept,
    /// A new one, for settings being tried before they are saved.
    Fresh,
}

/// Whose connector signs: its provider is kept by its id and the version of its settings, and a
/// role is assumed with its organization's external ID and a session named after it.
#[derive(Debug, Clone, Copy)]
pub struct Signer<'a> {
    pub id: &'a Id,
    pub org_id: &'a Id,
    pub updated_at: TimestampMicros,
}

/// The credentials `connector` signs its requests to `region` with, as `auth` says.
pub fn credential_provider(
    connector: &Connector,
    auth: &AwsAuth,
    region: &str,
    source: CredentialSource,
) -> Result<AwsCredentialProvider> {
    let signer = Signer {
        id: &connector.id,
        org_id: &connector.org_id,
        updated_at: connector.updated_at,
    };

    signer_credentials(signer, auth, region, source)
}

/// The credentials a connector known only by `signer` signs with, as a notification connector is.
pub fn signer_credentials(
    signer: Signer<'_>,
    auth: &AwsAuth,
    region: &str,
    source: CredentialSource,
) -> Result<AwsCredentialProvider> {
    if source == CredentialSource::Fresh {
        return build(signer, auth, region);
    }

    let key = format!(
        "{}:{}:{}",
        signer.id.0,
        signer.updated_at.0,
        region_or_default(region)
    );

    if let Some(provider) = PROVIDERS.get(&key) {
        return Ok(provider);
    }

    let provider = build(signer, auth, region)?;
    PROVIDERS.insert(key, provider.clone());

    Ok(provider)
}

fn build(signer: Signer<'_>, auth: &AwsAuth, region: &str) -> Result<AwsCredentialProvider> {
    match auth {
        AwsAuth::Key(key) => Ok(static_key(key)),
        AwsAuth::Node => node_identity(region),
        AwsAuth::WebIdentity { role_arn } => {
            web_identity(region, role_arn, &session_name(signer.id))
        }
        AwsAuth::AssumeRole { role_arn, base } => {
            let base = match base {
                Some(key) => static_key(key),
                None => node_identity(region)?,
            };

            Ok(Arc::new(AssumeRoleProvider::new(
                base,
                role_arn.clone(),
                external_id(signer.org_id),
                session_name(signer.id),
                region_or_default(region).to_string(),
            )))
        }
    }
}

fn static_key(key: &AccessKey) -> AwsCredentialProvider {
    Arc::new(StaticCredentialProvider::new(AwsCredential {
        key_id: key.access_key.clone(),
        secret_key: key.secret_key.clone(),
        token: key.session_token.clone(),
    }))
}

fn node_identity(region: &str) -> Result<AwsCredentialProvider> {
    let mut builder = chain(region);

    for (variable, key) in NODE_IDENTITY_ENV {
        if let Some(value) = env(variable) {
            builder = builder.with_config(*key, value);
        }
    }

    borrow(builder)
}

/// A role assumed with the web identity token the node is given (EKS projects one for IRSA).
/// The token's file comes from the node's environment only: a path a connector could set would
/// let it send any file the server can read to whichever STS it named.
fn web_identity(region: &str, role_arn: &str, session_name: &str) -> Result<AwsCredentialProvider> {
    let token_file = env("AWS_WEB_IDENTITY_TOKEN_FILE").ok_or_else(|| {
        Error::unavailable(
            "aws credentials: this node has no web identity token to assume a role with (AWS_WEB_IDENTITY_TOKEN_FILE is not set)",
        )
    })?;

    borrow(
        chain(region)
            .with_config(AmazonS3ConfigKey::WebIdentityTokenFile, token_file)
            .with_config(AmazonS3ConfigKey::RoleArn, role_arn)
            .with_config(AmazonS3ConfigKey::RoleSessionName, session_name),
    )
}

/// A client set up only for its credential chain: it never sends a request of its own.
fn chain(region: &str) -> AmazonS3Builder {
    // STS in the region's own partition, unless the node says otherwise.
    let sts = env("AWS_ENDPOINT_URL_STS").unwrap_or_else(|| sts_endpoint(region));

    AmazonS3Builder::new()
        .with_bucket_name("molesignal-credentials")
        .with_region(region_or_default(region))
        .with_config(AmazonS3ConfigKey::StsEndpoint, sts)
        // A node without an identity should say so in seconds, not hold up the pulls queued
        // behind it while its metadata service is retried for minutes.
        .with_retry(RetryConfig {
            max_retries: 2,
            retry_timeout: Duration::from_secs(15),
            ..RetryConfig::default()
        })
        .with_client_options(
            ClientOptions::new()
                .with_connect_timeout(Duration::from_secs(3))
                .with_timeout(Duration::from_secs(10)),
        )
}

fn borrow(builder: AmazonS3Builder) -> Result<AwsCredentialProvider> {
    let client = builder
        .build()
        .map_err(|error| Error::internal(format!("aws credentials: {error}")))?;

    Ok(client.credentials().clone())
}

fn env(variable: &str) -> Option<String> {
    std::env::var(variable)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn connector(updated_at: i64) -> Connector {
        Connector {
            id: Id::from_string("c1"),
            org_id: Id::from_string("o1"),
            name: "archive".into(),
            kind: "s3".into(),
            config_json: json!({}),
            enabled: true,
            last_run_at: None,
            read_state: serde_json::Value::Null,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(updated_at),
        }
    }

    fn key() -> AwsAuth {
        AwsAuth::Key(AccessKey {
            access_key: "AKIA".into(),
            secret_key: "secret".into(),
            session_token: Some("token".into()),
        })
    }

    #[tokio::test]
    async fn an_access_key_signs_as_itself() {
        let provider =
            credential_provider(&connector(1), &key(), "eu-west-1", CredentialSource::Fresh)
                .unwrap();
        let credential = provider.get_credential().await.unwrap();

        assert_eq!(credential.key_id, "AKIA");
        assert_eq!(credential.token.as_deref(), Some("token"));
    }

    #[test]
    fn a_kept_provider_lasts_as_long_as_the_settings() {
        let first = credential_provider(&connector(7), &key(), "eu-west-1", CredentialSource::Kept)
            .unwrap();
        let again = credential_provider(&connector(7), &key(), "eu-west-1", CredentialSource::Kept)
            .unwrap();
        let changed =
            credential_provider(&connector(8), &key(), "eu-west-1", CredentialSource::Kept)
                .unwrap();
        let tried =
            credential_provider(&connector(7), &key(), "eu-west-1", CredentialSource::Fresh)
                .unwrap();

        assert!(Arc::ptr_eq(&first, &again));
        assert!(!Arc::ptr_eq(&first, &changed));
        assert!(!Arc::ptr_eq(&first, &tried));
    }

    #[test]
    fn roles_get_a_provider_without_a_request_until_one_signs() {
        let assume = AwsAuth::AssumeRole {
            role_arn: "arn:aws:iam::123456789012:role/delivery".into(),
            base: None,
        };

        assert!(credential_provider(&connector(1), &assume, "", CredentialSource::Fresh).is_ok());
        assert!(
            credential_provider(&connector(1), &AwsAuth::Node, "", CredentialSource::Fresh).is_ok()
        );
    }
}
