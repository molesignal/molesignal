// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr as _,
};

use anyhow::{Result, bail};
use ipnet::IpNet;

use crate::protocol::v1::EgressPolicy;

pub struct EgressGuard {
    policy: EgressPolicy,
    allowed: Vec<IpNet>,
    denied: Vec<IpNet>,
}

impl EgressGuard {
    pub fn new(policy: Option<EgressPolicy>) -> Result<Self> {
        let policy = policy.unwrap_or_default();
        let allowed = parse_cidrs(&policy.allowed_cidrs)?;
        let denied = parse_cidrs(&policy.denied_cidrs)?;
        Ok(Self {
            policy,
            allowed,
            denied,
        })
    }

    pub async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
        self.validate_domain(host)?;
        self.validate_port(port)?;
        let resolved = tokio::net::lookup_host((host, port)).await?;
        let mut approved = Vec::new();
        for address in resolved {
            if self.ip_allowed(address.ip()) {
                approved.push(address);
            }
        }
        approved.sort_unstable();
        approved.dedup();
        if approved.is_empty() {
            bail!("egress policy rejected every resolved address for {host}");
        }
        Ok(approved)
    }

    fn validate_domain(&self, host: &str) -> Result<()> {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() || !host.is_ascii() {
            bail!("target hostname is invalid");
        }
        if self
            .policy
            .denied_domains
            .iter()
            .any(|pattern| domain_matches(&host, pattern))
        {
            bail!("target hostname is denied by egress policy");
        }
        if !self.policy.allowed_domains.is_empty()
            && !self
                .policy
                .allowed_domains
                .iter()
                .any(|pattern| domain_matches(&host, pattern))
        {
            bail!("target hostname is outside the egress allowlist");
        }
        Ok(())
    }

    fn validate_port(&self, port: u16) -> Result<()> {
        if port == 0 {
            bail!("target port is invalid");
        }
        if !self.policy.allowed_ports.is_empty()
            && !self.policy.allowed_ports.contains(&(u32::from(port)))
        {
            bail!("target port is outside the egress allowlist");
        }
        Ok(())
    }

    fn ip_allowed(&self, ip: IpAddr) -> bool {
        if self.denied.iter().any(|network| network.contains(&ip)) {
            return false;
        }
        if !self.allowed.is_empty() && !self.allowed.iter().any(|network| network.contains(&ip)) {
            return false;
        }
        if is_loopback(ip) && !self.policy.allow_loopback {
            return false;
        }
        if is_private_or_special(ip) && !is_loopback(ip) && !self.policy.allow_private_networks {
            return false;
        }
        true
    }
}

fn parse_cidrs(values: &[String]) -> Result<Vec<IpNet>> {
    values
        .iter()
        .map(|value| IpNet::from_str(value).map_err(Into::into))
        .collect()
}

fn domain_matches(host: &str, pattern: &str) -> bool {
    let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
    if let Some(suffix) = pattern.strip_prefix("*.") {
        host != suffix && host.ends_with(&format!(".{suffix}"))
    } else {
        host == pattern
    }
}

fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => value.is_loopback(),
        IpAddr::V6(value) => value.is_loopback(),
    }
}

fn is_private_or_special(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => ipv4_private_or_special(value),
        IpAddr::V6(value) => ipv6_private_or_special(value),
    }
}

fn ipv4_private_or_special(value: Ipv4Addr) -> bool {
    let octets = value.octets();
    value.is_private()
        || value.is_loopback()
        || value.is_link_local()
        || value.is_unspecified()
        || value.is_multicast()
        || value == Ipv4Addr::BROADCAST
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        || octets[0] >= 240
}

fn ipv6_private_or_special(value: Ipv6Addr) -> bool {
    let segments = value.segments();
    value.is_loopback()
        || value.is_unspecified()
        || value.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || value.to_ipv4_mapped().is_some_and(ipv4_private_or_special)
}
