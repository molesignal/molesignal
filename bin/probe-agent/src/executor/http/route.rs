// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where the requests of an HTTP check go, and how they get there: the addresses a host is
//! reached at, the proxies on the way, and how TLS and HTTP are spoken on the connection.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail};
use hickory_resolver::TokioResolver;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject as _};

use super::proxy;
use crate::{
    executor::{
        ExecutionContext, failure, network,
        security::EgressGuard,
        timing::{Phase, PhaseClock},
    },
    protocol::v1::{self as wire, HttpJourneySpec},
};

/// The route every request of one attempt takes.
pub(super) struct Route {
    pub guard: EgressGuard,
    pub proxies: proxy::Rules,
    pub transport: Transport,
    /// Host names the check reaches at addresses of its own choosing, by their lower-case name.
    bindings: HashMap<String, Vec<IpAddr>>,
    /// The DNS server the check looks host names up on, when it names one.
    dns: Option<TokioResolver>,
}

impl Route {
    pub async fn new(
        task: &wire::ProbeTask,
        spec: &HttpJourneySpec,
        context: &ExecutionContext,
        phases: &PhaseClock,
    ) -> Result<Self> {
        let guard = EgressGuard::new(task.egress_policy.clone())?;
        let timeout = Duration::from_millis(u64::from(task.timeout_millis.max(1)));

        let bindings = spec
            .hosts
            .iter()
            .map(|binding| {
                let addresses = binding
                    .addresses
                    .iter()
                    .map(|address| {
                        address.trim().parse::<IpAddr>().map_err(|_| {
                            anyhow!("host binding address `{address}` is not an IP address")
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;

                Ok((normal(&binding.name), addresses))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        let dns = match spec.dns_server.as_deref().map(str::trim) {
            Some(server) if !server.is_empty() => {
                Some(network::named_resolver(&guard, server, timeout, false, phases).await?)
            }
            _ => None,
        };

        Ok(Self {
            transport: Transport::new(spec, context)?,
            proxies: proxy::Rules::from_environment(),
            guard,
            bindings,
            dns,
        })
    }

    /// The addresses `host` is reached at that the egress policy lets by: those it is bound to,
    /// else those the check's own DNS server gives, else the system's.
    pub async fn resolve(
        &self,
        host: &str,
        port: u16,
        phases: &PhaseClock,
    ) -> Result<Vec<SocketAddr>> {
        if let Some(addresses) = self.bindings.get(&normal(host)) {
            return self.guard.approve(host, port, addresses);
        }

        if let Some(dns) = &self.dns
            && host.parse::<IpAddr>().is_err()
        {
            let lookup = phases
                .during(Phase::Dns, dns.lookup_ip(host))
                .await
                .map_err(|error| {
                    failure::target("dns_failed", format!("cannot resolve {host}: {error}"))
                })?;

            return self
                .guard
                .approve(host, port, &lookup.iter().collect::<Vec<_>>());
        }

        self.guard.resolve(host, port, phases).await
    }
}

/// A host name as a binding is looked up by: without the final dot, in lower case.
fn normal(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// How TLS and HTTP are spoken on a connection to the server a request is for.
pub(super) struct Transport {
    pub verify: bool,
    pub version: HttpVersion,
    pub identity: Option<ClientIdentity>,
}

/// The HTTP protocol a check speaks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HttpVersion {
    /// Over TLS what the server picks, HTTP/2 or HTTP/1.1; HTTP/1.1 over plain HTTP.
    Auto,
    Http1,
    /// HTTP/2 only; over plain HTTP by prior knowledge.
    Http2,
}

impl HttpVersion {
    /// The protocols offered in TLS, the server choosing among them.
    pub fn alpn(self) -> Vec<Vec<u8>> {
        match self {
            Self::Auto => vec![b"h2".to_vec(), b"http/1.1".to_vec()],
            Self::Http1 => vec![b"http/1.1".to_vec()],
            Self::Http2 => vec![b"h2".to_vec()],
        }
    }
}

impl Transport {
    fn new(spec: &HttpJourneySpec, context: &ExecutionContext) -> Result<Self> {
        let version = match wire::HttpVersion::try_from(spec.http_version) {
            Ok(wire::HttpVersion::Http1) => HttpVersion::Http1,
            Ok(wire::HttpVersion::Http2) => HttpVersion::Http2,
            _ => HttpVersion::Auto,
        };

        let identity = spec
            .client_certificate
            .as_ref()
            .map(|certificate| {
                let read = |value: &Option<wire::Value>, what: &str| {
                    value
                        .as_ref()
                        .ok_or_else(|| anyhow!("client certificate {what} is required"))
                        .and_then(|value| context.resolve(value))
                };

                ClientIdentity::parse(
                    &read(&certificate.certificate, "chain")?,
                    &read(&certificate.private_key, "key")?,
                )
            })
            .transpose()?;

        Ok(Self {
            verify: spec.verify_tls,
            version,
            identity,
        })
    }
}

/// The certificate chain a check shows a server that asks for one, and its private key.
pub(super) struct ClientIdentity {
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
}

impl ClientIdentity {
    fn parse(certificate: &str, key: &str) -> Result<Self> {
        let chain = CertificateDer::pem_slice_iter(certificate.as_bytes())
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("the client certificate is not PEM")?;

        if chain.is_empty() {
            bail!("the client certificate holds no certificate");
        }

        let key = PrivateKeyDer::from_pem_slice(key.as_bytes())
            .context("the client private key is not a PEM key")?;

        Ok(Self { chain, key })
    }

    pub fn chain(&self) -> Vec<CertificateDer<'static>> {
        self.chain.clone()
    }

    pub fn key(&self) -> PrivateKeyDer<'static> {
        self.key.clone_key()
    }
}
