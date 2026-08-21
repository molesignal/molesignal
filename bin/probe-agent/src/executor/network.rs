// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::HashMap,
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
use url::Url;
use x509_parser::{extensions::GeneralName, parse_x509_certificate};

use super::{AttemptOutcome, ExecutionContext, security::EgressGuard, unknown};
use crate::protocol::v1::{
    self as wire, DnsSpec, GrpcSpec, IcmpSpec, ProbeOutcome, TcpSpec, TlsSpec, grpc_spec,
};

trait ProbeIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> ProbeIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub(super) async fn execute_tcp(
    task: &wire::ProbeTask,
    spec: &TcpSpec,
    context: &ExecutionContext,
) -> Result<AttemptOutcome> {
    let host = context.resolve(
        spec.host
            .as_ref()
            .ok_or_else(|| anyhow!("TCP host is required"))?,
    )?;
    let port = u16::try_from(spec.port).context("TCP port is out of range")?;
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let address = guard.resolve(&host, port).await?.remove(0);
    let timeout = Duration::from_millis(u64::from(task.timeout_millis.max(1)));
    let started = Instant::now();
    let tcp = tokio::time::timeout(timeout, TcpStream::connect(address)).await??;
    let mut stream: Box<dyn ProbeIo> = if spec.use_tls {
        let server_name = if spec.server_name.is_empty() {
            &host
        } else {
            &spec.server_name
        };
        Box::new(connect_tls(tcp, server_name).await?)
    } else {
        Box::new(tcp)
    };
    if let Some(send) = &spec.send {
        stream.write_all(context.resolve(send)?.as_bytes()).await?;
        stream.flush().await?;
    }
    let mut response = vec![0u8; 64 * 1024];
    let length = if spec.expect_regex.is_some() || !spec.assertions.is_empty() {
        tokio::time::timeout(timeout, stream.read(&mut response)).await??
    } else {
        0
    };
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

pub(super) async fn execute_dns(task: &wire::ProbeTask, spec: &DnsSpec) -> Result<AttemptOutcome> {
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
    let addresses = guard.resolve(&spec.name, 53).await?;
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

pub(super) async fn execute_tls(task: &wire::ProbeTask, spec: &TlsSpec) -> Result<AttemptOutcome> {
    let port = u16::try_from(spec.port).context("TLS port is out of range")?;
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let address = guard.resolve(&spec.host, port).await?.remove(0);
    let timeout = Duration::from_millis(u64::from(task.timeout_millis.max(1)));
    let tcp = tokio::time::timeout(timeout, TcpStream::connect(address)).await??;
    let server_name = spec.server_name.as_deref().unwrap_or(&spec.host);
    let tls = connect_tls(tcp, server_name).await?;
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
) -> Result<AttemptOutcome> {
    let Some(grpc_spec::Call::Health(health)) = &spec.call else {
        return Ok(unknown(
            "grpc_unary_runtime_unavailable",
            "generic descriptor/reflection gRPC unary checks are not present in this Agent build",
        ));
    };
    let url = Url::parse(&spec.endpoint)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("gRPC endpoint has no host"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("gRPC endpoint has no port"))?;
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let address = guard.resolve(host, port).await?.remove(0);
    let endpoint_url = format!("{}://{}", url.scheme(), address);
    let mut endpoint = Endpoint::from_shared(endpoint_url)?
        .timeout(Duration::from_millis(u64::from(task.timeout_millis.max(1))));
    if spec.use_tls {
        endpoint = endpoint.tls_config(
            ClientTlsConfig::new()
                .domain_name(spec.server_name.clone().unwrap_or_else(|| host.to_string())),
        )?;
    }
    let channel = endpoint.connect().await?;
    let mut client = tonic_health::pb::health_client::HealthClient::new(channel);
    let response = client
        .check(tonic_health::pb::HealthCheckRequest {
            service: health.service.clone(),
        })
        .await?
        .into_inner();
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

async fn connect_tls(
    tcp: TcpStream,
    server_name: &str,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = ServerName::try_from(server_name.to_string()).context("invalid TLS server name")?;
    TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .context("TLS handshake failed")
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}
