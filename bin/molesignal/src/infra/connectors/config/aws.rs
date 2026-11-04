// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How an AWS connector (S3, CloudWatch Logs) proves who it is.
//!
//! `auth` names the way; a configuration without it is read as an access key when it holds one,
//! a role to assume when it names one, and the node's own identity otherwise. Where the
//! credentials then come from is [`crate::infra::connectors::aws`].

use serde_json::Value;

use super::{auth_mode, both_or_neither, text, unused};
use crate::shared::{Error, Result};

const MODES: &[&str] = &[
    "access_key",
    "temporary_key",
    "node_role",
    "assume_role",
    "web_identity",
];

const KEY_FIELDS: &[&str] = &["accessKey", "secretKey", "sessionToken"];

/// An access key: an IAM user's, or a temporary one with the session token it was issued with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessKey {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AwsAuth {
    Key(AccessKey),
    /// The node's own identity: an ECS task role, EKS IRSA or Pod Identity, or the EC2 instance
    /// profile.
    Node,
    /// A role assumed through STS, from `base` when a key is given and from the node's identity
    /// otherwise. The external ID it is asked with is the organization's.
    AssumeRole {
        role_arn: String,
        base: Option<AccessKey>,
    },
    /// A role assumed with the web identity token the node is given, as EKS projects for IRSA.
    WebIdentity {
        role_arn: String,
    },
}

impl AwsAuth {
    pub fn parse(config: &Value) -> Result<Self> {
        let key = access_key(config)?;
        let role = text(config, "roleArn");

        let inferred = if key.is_some() {
            "access_key"
        } else if !role.is_empty() {
            "assume_role"
        } else {
            "node_role"
        };

        let mode = auth_mode(config, MODES, inferred)?;

        match mode {
            "access_key" | "temporary_key" => {
                unused(config, mode, &["roleArn"])?;

                let key = key.ok_or_else(|| {
                    Error::invalid(format!(
                        "`auth` `{mode}` needs `access_key` and `secret_key`"
                    ))
                })?;

                if mode == "temporary_key" && key.session_token.is_none() {
                    return Err(Error::invalid(
                        "a temporary key is only valid with the `session_token` it was issued with",
                    ));
                }

                Ok(Self::Key(key))
            }
            "node_role" => {
                unused(config, mode, KEY_FIELDS)?;
                unused(config, mode, &["roleArn"])?;
                Ok(Self::Node)
            }
            "assume_role" => Ok(Self::AssumeRole {
                role_arn: role_arn(config)?,
                base: key,
            }),
            _ => {
                unused(config, mode, KEY_FIELDS)?;

                Ok(Self::WebIdentity {
                    role_arn: role_arn(config)?,
                })
            }
        }
    }
}

/// An AWS access key is an id and a secret; a session token only means something beside them.
fn access_key(config: &Value) -> Result<Option<AccessKey>> {
    both_or_neither(config, "accessKey", "secretKey")?;

    let session_token = text(config, "sessionToken");

    if !session_token.is_empty() && text(config, "accessKey").is_empty() {
        return Err(Error::invalid(
            "`session_token` belongs to a temporary access key: set `access_key` and `secret_key` too",
        ));
    }

    if text(config, "accessKey").is_empty() {
        return Ok(None);
    }

    Ok(Some(AccessKey {
        access_key: text(config, "accessKey").to_string(),
        secret_key: text(config, "secretKey").to_string(),
        session_token: (!session_token.is_empty()).then(|| session_token.to_string()),
    }))
}

/// `arn:<partition>:iam::<12-digit account>:role/<path and name>`.
fn role_arn(config: &Value) -> Result<String> {
    let arn = text(config, "roleArn");

    if arn.is_empty() {
        return Err(Error::invalid("assuming a role needs its `role_arn`"));
    }

    let parts: Vec<&str> = arn.splitn(6, ':').collect();

    let well_formed = parts.len() == 6
        && parts[0] == "arn"
        && parts[1].starts_with("aws")
        && parts[2] == "iam"
        && parts[3].is_empty()
        && parts[4].len() == 12
        && parts[4].bytes().all(|byte| byte.is_ascii_digit())
        && parts[5]
            .strip_prefix("role/")
            .is_some_and(|name| !name.is_empty());

    if !well_formed {
        return Err(Error::invalid(format!(
            "`role_arn` must read arn:aws:iam::<account>:role/<name>, not `{arn}`"
        )));
    }

    Ok(arn.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ROLE: &str = "arn:aws:iam::123456789012:role/molesignal-delivery";

    fn key(token: Option<&str>) -> AccessKey {
        AccessKey {
            access_key: "AKIA".into(),
            secret_key: "secret".into(),
            session_token: token.map(str::to_string),
        }
    }

    #[test]
    fn a_configuration_without_auth_is_read_by_what_it_holds() {
        assert_eq!(AwsAuth::parse(&json!({})).unwrap(), AwsAuth::Node);
        assert_eq!(
            AwsAuth::parse(&json!({ "accessKey": "AKIA", "secretKey": "secret" })).unwrap(),
            AwsAuth::Key(key(None))
        );
        assert_eq!(
            AwsAuth::parse(&json!({ "roleArn": ROLE })).unwrap(),
            AwsAuth::AssumeRole {
                role_arn: ROLE.into(),
                base: None
            }
        );
    }

    #[test]
    fn each_way_takes_what_it_needs() {
        let temporary = json!({
            "auth": "temporary_key",
            "accessKey": "AKIA",
            "secretKey": "secret",
            "sessionToken": "issued-with-it",
        });

        assert_eq!(
            AwsAuth::parse(&temporary).unwrap(),
            AwsAuth::Key(key(Some("issued-with-it")))
        );
        assert!(
            AwsAuth::parse(
                &json!({ "auth": "temporary_key", "accessKey": "AKIA", "secretKey": "s" })
            )
            .is_err()
        );
        assert!(AwsAuth::parse(&json!({ "auth": "access_key" })).is_err());
        assert!(AwsAuth::parse(&json!({ "auth": "assume_role" })).is_err());
        assert_eq!(
            AwsAuth::parse(&json!({ "auth": "web_identity", "roleArn": ROLE })).unwrap(),
            AwsAuth::WebIdentity {
                role_arn: ROLE.into()
            }
        );
    }

    #[test]
    fn a_role_can_be_assumed_from_a_key() {
        let config = json!({
            "auth": "assume_role",
            "roleArn": ROLE,
            "accessKey": "AKIA",
            "secretKey": "secret",
        });

        assert_eq!(
            AwsAuth::parse(&config).unwrap(),
            AwsAuth::AssumeRole {
                role_arn: ROLE.into(),
                base: Some(key(None))
            }
        );
    }

    #[test]
    fn what_a_way_does_not_use_is_refused() {
        assert!(
            AwsAuth::parse(&json!({ "auth": "node_role", "accessKey": "AKIA", "secretKey": "s" }))
                .is_err()
        );
        assert!(
            AwsAuth::parse(&json!({
                "auth": "web_identity",
                "roleArn": ROLE,
                "accessKey": "AKIA",
                "secretKey": "s",
            }))
            .is_err()
        );
        assert!(
            AwsAuth::parse(&json!({
                "auth": "access_key",
                "accessKey": "AKIA",
                "secretKey": "s",
                "roleArn": ROLE,
            }))
            .is_err()
        );
    }

    #[test]
    fn a_role_arn_names_an_iam_role() {
        let with = |arn: &str| AwsAuth::parse(&json!({ "auth": "assume_role", "roleArn": arn }));

        assert!(with(ROLE).is_ok());
        assert!(with("arn:aws-cn:iam::123456789012:role/path/to/role").is_ok());
        assert!(with("arn:aws:iam::123456789012:user/someone").is_err());
        assert!(with("arn:aws:iam::12345:role/short-account").is_err());
        assert!(with("arn:aws:s3:::bucket").is_err());
        assert!(with("molesignal-delivery").is_err());
    }
}
