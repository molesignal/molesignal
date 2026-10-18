// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织的 Mole Agent 网络策略：团队成员与个人建的连接（Skill 仓库、MCP 服务器）默认只能连
//! 公网，这里列出组织允许他们连的内网主机与网段。管理员建的组织级连接不受它约束。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

/// 每类最多多少条。
pub const MAX_POLICY_ENTRIES: usize = 100;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberNetworkPolicy {
    /// 主机名，含其子域。
    pub private_hosts: Vec<String>,
    /// CIDR 网段。
    pub private_cidrs: Vec<String>,
    pub updated_by: Option<Id>,
    pub updated_at: Option<TimestampMicros>,
}

impl MemberNetworkPolicy {
    /// 去掉空白与重复，校验主机名与网段的写法。
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
    /// 组织还没设过时是空策略。
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
