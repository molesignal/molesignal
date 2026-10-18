// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 替用户连出去的地址怎样算安全：Skill 仓库、MCP 服务器。
//!
//! 每次请求前解析域名，把连接钉在解析出的地址上，防 DNS rebinding；不自动跟随重定向，需要
//! 跟随时每一跳重新校验。管理员建的组织级连接按它自己的私网与名单设置；团队成员和个人建的
//! 连接只能连公网，组织在网络策略里允许的内网主机与网段除外。

use std::net::{IpAddr, SocketAddr};

use ipnet::IpNet;
use url::Url;

use crate::{
    agent::network::MemberNetworkPolicy,
    shared::{Error, Result},
};

/// 一个连接可以连到哪里。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetPolicy {
    /// 管理员配置的组织级连接。
    Admin {
        private_only: bool,
        allowed_domains: Vec<String>,
        allowed_cidrs: Vec<IpNet>,
    },
    /// 团队成员或个人建的连接。
    Member {
        private_hosts: Vec<String>,
        private_cidrs: Vec<IpNet>,
    },
}

impl TargetPolicy {
    /// 组织级、不限私网与公网。
    pub fn admin_any() -> Self {
        Self::Admin {
            private_only: false,
            allowed_domains: Vec::new(),
            allowed_cidrs: Vec::new(),
        }
    }

    /// 团队成员与个人建的连接：公网，加上组织网络策略允许的内网主机与网段。
    pub fn member(policy: &MemberNetworkPolicy) -> Self {
        Self::Member {
            private_hosts: policy.private_hosts.clone(),
            private_cidrs: policy
                .private_cidrs
                .iter()
                .filter_map(|cidr| cidr.parse().ok())
                .collect(),
        }
    }
}

/// 不能当公网目标的地址：私网、回环、链路本地（含云厂商元数据地址）、运营商 NAT、基准测试
/// 网段、文档网段、组播、广播、未指定与 IPv6 唯一本地地址。
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();

            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_documentation()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 198 && (18..=19).contains(&octets[1]))
                || octets[0] >= 240)
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(mapped));
            }

            let first = ip.segments()[0];

            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || first == 0x2001 && ip.segments()[1] == 0x0db8)
        }
    }
}

fn host_matches(host: &str, domains: &[String]) -> bool {
    domains.iter().any(|domain| {
        let domain = domain.trim().trim_start_matches('.').to_ascii_lowercase();
        !domain.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
    })
}

/// 判一个已经解析出的地址能不能连。
pub fn check_address(policy: &TargetPolicy, host: &str, ip: IpAddr) -> Result<()> {
    match policy {
        TargetPolicy::Admin {
            private_only,
            allowed_cidrs,
            ..
        } => {
            if *private_only && is_public_ip(ip) {
                return Err(Error::forbidden(format!(
                    "`{ip}` is outside the private network this connection is limited to"
                )));
            }

            if !allowed_cidrs.is_empty() && !allowed_cidrs.iter().any(|net| net.contains(&ip)) {
                return Err(Error::forbidden(format!(
                    "`{ip}` is outside the allowed address ranges"
                )));
            }

            Ok(())
        }
        TargetPolicy::Member {
            private_hosts,
            private_cidrs,
        } => {
            if is_public_ip(ip)
                || host_matches(host, private_hosts)
                || private_cidrs.iter().any(|net| net.contains(&ip))
            {
                Ok(())
            } else {
                Err(Error::forbidden(format!(
                    "`{host}` resolves to the internal address `{ip}`; an administrator can allow it in the network policy of Mole Agent"
                )))
            }
        }
    }
}

/// 校验地址并解析，返回主机名与可连的地址，用于把连接钉在这些地址上。
pub async fn resolve_target(url: &Url, policy: &TargetPolicy) -> Result<(String, Vec<SocketAddr>)> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::invalid("only http and https addresses are allowed"));
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::invalid("addresses must not carry credentials"));
    }

    let host = url
        .host_str()
        .ok_or_else(|| Error::invalid("the address has no host"))?
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();

    if let TargetPolicy::Admin {
        allowed_domains, ..
    } = policy
        && !allowed_domains.is_empty()
        && !host_matches(&host, allowed_domains)
    {
        return Err(Error::forbidden(format!(
            "`{host}` is not in the allowed domain list"
        )));
    }

    let port = url
        .port_or_known_default()
        .ok_or_else(|| Error::invalid("the address has no port"))?;

    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|error| Error::unavailable(format!("`{host}` could not be resolved: {error}")))?
        .collect();

    if addresses.is_empty() {
        return Err(Error::unavailable(format!(
            "`{host}` resolves to no address"
        )));
    }

    for address in &addresses {
        check_address(policy, &host, address.ip())?;
    }

    Ok((host, addresses))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    #[test]
    fn internal_ranges_are_not_public() {
        for value in [
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "198.18.0.7",
            "0.0.0.0",
            "255.255.255.255",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!is_public_ip(ip(value)), "{value}");
        }

        assert!(is_public_ip(ip("140.82.112.3")));
        assert!(is_public_ip(ip("2606:4700::6810:85e5")));
    }

    #[test]
    fn members_reach_the_internet_and_what_the_organization_allows() {
        let policy = TargetPolicy::Member {
            private_hosts: vec!["git.corp.example".into()],
            private_cidrs: vec!["10.20.0.0/16".parse().unwrap()],
        };

        assert!(check_address(&policy, "github.com", ip("140.82.112.3")).is_ok());
        assert!(check_address(&policy, "git.corp.example", ip("10.1.1.1")).is_ok());
        assert!(check_address(&policy, "ci.git.corp.example", ip("10.1.1.1")).is_ok());
        assert!(check_address(&policy, "other.internal", ip("10.20.3.4")).is_ok());
        assert!(check_address(&policy, "metadata", ip("169.254.169.254")).is_err());
        assert!(check_address(&policy, "evil.example", ip("10.1.1.1")).is_err());
    }

    #[test]
    fn administrators_keep_their_own_limits() {
        let private_only = TargetPolicy::Admin {
            private_only: true,
            allowed_domains: Vec::new(),
            allowed_cidrs: Vec::new(),
        };

        assert!(check_address(&private_only, "mcp", ip("10.0.0.5")).is_ok());
        assert!(check_address(&private_only, "mcp", ip("140.82.112.3")).is_err());
        assert!(check_address(&TargetPolicy::admin_any(), "x", ip("10.0.0.5")).is_ok());
    }
}
