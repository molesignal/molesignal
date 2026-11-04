// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! DNS: the answers a resolver gives for a name, the code it answers with, and whether the
//! answers are signed.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    str::FromStr as _,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow};
use hickory_resolver::{
    Resolver, TokioResolver,
    config::{NameServerConfig, ResolveHosts, ResolverConfig, ResolverOpts},
    lookup::Lookup,
    net::{DnsError, NetError, NoRecords, runtime::TokioRuntimeProvider},
    proto::{dnssec::Proof, op::ResponseCode, rr::RecordType},
};

use super::super::{
    AttemptOutcome, ExecutionContext, assertions, failure,
    security::EgressGuard,
    timing::{Phase, PhaseClock},
};
use crate::protocol::v1::{self as wire, DnsSpec, ProbeOutcome};

const DNS_PORT: u16 = 53;

/// What came back for the query.
struct Answer {
    rcode: String,
    values: Vec<String>,
    /// How sure the resolver is of each answer, when it was asked to check signatures.
    proofs: Vec<Proof>,
}

pub(in super::super) async fn execute(
    task: &wire::ProbeTask,
    spec: &DnsSpec,
    context: &ExecutionContext,
    phases: &PhaseClock,
) -> Result<AttemptOutcome> {
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    guard.allows_name(&spec.name)?;

    let record_type = RecordType::from_str(&spec.record_type.to_ascii_uppercase())
        .map_err(|_| anyhow!("unsupported DNS record type `{}`", spec.record_type))?;

    let timeout = Duration::from_millis(u64::from(task.timeout_millis.max(1)));
    let (resolver, nameserver) = resolver(&guard, spec, timeout, phases).await?;
    let started = Instant::now();
    let lookup = tokio::time::timeout(timeout, resolver.lookup(spec.name.as_str(), record_type));

    let answer = match phases.during(Phase::Dns, lookup).await {
        Ok(lookup) => answer(lookup)?,
        Err(_) => return Err(failure::target("timeout", "DNS query timed out")),
    };

    let elapsed = started.elapsed();

    let read = |source: &str| match source {
        "answers" => Some(answer.values.join(", ")),
        "answer_count" => Some(answer.values.len().to_string()),
        "rcode" => Some(answer.rcode.clone()),
        "duration_ms" => Some(elapsed.as_millis().to_string()),
        _ => None,
    };

    let assertions = assertions::evaluate_all(&spec.assertions, read, context)?;
    let (expected, category, message) = expectation(spec, &answer);

    let outcome = match (expected, assertions::outcome(&assertions)) {
        (ProbeOutcome::Failing, _) | (_, ProbeOutcome::Failing) => ProbeOutcome::Failing,
        (_, judged) => judged,
    };

    let mut metadata = HashMap::from([
        ("answers".into(), answer.values.join(", ")),
        ("rcode".into(), answer.rcode.clone()),
        ("resolver".into(), nameserver),
    ]);

    if spec.require_dnssec {
        metadata.insert("dnssec".into(), proof_summary(&answer.proofs).into());
    }

    Ok(AttemptOutcome {
        outcome,
        assertions,
        response_excerpt: Vec::new(),
        error_category: category.into(),
        error_message: message,
        metadata,
        evidence: Vec::new(),
        artifacts: Vec::new(),
    })
}

/// The resolver the check asks: the one it names, or the system's.
async fn resolver(
    guard: &EgressGuard,
    spec: &DnsSpec,
    timeout: Duration,
    phases: &PhaseClock,
) -> Result<(TokioResolver, String)> {
    match spec.resolver.as_deref().map(str::trim) {
        Some(named) if !named.is_empty() => Ok((
            named_resolver(guard, named, timeout, spec.require_dnssec, phases).await?,
            named.to_string(),
        )),
        _ => {
            let mut builder =
                TokioResolver::builder_tokio().context("cannot read the system DNS settings")?;

            configure(builder.options_mut(), timeout, spec.require_dnssec);
            Ok((builder.build()?, "system".into()))
        }
    }
}

/// A resolver that asks the DNS server `named`. The server is an address the check reaches out
/// to, so it passes the egress policy like any target.
pub(in crate::executor) async fn named_resolver(
    guard: &EgressGuard,
    named: &str,
    timeout: Duration,
    validate: bool,
    phases: &PhaseClock,
) -> Result<TokioResolver> {
    let (host, port) = split_resolver(named)?;
    let address = guard.resolve(&host, port, phases).await?.remove(0);
    let mut nameserver = NameServerConfig::udp_and_tcp(address.ip());

    for connection in &mut nameserver.connections {
        connection.port = address.port();
    }

    let config = ResolverConfig::from_name_servers(vec![nameserver]);
    let mut builder = Resolver::builder_with_config(config, TokioRuntimeProvider::default());

    configure(builder.options_mut(), timeout, validate);
    Ok(builder.build()?)
}

/// A check asks the DNS, not the hosts file, once, within the time the attempt has. A retry is
/// the task's to make, so that every attempt is reported.
fn configure(options: &mut ResolverOpts, timeout: Duration, validate: bool) {
    options.use_hosts_file = ResolveHosts::Never;
    options.validate = validate;
    options.timeout = timeout;
    options.attempts = 1;
}

/// `host`, `host:port`, `ip`, `ip:port` or `[ipv6]:port`.
fn split_resolver(named: &str) -> Result<(String, u16)> {
    if let Ok(address) = named.parse::<SocketAddr>() {
        return Ok((address.ip().to_string(), address.port()));
    }

    if let Ok(address) = named.trim_matches(['[', ']']).parse::<IpAddr>() {
        return Ok((address.to_string(), DNS_PORT));
    }

    match named.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            let port = port
                .parse::<u16>()
                .map_err(|_| anyhow!("DNS resolver port `{port}` is invalid"))?;

            Ok((host.to_string(), port))
        }
        _ => Ok((named.to_string(), DNS_PORT)),
    }
}

/// The answer to the query. A name that does not exist and an error code are answers too, which
/// the check may expect; a resolver that cannot be reached, or does not answer, fails the check.
fn answer(lookup: std::result::Result<Lookup, NetError>) -> Result<Answer> {
    match lookup {
        Ok(lookup) => Ok(Answer {
            rcode: "NOERROR".into(),
            values: lookup
                .answers()
                .iter()
                .map(|record| record.data.to_string())
                .collect(),
            proofs: lookup.answers().iter().map(|record| record.proof).collect(),
        }),
        Err(NetError::Dns(DnsError::NoRecordsFound(NoRecords { response_code, .. }))) => {
            Ok(empty(response_code))
        }
        Err(NetError::Dns(DnsError::ResponseCode(code))) => Ok(empty(code)),
        Err(NetError::Dns(DnsError::DnssecBogus)) => Err(failure::target(
            "dnssec_bogus",
            "the DNSSEC signatures of the answer do not validate",
        )),
        Err(error) => Err(failure::target(
            "dns_failed",
            format!("DNS query failed: {error}"),
        )),
    }
}

fn empty(code: ResponseCode) -> Answer {
    Answer {
        rcode: mnemonic(code),
        values: Vec::new(),
        proofs: Vec::new(),
    }
}

/// Whether the answer is the one the check expects, and why not when it is not.
fn expectation(spec: &DnsSpec, answer: &Answer) -> (ProbeOutcome, &'static str, String) {
    let failing = |category, message| (ProbeOutcome::Failing, category, message);

    match spec.expected_rcode.as_deref().map(str::trim) {
        Some(expected) if !expected.is_empty() => {
            if !answer.rcode.eq_ignore_ascii_case(expected) {
                return failing(
                    "dns_rcode_mismatch",
                    format!(
                        "DNS answered {}, not {}",
                        answer.rcode,
                        expected.to_uppercase()
                    ),
                );
            }
        }
        _ => {
            if answer.rcode != "NOERROR" {
                return failing(
                    "dns_rcode_mismatch",
                    format!("DNS answered {}", answer.rcode),
                );
            }

            if answer.values.is_empty() {
                return failing(
                    "dns_no_answer",
                    format!("DNS has no {} record for {}", spec.record_type, spec.name),
                );
            }
        }
    }

    if let Some(missing) = spec.expected_values.iter().find(|expected| {
        !answer
            .values
            .iter()
            .any(|value| matches_answer(expected, value))
    }) {
        return failing(
            "dns_answer_mismatch",
            format!("DNS answer does not contain `{missing}`"),
        );
    }

    if spec.require_dnssec && !answer.proofs.iter().all(|proof| *proof == Proof::Secure) {
        return failing(
            "dnssec_insecure",
            "DNS answer is not signed with a chain of trust the resolver can validate".into(),
        );
    }

    (ProbeOutcome::Healthy, "", String::new())
}

/// An expected value matches an answer written the way people write it: without the final dot,
/// in any case, a TXT record without its quotes, and an MX or SRV record by its host alone.
fn matches_answer(expected: &str, answer: &str) -> bool {
    let normal = |value: &str| {
        value
            .trim()
            .trim_matches('"')
            .trim_end_matches('.')
            .to_ascii_lowercase()
    };

    let (expected, answer) = (normal(expected), normal(answer));

    expected == answer
        || answer
            .rsplit_once(' ')
            .is_some_and(|(_, host)| host == expected)
}

fn proof_summary(proofs: &[Proof]) -> &'static str {
    if proofs.contains(&Proof::Bogus) {
        "bogus"
    } else if !proofs.is_empty() && proofs.iter().all(|proof| *proof == Proof::Secure) {
        "secure"
    } else if proofs.contains(&Proof::Insecure) {
        "insecure"
    } else {
        "indeterminate"
    }
}

/// The name a response code goes by in DNS tools: NOERROR, NXDOMAIN, SERVFAIL.
fn mnemonic(code: ResponseCode) -> String {
    match code {
        ResponseCode::NoError => "NOERROR".into(),
        ResponseCode::FormErr => "FORMERR".into(),
        ResponseCode::ServFail => "SERVFAIL".into(),
        ResponseCode::NXDomain => "NXDOMAIN".into(),
        ResponseCode::NotImp => "NOTIMP".into(),
        ResponseCode::Refused => "REFUSED".into(),
        ResponseCode::YXDomain => "YXDOMAIN".into(),
        ResponseCode::YXRRSet => "YXRRSET".into(),
        ResponseCode::NXRRSet => "NXRRSET".into(),
        ResponseCode::NotAuth => "NOTAUTH".into(),
        ResponseCode::NotZone => "NOTZONE".into(),
        other => u16::from(other).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Answer, expectation, matches_answer, split_resolver};
    use crate::protocol::v1::{DnsSpec, ProbeOutcome};

    fn answer(rcode: &str, values: &[&str]) -> Answer {
        Answer {
            rcode: rcode.into(),
            values: values.iter().map(|value| (*value).to_string()).collect(),
            proofs: Vec::new(),
        }
    }

    #[test]
    fn values_match_as_people_write_them() {
        assert!(matches_answer("mail.example.com", "10 mail.example.com."));
        assert!(matches_answer(
            "10 Mail.Example.com",
            "10 mail.example.com."
        ));
        assert!(matches_answer("v=spf1 -all", "\"v=spf1 -all\""));
        assert!(!matches_answer("mx.example.com", "10 mail.example.com."));
    }

    #[test]
    fn a_name_that_must_not_exist_can_be_expected() {
        let spec = DnsSpec {
            name: "gone.example.com".into(),
            record_type: "A".into(),
            expected_rcode: Some("nxdomain".into()),
            ..Default::default()
        };

        assert_eq!(
            expectation(&spec, &answer("NXDOMAIN", &[])).0,
            ProbeOutcome::Healthy
        );
        assert_eq!(
            expectation(&spec, &answer("NOERROR", &["192.0.2.1"])).0,
            ProbeOutcome::Failing
        );
    }

    #[test]
    fn without_an_expected_code_an_empty_answer_fails() {
        let spec = DnsSpec {
            name: "example.com".into(),
            record_type: "MX".into(),
            ..Default::default()
        };

        assert_eq!(
            expectation(&spec, &answer("NOERROR", &[])).1,
            "dns_no_answer"
        );
        assert_eq!(
            expectation(&spec, &answer("SERVFAIL", &[])).1,
            "dns_rcode_mismatch"
        );
    }

    #[test]
    fn resolvers_are_written_with_or_without_a_port() {
        assert_eq!(split_resolver("1.1.1.1").unwrap(), ("1.1.1.1".into(), 53));
        assert_eq!(
            split_resolver("1.1.1.1:5353").unwrap(),
            ("1.1.1.1".into(), 5353)
        );
        assert_eq!(
            split_resolver("[2606:4700::1111]:53").unwrap(),
            ("2606:4700::1111".into(), 53)
        );
        assert_eq!(
            split_resolver("2606:4700::1111").unwrap(),
            ("2606:4700::1111".into(), 53)
        );
        assert_eq!(
            split_resolver("dns.example.net").unwrap(),
            ("dns.example.net".into(), 53)
        );
    }
}
