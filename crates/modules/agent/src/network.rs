// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The organization's Mole Agent network policy: connections created by team members and
//! individuals (Skill repositories, MCP servers) can only access the public network by default;
//! this lists the internal hosts and subnets the organization allows them to access.
//! Organization-level connections created by administrators are not subject to this policy.

use async_trait::async_trait;
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::{Deserialize, Serialize};

/// The maximum number of entries per category.
pub const MAX_POLICY_ENTRIES: usize = 100;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberNetworkPolicy {
    /// Hostnames, including their subdomains.
    pub private_hosts: Vec<String>,
    /// CIDR subnets.
    pub private_cidrs: Vec<String>,
    pub updated_by: Option<Id>,
    pub updated_at: Option<TimestampMicros>,
}

impl MemberNetworkPolicy {
    /// Removes whitespace and duplicates, and validates hostname and subnet syntax.
    pub fn normalized(mut self) -> Result<Self> {
        let mut hosts = Vec::new();

        for host in &self.private_hosts {
            let host = host.trim().trim_start_matches('.').to_ascii_lowercase();

            if host.is_empty() {
                continue;
            }

            if host.contains(['/', ':', '@']) || host.contains(char::is_whitespace) {
                return Err(Error::invalid(format!(
                    "`{host}` is not a host name; write a CIDR range in the other list"
                )));
            }

            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }

        let mut cidrs = Vec::new();

        for cidr in &self.private_cidrs {
            let cidr = cidr.trim();

            if cidr.is_empty() {
                continue;
            }

            let parsed: ipnet::IpNet = cidr
                .parse()
                .map_err(|_| Error::invalid(format!("`{cidr}` is not a CIDR range")))?;

            let text = parsed.trunc().to_string();

            if !cidrs.contains(&text) {
                cidrs.push(text);
            }
        }

        if hosts.len() > MAX_POLICY_ENTRIES || cidrs.len() > MAX_POLICY_ENTRIES {
            return Err(Error::invalid(format!(
                "a network policy lists at most {MAX_POLICY_ENTRIES} hosts and {MAX_POLICY_ENTRIES} ranges"
            )));
        }

        self.private_hosts = hosts;
        self.private_cidrs = cidrs;
        Ok(self)
    }
}

#[async_trait]
pub trait NetworkPolicyStore: Send + Sync {
    /// Returns an empty policy if the organization has not configured one.
    async fn get(&self, org_id: &Id) -> Result<MemberNetworkPolicy>;
    async fn put(&self, org_id: &Id, policy: MemberNetworkPolicy) -> Result<MemberNetworkPolicy>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_and_ranges_are_cleaned_up() {
        let policy = MemberNetworkPolicy {
            private_hosts: vec![
                " .Git.Corp.Example ".into(),
                "git.corp.example".into(),
                "".into(),
            ],
            private_cidrs: vec!["10.20.1.7/16".into()],
            ..MemberNetworkPolicy::default()
        }
        .normalized()
        .unwrap();

        assert_eq!(policy.private_hosts, vec!["git.corp.example"]);
        assert_eq!(policy.private_cidrs, vec!["10.20.0.0/16"]);
    }

    #[test]
    fn a_range_is_not_a_host() {
        let wrong = MemberNetworkPolicy {
            private_hosts: vec!["10.0.0.0/8".into()],
            ..MemberNetworkPolicy::default()
        };

        assert!(wrong.normalized().is_err());

        let bad_range = MemberNetworkPolicy {
            private_cidrs: vec!["10.0.0.0/33".into()],
            ..MemberNetworkPolicy::default()
        };

        assert!(bad_range.normalized().is_err());
    }
}
