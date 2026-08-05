// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM 客户端 IP 识别：配置来自数据库，热路径只读取编译后的内存快照。

use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};

use axum::http::{HeaderMap, HeaderName};
use ipnet::IpNet;
use parking_lot::RwLock;

use crate::{
    domain::iam::{ClientIpMode, ClientIpResolverSettings},
    shared::{Error, Result},
};

const MAX_TRUSTED_PROXY_CIDRS: usize = 64;
const MAX_HEADER_NAME_LENGTH: usize = 128;
const MAX_HEADER_VALUE_LENGTH: usize = 4096;
const MAX_CHAIN_LENGTH: u16 = 64;

#[derive(Clone)]
pub(crate) struct ClientIpResolverHandle {
    current: Arc<RwLock<Arc<ClientIpResolver>>>,
}

impl ClientIpResolverHandle {
    pub(crate) fn peer() -> Self {
        Self::new(&ClientIpResolverSettings::default())
            .expect("the built-in peer client IP resolver must be valid")
    }

    pub(crate) fn new(settings: &ClientIpResolverSettings) -> Result<Self> {
        let normalized = normalize_settings(settings.clone())?;
        Ok(Self {
            current: Arc::new(RwLock::new(Arc::new(ClientIpResolver::compile(
                &normalized,
            )?))),
        })
    }

    pub(crate) fn replace(&self, settings: &ClientIpResolverSettings) -> Result<()> {
        let normalized = normalize_settings(settings.clone())?;
        let resolver = Arc::new(ClientIpResolver::compile(&normalized)?);
        *self.current.write() = resolver;
        Ok(())
    }

    pub(crate) fn resolve(&self, headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<IpAddr> {
        self.current.read().resolve(headers, peer)
    }
}

pub(crate) fn normalize_settings(
    mut settings: ClientIpResolverSettings,
) -> Result<ClientIpResolverSettings> {
    if !(1..=MAX_CHAIN_LENGTH).contains(&settings.max_chain_length) {
        return Err(Error::invalid(format!(
            "max_chain_length must be between 1 and {MAX_CHAIN_LENGTH}"
        )));
    }

    if settings.mode == ClientIpMode::Peer {
        settings.header_name.clear();
        settings.trusted_proxy_cidrs.clear();
        return Ok(settings);
    }

    let raw_header = settings.header_name.trim();
    if raw_header.is_empty() || raw_header.len() > MAX_HEADER_NAME_LENGTH {
        return Err(Error::invalid(format!(
            "header_name must contain 1 to {MAX_HEADER_NAME_LENGTH} characters"
        )));
    }
    let header = HeaderName::from_bytes(raw_header.as_bytes())
        .map_err(|_| Error::invalid("header_name is not a valid HTTP header name"))?;
    if is_sensitive_header(header.as_str()) {
        return Err(Error::invalid(
            "header_name cannot reference a sensitive header",
        ));
    }
    settings.header_name = header.as_str().to_string();

    if settings.trusted_proxy_cidrs.is_empty() {
        return Err(Error::invalid(
            "trusted_proxy_cidrs is required for header and forwarded_chain modes",
        ));
    }
    if settings.trusted_proxy_cidrs.len() > MAX_TRUSTED_PROXY_CIDRS {
        return Err(Error::invalid(format!(
            "trusted_proxy_cidrs supports at most {MAX_TRUSTED_PROXY_CIDRS} entries"
        )));
    }

    let mut seen = HashSet::new();
    let mut normalized_cidrs = Vec::with_capacity(settings.trusted_proxy_cidrs.len());
    for raw in &settings.trusted_proxy_cidrs {
        let network = raw
            .trim()
            .parse::<IpNet>()
            .map_err(|_| Error::invalid(format!("invalid trusted proxy CIDR: {raw}")))?;
        let canonical = network.trunc().to_string();
        if seen.insert(canonical.clone()) {
            normalized_cidrs.push(canonical);
        }
    }
    settings.trusted_proxy_cidrs = normalized_cidrs;
    Ok(settings)
}

struct ClientIpResolver {
    mode: ClientIpMode,
    header_name: Option<HeaderName>,
    trusted_proxies: Vec<IpNet>,
    fallback_to_peer: bool,
    allow_private_client_ips: bool,
    max_chain_length: usize,
}

impl ClientIpResolver {
    fn compile(settings: &ClientIpResolverSettings) -> Result<Self> {
        let header_name = if settings.mode == ClientIpMode::Peer {
            None
        } else {
            Some(
                HeaderName::from_bytes(settings.header_name.as_bytes())
                    .map_err(|_| Error::invalid("header_name is not a valid HTTP header name"))?,
            )
        };
        let trusted_proxies = settings
            .trusted_proxy_cidrs
            .iter()
            .map(|cidr| {
                cidr.parse::<IpNet>()
                    .map_err(|_| Error::invalid(format!("invalid trusted proxy CIDR: {cidr}")))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            mode: settings.mode,
            header_name,
            trusted_proxies,
            fallback_to_peer: settings.fallback_to_peer,
            allow_private_client_ips: settings.allow_private_client_ips,
            max_chain_length: usize::from(settings.max_chain_length),
        })
    }

    fn resolve(&self, headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<IpAddr> {
        let peer_ip = peer.map(|address| normalize_ip(address.ip()));
        if self.mode == ClientIpMode::Peer {
            return peer_ip;
        }
        let trusted_peer = peer_ip.is_some_and(|ip| {
            self.trusted_proxies
                .iter()
                .any(|network| network.contains(&ip))
        });
        if !trusted_peer {
            return self.fallback(peer_ip);
        }

        let header_name = self.header_name.as_ref()?;
        let candidate = match self.mode {
            ClientIpMode::Peer => None,
            ClientIpMode::Header => single_header_ip(headers, header_name),
            ClientIpMode::ForwardedChain => self.forwarded_chain_ip(headers, header_name),
        };
        candidate
            .filter(|ip| self.allow_private_client_ips || is_public_client_ip(*ip))
            .or_else(|| self.fallback(peer_ip))
    }

    fn forwarded_chain_ip(&self, headers: &HeaderMap, header_name: &HeaderName) -> Option<IpAddr> {
        let mut chain = Vec::new();
        for value in headers.get_all(header_name).iter() {
            let value = value.to_str().ok()?;
            if value.len() > MAX_HEADER_VALUE_LENGTH {
                return None;
            }
            for part in value.split(',') {
                if chain.len() >= self.max_chain_length {
                    return None;
                }
                chain.push(parse_bare_ip(part)?);
            }
        }
        if chain.is_empty() {
            return None;
        }
        chain.into_iter().rev().find(|ip| {
            !self
                .trusted_proxies
                .iter()
                .any(|network| network.contains(ip))
        })
    }

    fn fallback(&self, peer_ip: Option<IpAddr>) -> Option<IpAddr> {
        self.fallback_to_peer.then_some(peer_ip).flatten()
    }
}

fn single_header_ip(headers: &HeaderMap, header_name: &HeaderName) -> Option<IpAddr> {
    let mut values = headers.get_all(header_name).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let value = value.to_str().ok()?;
    if value.len() > 64 || value.contains(',') {
        return None;
    }
    parse_bare_ip(value)
}

fn parse_bare_ip(value: &str) -> Option<IpAddr> {
    value.trim().parse::<IpAddr>().ok().map(normalize_ip)
}

fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(ipv6) => ipv6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ipv6)),
        other => other,
    }
}

fn is_sensitive_header(name: &str) -> bool {
    matches!(
        name,
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "x-auth-token"
            | "x-forwarded-client-cert"
    )
}

fn is_public_client_ip(ip: IpAddr) -> bool {
    match normalize_ip(ip) {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (18..=19).contains(&b))
        || a >= 240)
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] & 0xffc0) == 0xfec0
        || (segments[0] == 0x2001 && segments[1] == 0x0db8))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(mode: ClientIpMode) -> ClientIpResolverSettings {
        ClientIpResolverSettings {
            mode,
            header_name: "X-Forwarded-For".into(),
            trusted_proxy_cidrs: vec!["10.0.0.0/8".into(), "2001:db8:1::/48".into()],
            ..ClientIpResolverSettings::default()
        }
    }

    fn peer(ip: &str) -> Option<SocketAddr> {
        Some(format!("{ip}:443").parse().unwrap())
    }

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", value.parse().unwrap());
        headers
    }

    #[test]
    fn peer_mode_ignores_forwarding_headers() {
        let resolver = ClientIpResolverHandle::peer();
        assert_eq!(
            resolver.resolve(&headers("1.1.1.1"), peer("127.0.0.1")),
            Some("127.0.0.1".parse().unwrap())
        );
    }

    #[test]
    fn untrusted_peer_cannot_spoof_header() {
        let resolver = ClientIpResolverHandle::new(&settings(ClientIpMode::Header)).unwrap();
        assert_eq!(
            resolver.resolve(&headers("1.1.1.1"), peer("9.9.9.9")),
            Some("9.9.9.9".parse().unwrap())
        );
    }

    #[test]
    fn trusted_peer_can_supply_single_ip() {
        let resolver = ClientIpResolverHandle::new(&settings(ClientIpMode::Header)).unwrap();
        assert_eq!(
            resolver.resolve(&headers("1.1.1.1"), peer("10.0.2.30")),
            Some("1.1.1.1".parse().unwrap())
        );
    }

    #[test]
    fn forwarded_chain_uses_rightmost_untrusted_address() {
        let resolver =
            ClientIpResolverHandle::new(&settings(ClientIpMode::ForwardedChain)).unwrap();
        assert_eq!(
            resolver.resolve(
                &headers("8.8.8.8, 1.1.1.1, 10.0.1.20, 10.0.2.30"),
                peer("10.0.3.40")
            ),
            Some("1.1.1.1".parse().unwrap())
        );
    }

    #[test]
    fn invalid_or_too_long_chain_falls_back_to_peer() {
        let mut config = settings(ClientIpMode::ForwardedChain);
        config.max_chain_length = 2;
        let resolver = ClientIpResolverHandle::new(&config).unwrap();
        assert_eq!(
            resolver.resolve(&headers("8.8.8.8, 1.1.1.1, 10.0.1.20"), peer("10.0.3.40")),
            Some("10.0.3.40".parse().unwrap())
        );
        assert_eq!(
            resolver.resolve(&headers("not-an-ip"), peer("10.0.3.40")),
            Some("10.0.3.40".parse().unwrap())
        );
    }

    #[test]
    fn private_header_address_requires_explicit_opt_in() {
        let config = settings(ClientIpMode::Header);
        let resolver = ClientIpResolverHandle::new(&config).unwrap();
        assert_eq!(
            resolver.resolve(&headers("192.168.1.8"), peer("10.0.3.40")),
            Some("10.0.3.40".parse().unwrap())
        );

        let mut allowed = config;
        allowed.allow_private_client_ips = true;
        let resolver = ClientIpResolverHandle::new(&allowed).unwrap();
        assert_eq!(
            resolver.resolve(&headers("192.168.1.8"), peer("10.0.3.40")),
            Some("192.168.1.8".parse().unwrap())
        );
    }

    #[test]
    fn validation_rejects_sensitive_header() {
        let mut config = settings(ClientIpMode::Header);
        config.header_name = "Authorization".into();
        assert!(normalize_settings(config).is_err());
    }

    #[test]
    fn wildcard_proxy_networks_are_accepted() {
        let mut config = settings(ClientIpMode::Header);
        config.trusted_proxy_cidrs = vec!["0.0.0.0/0".into(), "::/0".into()];
        let normalized = normalize_settings(config).unwrap();
        assert_eq!(
            normalized.trusted_proxy_cidrs,
            vec!["0.0.0.0/0".to_string(), "::/0".to_string()]
        );

        let resolver = ClientIpResolverHandle::new(&normalized).unwrap();
        assert_eq!(
            resolver.resolve(&headers("1.1.1.1"), peer("9.9.9.9")),
            Some("1.1.1.1".parse().unwrap())
        );
    }

    #[test]
    fn validation_normalizes_header_and_deduplicates_networks() {
        let mut config = settings(ClientIpMode::Header);
        config.trusted_proxy_cidrs = vec!["10.2.3.4/8".into(), "10.0.0.0/8".into()];
        let normalized = normalize_settings(config).unwrap();
        assert_eq!(normalized.header_name, "x-forwarded-for");
        assert_eq!(
            normalized.trusted_proxy_cidrs,
            vec!["10.0.0.0/8".to_string()]
        );
    }

    #[test]
    fn fallback_can_be_disabled() {
        let mut config = settings(ClientIpMode::Header);
        config.fallback_to_peer = false;
        let resolver = ClientIpResolverHandle::new(&config).unwrap();
        assert_eq!(resolver.resolve(&headers("1.1.1.1"), peer("9.9.9.9")), None);
    }

    #[test]
    fn peer_mode_clears_inapplicable_proxy_fields() {
        let normalized = normalize_settings(settings(ClientIpMode::Peer)).unwrap();
        assert!(normalized.header_name.is_empty());
        assert!(normalized.trusted_proxy_cidrs.is_empty());
    }
}
