// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where an AWS connector's credentials come from, as its [`AwsAuth`] says.
//!
//! No AWS SDK (the repository's rule): the node's identity and web identity are read through
//! `object_store`'s credential chain, STS AssumeRole is one signed request ([`sts`]), and
//! requests are signed by [`sigv4`].
//!
//! [`AwsAuth`]: super::config::aws::AwsAuth

pub mod credentials;
pub mod sigv4;
mod sts;

pub use credentials::{CredentialSource, Signer, credential_provider, signer_credentials};

use crate::shared::ids::Id;

/// The external ID STS is asked with when a connector of `org_id` assumes a role.
///
/// It is the organization's and cannot be set: a role that requires it can only be assumed for
/// this organization, even when another one learns the role's ARN.
pub fn external_id(org_id: &Id) -> String {
    format!("molesignal-{}", org_id.0)
}

/// What a connector's sessions are named, which is how they show in the account's CloudTrail.
pub fn session_name(connector_id: &Id) -> String {
    format!("molesignal-{}", connector_id.0)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "+=,.@-_".contains(*c))
        .take(64)
        .collect()
}

/// The region a connector names, or the one AWS falls back to.
pub fn region_or_default(region: &str) -> &str {
    match region.trim() {
        "" => "us-east-1",
        region => region,
    }
}

/// The domains of the partitions: the China regions (`cn-`) are in the second.
pub const PARTITION_DOMAINS: [&str; 2] = ["amazonaws.com", "amazonaws.com.cn"];

/// The domain of the partition `region` is in.
fn partition_domain(region: &str) -> &'static str {
    if region.starts_with("cn-") {
        PARTITION_DOMAINS[1]
    } else {
        PARTITION_DOMAINS[0]
    }
}

/// The regional STS endpoint for `region`, in its partition.
pub fn sts_endpoint(region: &str) -> String {
    let region = region_or_default(region);

    format!("https://sts.{region}.{}", partition_domain(region))
}

/// The host of the CloudWatch Logs endpoint for `region`.
pub fn logs_host(region: &str) -> String {
    format!("logs.{region}.{}", partition_domain(region))
}

/// The host of the End User Messaging SMS and voice endpoint for `region`.
pub fn sms_voice_host(region: &str) -> String {
    format!("sms-voice.{region}.{}", partition_domain(region))
}

/// The host of the EC2 endpoint for `region`, which lists the account's regions.
pub fn ec2_host(region: &str) -> String {
    format!("ec2.{region}.{}", partition_domain(region))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_external_id_belongs_to_the_organization() {
        assert_eq!(
            external_id(&Id::from_string("2pQ4kX9")),
            "molesignal-2pQ4kX9"
        );
        assert_ne!(
            external_id(&Id::from_string("a")),
            external_id(&Id::from_string("b"))
        );
    }

    #[test]
    fn a_session_name_keeps_to_what_sts_accepts() {
        assert_eq!(
            session_name(&Id::from_string("2pQ4kX9")),
            "molesignal-2pQ4kX9"
        );
        assert_eq!(session_name(&Id::from_string("a/b c")), "molesignal-abc");
        assert_eq!(session_name(&Id::from_string("x".repeat(100))).len(), 64);
    }

    #[test]
    fn endpoints_follow_the_partition() {
        assert_eq!(sts_endpoint(""), "https://sts.us-east-1.amazonaws.com");
        assert_eq!(
            sts_endpoint("eu-west-1"),
            "https://sts.eu-west-1.amazonaws.com"
        );
        assert_eq!(
            sts_endpoint("cn-north-1"),
            "https://sts.cn-north-1.amazonaws.com.cn"
        );
        assert_eq!(
            logs_host("cn-northwest-1"),
            "logs.cn-northwest-1.amazonaws.com.cn"
        );
        assert_eq!(logs_host("us-west-2"), "logs.us-west-2.amazonaws.com");
    }
}
