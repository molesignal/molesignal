// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ICMP and DNS settings an Agent can run.

use super::assertions::validate_assertions;
use crate::{
    domain::synthetics::{DnsSpec, IcmpSpec, MonitorKind},
    shared::{Error, Result},
};

const MAX_PINGS: u8 = 20;
const MAX_PING_INTERVAL_MILLIS: u32 = 10_000;

const DNS_RECORD_TYPES: &[&str] = &[
    "A", "AAAA", "CAA", "CNAME", "MX", "NS", "PTR", "SOA", "SRV", "TXT",
];

const DNS_RESPONSE_CODES: &[&str] = &[
    "NOERROR", "FORMERR", "SERVFAIL", "NXDOMAIN", "NOTIMP", "REFUSED",
];

pub(super) fn validate_icmp(spec: &IcmpSpec, timeout_millis: u32) -> Result<()> {
    if spec.host.trim().is_empty() {
        return Err(Error::invalid("ICMP host is required"));
    }

    if spec.count == 0 || spec.count > MAX_PINGS {
        return Err(Error::invalid(format!("ICMP sends 1 to {MAX_PINGS} pings")));
    }

    if spec.interval_millis > MAX_PING_INTERVAL_MILLIS {
        return Err(Error::invalid("ICMP pings are at most 10 seconds apart"));
    }

    if !(0.0..=1.0).contains(&spec.max_packet_loss_ratio) {
        return Err(Error::invalid("ICMP packet loss limit is between 0 and 1"));
    }

    if spec.max_mean_rtt_millis == Some(0) {
        return Err(Error::invalid("ICMP latency limit must be positive"));
    }

    // The pings go out one interval apart, and all of them within the timeout.
    let spread = u64::from(spec.count.saturating_sub(1)) * u64::from(spec.interval_millis);

    if spread >= u64::from(timeout_millis) {
        return Err(Error::invalid(
            "ICMP pings take longer than the timeout allows: send fewer, closer together, or raise the timeout",
        ));
    }

    Ok(())
}

pub(super) fn validate_dns(spec: &DnsSpec) -> Result<()> {
    if spec.name.trim().is_empty() {
        return Err(Error::invalid("DNS name is required"));
    }

    if !DNS_RECORD_TYPES.contains(&spec.record_type.to_ascii_uppercase().as_str()) {
        return Err(Error::invalid(format!(
            "unsupported DNS record type `{}`",
            spec.record_type
        )));
    }

    if let Some(code) = spec.expected_rcode.as_deref().map(str::trim)
        && !code.is_empty()
        && !DNS_RESPONSE_CODES.contains(&code.to_ascii_uppercase().as_str())
    {
        return Err(Error::invalid(format!(
            "unknown DNS response code `{code}`"
        )));
    }

    if let Some(resolver) = spec.resolver.as_deref().map(str::trim)
        && (resolver.contains('/') || resolver.contains(' '))
    {
        return Err(Error::invalid(
            "DNS resolver is a host or an address, with an optional port",
        ));
    }

    validate_assertions(MonitorKind::Dns, &spec.assertions)
}

#[cfg(test)]
mod tests {
    use super::{validate_dns, validate_icmp};
    use crate::domain::synthetics::{DnsSpec, IcmpSpec};

    fn icmp(count: u8, interval_millis: u32) -> IcmpSpec {
        IcmpSpec {
            host: "192.0.2.1".into(),
            count,
            interval_millis,
            max_packet_loss_ratio: 0.2,
            max_mean_rtt_millis: None,
        }
    }

    #[test]
    fn pings_fit_in_the_timeout() {
        assert!(validate_icmp(&icmp(5, 1_000), 10_000).is_ok());
        assert!(validate_icmp(&icmp(5, 3_000), 10_000).is_err());
        assert!(validate_icmp(&icmp(0, 100), 10_000).is_err());
        assert!(validate_icmp(&icmp(21, 100), 10_000).is_err());
    }

    #[test]
    fn dns_names_a_known_record_type_and_code() {
        let spec = |record_type: &str, rcode: Option<&str>| DnsSpec {
            name: "example.com".into(),
            record_type: record_type.into(),
            resolver: Some("1.1.1.1:53".into()),
            require_dnssec: false,
            expected_values: Vec::new(),
            expected_rcode: rcode.map(Into::into),
            assertions: Vec::new(),
        };

        assert!(validate_dns(&spec("mx", Some("nxdomain"))).is_ok());
        assert!(validate_dns(&spec("ANY", None)).is_err());
        assert!(validate_dns(&spec("A", Some("GONE"))).is_err());
    }
}
