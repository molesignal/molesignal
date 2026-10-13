// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result, anyhow};
use regex::Regex;
use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _},
    net::TcpStream,
};
use tokio_rustls::TlsConnector;
use tonic::transport::{ClientTlsConfig, Endpoint};
use x509_parser::{extensions::GeneralName, parse_x509_certificate};

use super::{
    AttemptOutcome, ExecutionContext,
    security::EgressGuard,
    timing::{Phase, PhaseClock},
    unknown,
};
use crate::protocol::v1::{
    self as wire, DnsSpec, GrpcSpec, IcmpSpec, ProbeOutcome, TcpSpec, TlsSpec, grpc_spec,
};

mod endpoint;

use endpoint::GrpcEndpoint;

trait ProbeIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> ProbeIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub(super) async fn execute_tcp(
    task: &wire::ProbeTask,
    spec: &TcpSpec,
    context: &ExecutionContext,
    phases: &PhaseClock,
) -> Result<AttemptOutcome> {
    let host = context.resolve(
        spec.host
            .as_ref()
            .ok_or_else(|| anyhow!("TCP host is required"))?,
    )?;

    let port = u16::try_from(spec.port).context("TCP port is out of range")?;
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let address = guard.resolve(&host, port, phases).await?.remove(0);
    let timeout = Duration::from_millis(u64::from(task.timeout_millis.max(1)));
    let started = Instant::now();
    let tcp = connect_tcp(address, timeout, phases).await?;

    let mut stream: Box<dyn ProbeIo> = if spec.use_tls {
        let server_name = if spec.server_name.is_empty() {
            &host
        } else {
            &spec.server_name
        };

        Box::new(connect_tls(tcp, server_name, timeout, phases).await?)
    } else {
        Box::new(tcp)
    };

    let reads_reply = spec.expect_regex.is_some() || !spec.assertions.is_empty();

    // The wait for a reply starts with the connection ready, before the payload goes out.
    let waiting = reads_reply.then(|| phases.enter(Phase::FirstByte));

    if let Some(send) = &spec.send {
        stream.write_all(context.resolve(send)?.as_bytes()).await?;
        stream.flush().await?;
    }

    let mut response = vec![0u8; 64 * 1024];

    let length = if reads_reply {
        tokio::time::timeout(timeout, stream.read(&mut response)).await??
    } else {
        0
    };

    drop(waiting);
    response.truncate(length);
    let text = String::from_utf8_lossy(&response);

    let matched = spec
        .expect_regex
        .as_ref()
        .map(|pattern| Regex::new(pattern).map(|regex| regex.is_match(&text)))
        .transpose()?
        .unwrap_or(true);

    let outcome = if matched {
        ProbeOutcome::Healthy
    } else {
        ProbeOutcome::Failing
    };

    Ok(AttemptOutcome {
        outcome,
        assertions: Vec::new(),
        response_excerpt: if matched {
            Vec::new()
        } else {
            context.redact(&text).into_bytes()
        },
        error_category: if matched {
            String::new()
        } else {
            "tcp_expectation_failed".into()
        },
        error_message: if matched {
            String::new()
        } else {
            "TCP response did not match the expected pattern".into()
        },
        metadata: HashMap::from([
            ("remote_address".into(), address.to_string()),
            (
                "duration_micros".into(),
                started.elapsed().as_micros().to_string(),
            ),
        ]),
        evidence: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(super) async fn execute_dns(
    task: &wire::ProbeTask,
    spec: &DnsSpec,
    phases: &PhaseClock,
) -> Result<AttemptOutcome> {
    if spec.require_dnssec
        || !matches!(spec.record_type.to_ascii_uppercase().as_str(), "A" | "AAAA")
    {
        return Ok(unknown(
            "dns_capability_unavailable",
            "this Agent build supports system-resolver A/AAAA checks; DNSSEC and other records require a full DNS runtime",
        ));
    }

    if spec.resolver.is_some() {
        return Ok(unknown(
            "custom_dns_resolver_unavailable",
            "custom DNS resolvers are not available in this Agent build",
        ));
    }

    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let addresses = guard.resolve(&spec.name, 53, phases).await?;

    let values = addresses
        .iter()
        .map(|address| address.ip().to_string())
        .filter(|address| {
            (spec.record_type.eq_ignore_ascii_case("A") && address.contains('.'))
                || (spec.record_type.eq_ignore_ascii_case("AAAA") && address.contains(':'))
        })
        .collect::<Vec<_>>();

    let matches = spec.expected_values.is_empty()
        || spec
            .expected_values
            .iter()
            .all(|expected| values.contains(expected));

    Ok(AttemptOutcome {
        outcome: if matches {
            ProbeOutcome::Healthy
        } else {
            ProbeOutcome::Failing
        },
        assertions: Vec::new(),
        response_excerpt: Vec::new(),
        error_category: if matches {
            String::new()
        } else {
            "dns_answer_mismatch".into()
        },
        error_message: if matches {
            String::new()
        } else {
            "DNS answer did not contain every expected value".into()
        },
        metadata: HashMap::from([("answers".into(), values.join(","))]),
        evidence: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(super) async fn execute_tls(
    task: &wire::ProbeTask,
    spec: &TlsSpec,
    phases: &PhaseClock,
) -> Result<AttemptOutcome> {
    let port = u16::try_from(spec.port).context("TLS port is out of range")?;
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let address = guard.resolve(&spec.host, port, phases).await?.remove(0);
    let timeout = Duration::from_millis(u64::from(task.timeout_millis.max(1)));
    let tcp = connect_tcp(address, timeout, phases).await?;
    let server_name = spec.server_name.as_deref().unwrap_or(&spec.host);
    let tls = connect_tls(tcp, server_name, timeout, phases).await?;
    let (_, connection) = tls.get_ref();

    let certificate = connection
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .ok_or_else(|| anyhow!("TLS peer did not present a certificate"))?;

    let (_, parsed) = parse_x509_certificate(certificate.as_ref())
        .map_err(|_| anyhow!("cannot parse TLS peer certificate"))?;
    let expires_at = parsed.validity().not_after.timestamp();
    let remaining_days = (expires_at - now_seconds()).max(0) / 86_400;
    let issuer = parsed.issuer().to_string();

    let sans = parsed
        .subject_alternative_name()?
        .map(|extension| {
            extension
                .value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    GeneralName::DNSName(value) => Some((*value).to_string()),
                    GeneralName::IPAddress(value) => Some(
                        value
                            .iter()
                            .map(|byte| byte.to_string())
                            .collect::<Vec<_>>()
                            .join("."),
                    ),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let issuer_matches = spec
        .expected_issuer_regex
        .as_ref()
        .map(|pattern| Regex::new(pattern).map(|regex| regex.is_match(&issuer)))
        .transpose()?
        .unwrap_or(true);

    let sans_match = spec
        .expected_sans
        .iter()
        .all(|expected| sans.contains(expected));

    let healthy =
        remaining_days >= i64::from(spec.minimum_days_remaining) && issuer_matches && sans_match;

    Ok(AttemptOutcome {
        outcome: if healthy {
            ProbeOutcome::Healthy
        } else {
            ProbeOutcome::Failing
        },
        assertions: Vec::new(),
        response_excerpt: Vec::new(),
        error_category: if healthy {
            String::new()
        } else {
            "tls_expectation_failed".into()
        },
        error_message: if healthy {
            String::new()
        } else {
            "TLS certificate did not satisfy the configured expectations".into()
        },
        metadata: HashMap::from([
            ("issuer".into(), issuer),
            ("remaining_days".into(), remaining_days.to_string()),
            ("sans".into(), sans.join(",")),
        ]),
        evidence: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(super) async fn execute_icmp(
    _task: &wire::ProbeTask,
    _spec: &IcmpSpec,
) -> Result<AttemptOutcome> {
    Ok(unknown(
        "icmp_runtime_unavailable",
        "ICMP requires a platform-specific raw-socket capability not present in this Agent build",
    ))
}

pub(super) async fn execute_grpc(
    task: &wire::ProbeTask,
    spec: &GrpcSpec,
    phases: &PhaseClock,
) -> Result<AttemptOutcome> {
    let Some(grpc_spec::Call::Health(health)) = &spec.call else {
        return Ok(unknown(
            "grpc_unary_runtime_unavailable",
            "generic descriptor/reflection gRPC unary checks are not present in this Agent build",
        ));
    };

    let target = GrpcEndpoint::parse(&spec.endpoint, spec.use_tls)?;
    let host = target.host();
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let address = guard.resolve(&host, target.port(), phases).await?.remove(0);

    // The connection goes to the address the egress policy let by. Every call on it still names
    // the endpoint's own host, which is what a server that routes by host name goes by.
    let mut endpoint =
        Endpoint::from_shared(target.dialled(address))?.origin(target.named().parse()?);
    let timeout = Duration::from_millis(u64::from(task.timeout_millis.max(1)));

    if spec.use_tls {
        let server_name = spec.server_name.clone().unwrap_or(host);

        // tonic trusts no certificate authority it is not told of. The check goes by the public
        // roots, as the TCP and TLS checks do.
        let tls = ClientTlsConfig::new()
            .with_webpki_roots()
            .domain_name(server_name);

        endpoint = endpoint.tls_config(tls)?;
    }

    let request = tonic_health::pb::HealthCheckRequest {
        service: health.service.clone(),
    };

    // tonic can time the TCP connection, the TLS handshake and the wait for the head of the
    // reply, each on its own, and nothing after that. One timeout over the whole exchange leaves
    // no part of it out.
    let exchange = async {
        let channel = endpoint.connect().await?;
        let mut client = tonic_health::pb::health_client::HealthClient::new(channel);
        anyhow::Ok(client.check(request).await?.into_inner())
    };

    let response = tokio::time::timeout(timeout, exchange)
        .await
        .context("gRPC check timed out")??;

    let serving =
        response.status == tonic_health::pb::health_check_response::ServingStatus::Serving as i32;

    Ok(AttemptOutcome {
        outcome: if serving {
            ProbeOutcome::Healthy
        } else {
            ProbeOutcome::Failing
        },
        assertions: Vec::new(),
        response_excerpt: Vec::new(),
        error_category: if serving {
            String::new()
        } else {
            "grpc_not_serving".into()
        },
        error_message: if serving {
            String::new()
        } else {
            "gRPC Health service is not serving".into()
        },
        metadata: HashMap::from([("health_status".into(), response.status.to_string())]),
        evidence: Vec::new(),
        artifacts: Vec::new(),
    })
}

async fn connect_tcp(
    address: SocketAddr,
    timeout: Duration,
    phases: &PhaseClock,
) -> Result<TcpStream> {
    let connecting = tokio::time::timeout(timeout, TcpStream::connect(address));
    Ok(phases.during(Phase::Connect, connecting).await??)
}

async fn connect_tls(
    tcp: TcpStream,
    server_name: &str,
    timeout: Duration,
    phases: &PhaseClock,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let name = ServerName::try_from(server_name.to_string()).context("invalid TLS server name")?;
    let connector = TlsConnector::from(Arc::new(config));

    // The connector starts the handshake as soon as it is called, so the call is timed with it.
    // The timeout starts inside the phase too, so a handshake it cuts short is never timed at less.
    let handshake = async { tokio::time::timeout(timeout, connector.connect(name, tcp)).await };

    phases
        .during(Phase::Tls, handshake)
        .await
        .context("TLS handshake timed out")?
        .context("TLS handshake failed")
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

#[cfg(test)]
mod tests;
