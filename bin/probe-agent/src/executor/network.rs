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
    AttemptOutcome, ExecutionContext, assertions, failure,
    security::EgressGuard,
    timing::{Phase, PhaseClock},
    trace_context::{self, TRACEPARENT, TraceContext},
    unknown,
};
use crate::protocol::v1::{self as wire, GrpcSpec, ProbeOutcome, TcpSpec, TlsSpec, grpc_spec};

mod dns;
mod endpoint;
mod icmp;
mod traceroute;

pub(super) use dns::{execute as execute_dns, named_resolver};
use endpoint::GrpcEndpoint;
pub(super) use icmp::{available as icmp_available, execute as execute_icmp};
pub(super) use traceroute::{trace as trace_route, worth_tracing};

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
    context.dialling(address.ip());
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
        match tokio::time::timeout(timeout, stream.read(&mut response)).await {
            Ok(read) => read.map_err(|error| {
                failure::target("connection_lost", format!("TCP connection failed: {error}"))
            })?,
            Err(_) => return Err(failure::target("timeout", "TCP reply timed out")),
        }
    } else {
        0
    };

    drop(waiting);
    response.truncate(length);
    let text = String::from_utf8_lossy(&response);
    let elapsed = started.elapsed();

    let matched = spec
        .expect_regex
        .as_ref()
        .map(|pattern| Regex::new(pattern).map(|regex| regex.is_match(&text)))
        .transpose()?
        .unwrap_or(true);

    let read = |source: &str| match source {
        "response" => Some(text.to_string()),
        "duration_ms" => Some(elapsed.as_millis().to_string()),
        _ => None,
    };

    let assertions = assertions::evaluate_all(&spec.assertions, read, context)?;

    let outcome = if matched {
        assertions::outcome(&assertions)
    } else {
        ProbeOutcome::Failing
    };

    let passed = outcome == ProbeOutcome::Healthy;

    Ok(AttemptOutcome {
        outcome,
        assertions,
        response_excerpt: if passed {
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
            ("duration_micros".into(), elapsed.as_micros().to_string()),
        ]),
        evidence: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(super) async fn execute_tls(
    task: &wire::ProbeTask,
    spec: &TlsSpec,
    context: &ExecutionContext,
    phases: &PhaseClock,
) -> Result<AttemptOutcome> {
    let port = u16::try_from(spec.port).context("TLS port is out of range")?;
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let address = guard.resolve(&spec.host, port, phases).await?.remove(0);
    context.dialling(address.ip());
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

pub(super) async fn execute_grpc(
    task: &wire::ProbeTask,
    spec: &GrpcSpec,
    context: &ExecutionContext,
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
    context.dialling(address.ip());

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

    let mut request = tonic::Request::new(tonic_health::pb::HealthCheckRequest {
        service: health.service.clone(),
    });

    let trace_id = call_metadata(&mut request, task, spec, context)?;

    // tonic can time the TCP connection, the TLS handshake and the wait for the head of the
    // reply, each on its own, and nothing after that. One timeout over the whole exchange leaves
    // no part of it out.
    let exchange = async {
        let channel = endpoint.connect().await?;
        let mut client = tonic_health::pb::health_client::HealthClient::new(channel);
        anyhow::Ok(client.check(request).await?.into_inner())
    };

    let started = Instant::now();

    // A server that cannot be reached, turns the call down or does not answer in time fails the
    // check, as a service that is not serving does.
    let response = match tokio::time::timeout(timeout, exchange).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            return Err(failure::target(
                "grpc_failed",
                format!("gRPC call failed: {error:#}"),
            ));
        }
        Err(_) => return Err(failure::target("timeout", "gRPC check timed out")),
    };

    let elapsed = started.elapsed();
    let status = serving_status(response.status);
    let serving = status == "SERVING";

    let read = |source: &str| match source {
        "status" => Some(status.to_string()),
        "duration_ms" => Some(elapsed.as_millis().to_string()),
        _ => None,
    };

    let assertions = assertions::evaluate_all(&spec.assertions, read, context)?;

    Ok(AttemptOutcome {
        outcome: if serving {
            assertions::outcome(&assertions)
        } else {
            ProbeOutcome::Failing
        },
        assertions,
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
        metadata: HashMap::from_iter(
            [
                ("health_status".into(), status.to_string()),
                ("duration_micros".into(), elapsed.as_micros().to_string()),
            ]
            .into_iter()
            .chain(trace_id.map(|trace_id| ("trace_id".into(), trace_id))),
        ),
        evidence: Vec::new(),
        artifacts: Vec::new(),
    })
}

/// Puts the check's metadata on a call, and a span of a new trace unless the check sends its own
/// `traceparent`. Returns the trace the call belongs to, when it names one.
fn call_metadata<T>(
    request: &mut tonic::Request<T>,
    task: &wire::ProbeTask,
    spec: &GrpcSpec,
    context: &ExecutionContext,
) -> Result<Option<String>> {
    for entry in &spec.metadata {
        let value = context.resolve(
            entry
                .value
                .as_ref()
                .ok_or_else(|| anyhow!("gRPC metadata value is required"))?,
        )?;

        let key = tonic::metadata::AsciiMetadataKey::from_bytes(
            entry.name.trim().to_ascii_lowercase().as_bytes(),
        )
        .with_context(|| format!("`{}` is not a gRPC metadata name", entry.name))?;

        let value = value
            .parse()
            .with_context(|| format!("the value of `{}` is not ASCII", entry.name))?;

        request.metadata_mut().append(key, value);
    }

    if let Some(own) = request.metadata().get(TRACEPARENT) {
        return Ok(own.to_str().ok().and_then(trace_context::trace_id_of));
    }

    if !task.inject_trace_context {
        return Ok(None);
    }

    let trace = TraceContext::new();
    let (_, traceparent) = trace.span();
    request
        .metadata_mut()
        .insert(TRACEPARENT, traceparent.parse()?);
    Ok(Some(trace.trace_id()))
}

/// The name the gRPC health protocol gives a serving status.
fn serving_status(status: i32) -> &'static str {
    use tonic_health::pb::health_check_response::ServingStatus;

    match ServingStatus::try_from(status) {
        Ok(ServingStatus::Serving) => "SERVING",
        Ok(ServingStatus::NotServing) => "NOT_SERVING",
        Ok(ServingStatus::ServiceUnknown) => "SERVICE_UNKNOWN",
        _ => "UNKNOWN",
    }
}

async fn connect_tcp(
    address: SocketAddr,
    timeout: Duration,
    phases: &PhaseClock,
) -> Result<TcpStream> {
    let connecting = tokio::time::timeout(timeout, TcpStream::connect(address));

    match phases.during(Phase::Connect, connecting).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(error)) => Err(failure::target(
            "connect_failed",
            format!("cannot connect to {address}: {error}"),
        )),
        Err(_) => Err(failure::target(
            "timeout",
            format!("connecting to {address} timed out"),
        )),
    }
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

    // A certificate the check cannot trust fails the handshake, and the check with it.
    match phases.during(Phase::Tls, handshake).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(error)) => Err(failure::target(
            "tls_failed",
            format!("TLS handshake failed: {error}"),
        )),
        Err(_) => Err(failure::target("timeout", "TLS handshake timed out")),
    }
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
