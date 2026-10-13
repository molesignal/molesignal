// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{net::SocketAddr, num::NonZeroU16};

use anyhow::{Result, anyhow, bail};
use url::Host;

/// Where a gRPC check goes, as the endpoint of its spec writes it.
///
/// An endpoint is a host and a port, `grpc.example.com:443`, and whether the check uses TLS is
/// for its TLS setting alone to say. An `http://` or `https://` URL is read as well, as long as
/// its scheme says the same as the TLS setting. The server holds the endpoint of a Monitor to the
/// same rules when it saves one.
pub(super) struct GrpcEndpoint {
    host: Host,
    /// The port the endpoint writes. Only a URL may leave it out, for the port of its scheme.
    port: Option<NonZeroU16>,
    tls: bool,
}

impl GrpcEndpoint {
    pub(super) fn parse(endpoint: &str, use_tls: bool) -> Result<Self> {
        let endpoint = endpoint.trim();

        let malformed = || {
            anyhow!(
                "gRPC endpoint `{endpoint}` must be a host and a port, such as \
                 grpc.example.com:443"
            )
        };

        let (scheme, address) = match endpoint.split_once("://") {
            Some((scheme, address)) => (Some(scheme), address.strip_suffix('/').unwrap_or(address)),
            None => (None, endpoint),
        };

        // A scheme that drops TLS the check was told to use must not win in silence, and neither
        // side has a better claim than the other. The check says so and leaves the choice open.
        match scheme.map(str::to_ascii_lowercase).as_deref() {
            None => {}
            Some("https") if use_tls => {}
            Some("http") if !use_tls => {}
            Some("https") => bail!(
                "gRPC endpoint starts with https:// while TLS is turned off; make the two agree, \
                 or leave the scheme out"
            ),
            Some("http") => bail!(
                "gRPC endpoint starts with http:// while TLS is turned on; make the two agree, \
                 or leave the scheme out"
            ),
            Some(_) => return Err(malformed()),
        }

        // The port follows the last colon, unless that colon is inside the brackets of an IPv6
        // address.
        let (host, port) = match address.rsplit_once(':') {
            Some((host, port)) if !port.ends_with(']') => (host, Some(port)),
            _ => (address, None),
        };

        let port = match port {
            Some(port) => Some(written_port(port).ok_or_else(malformed)?),
            None if scheme.is_some() => None,
            None => return Err(malformed()),
        };

        let host = Host::parse(host).map_err(|_| malformed())?;

        Ok(Self {
            host,
            port,
            tls: use_tls,
        })
    }

    /// The host to look up, and to hold the certificate of the server to. An IPv6 address stands
    /// here without the brackets an endpoint writes it in.
    pub(super) fn host(&self) -> String {
        match &self.host {
            Host::Domain(name) => name.clone(),
            Host::Ipv4(address) => address.to_string(),
            Host::Ipv6(address) => address.to_string(),
        }
    }

    /// The port to dial: the one the endpoint writes, or else the port of its scheme.
    pub(super) fn port(&self) -> u16 {
        match self.port {
            Some(port) => port.get(),
            None if self.tls => 443,
            None => 80,
        }
    }

    /// The URI of a connection to `address`, one of the addresses of the host.
    pub(super) fn dialled(&self, address: SocketAddr) -> String {
        format!("{}://{address}", self.scheme())
    }

    /// The URI every call names the server by: the host, and the port if the endpoint writes one.
    /// A gRPC client handed the endpoint as its target names the server the same way.
    pub(super) fn named(&self) -> String {
        match self.port {
            Some(port) => format!("{}://{}:{port}", self.scheme(), self.host),
            None => format!("{}://{}", self.scheme(), self.host),
        }
    }

    fn scheme(&self) -> &'static str {
        if self.tls { "https" } else { "http" }
    }
}

/// Reads the port of an endpoint. A port is digits and nothing else, and the number parser alone
/// would let a sign by.
fn written_port(text: &str) -> Option<NonZeroU16> {
    if text.bytes().all(|byte| byte.is_ascii_digit()) {
        text.parse().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::GrpcEndpoint;

    /// What a check makes of an endpoint: the host it looks up, the port it dials and the URI its
    /// calls name the server by. Of an endpoint it turns down, the reason.
    fn read(endpoint: &str, use_tls: bool) -> Result<(String, u16, String), String> {
        GrpcEndpoint::parse(endpoint, use_tls)
            .map(|target| (target.host(), target.port(), target.named()))
            .map_err(|error| error.to_string())
    }

    fn reads(host: &str, port: u16, named: &str) -> Result<(String, u16, String), String> {
        Ok((host.into(), port, named.into()))
    }

    #[test]
    fn a_host_and_a_port_go_by_the_tls_setting() {
        assert_eq!(
            read("grpc.example.com:443", true),
            reads("grpc.example.com", 443, "https://grpc.example.com:443")
        );
        assert_eq!(
            read("grpc.example.com:443", false),
            reads("grpc.example.com", 443, "http://grpc.example.com:443")
        );
        assert_eq!(
            read("10.0.0.1:50051", false),
            reads("10.0.0.1", 50051, "http://10.0.0.1:50051")
        );
        assert_eq!(
            read(" localhost:50051 ", true),
            reads("localhost", 50051, "https://localhost:50051")
        );
    }

    #[test]
    fn an_ipv6_address_is_looked_up_without_its_brackets() {
        assert_eq!(
            read("[::1]:50051", false),
            reads("::1", 50051, "http://[::1]:50051")
        );
        assert_eq!(
            read("https://[2001:db8::1]", true),
            reads("2001:db8::1", 443, "https://[2001:db8::1]")
        );
    }

    #[test]
    fn a_url_names_a_port_only_when_it_writes_one() {
        assert_eq!(
            read("https://grpc.example.com", true),
            reads("grpc.example.com", 443, "https://grpc.example.com")
        );
        assert_eq!(
            read("https://grpc.example.com:443/", true),
            reads("grpc.example.com", 443, "https://grpc.example.com:443")
        );
        assert_eq!(
            read("http://grpc.example.com", false),
            reads("grpc.example.com", 80, "http://grpc.example.com")
        );
        assert_eq!(
            read("HTTPS://Grpc.Example.com:8443", true),
            reads("grpc.example.com", 8443, "https://grpc.example.com:8443")
        );
    }

    #[test]
    fn a_scheme_at_odds_with_the_tls_setting_is_named() {
        let plain = read("http://grpc.example.com:443", true).expect_err("at odds");
        let secure = read("https://grpc.example.com:443", false).expect_err("at odds");
        let advice = "; make the two agree, or leave the scheme out";

        assert_eq!(
            plain.strip_suffix(advice),
            Some("gRPC endpoint starts with http:// while TLS is turned on")
        );
        assert_eq!(
            secure.strip_suffix(advice),
            Some("gRPC endpoint starts with https:// while TLS is turned off")
        );
    }

    #[test]
    fn what_is_not_a_host_and_a_port_is_turned_down() {
        let malformed = [
            "",
            "grpc.example.com",
            "grpc.example.com:",
            "grpc.example.com:0",
            "grpc.example.com:65536",
            "grpc.example.com:+443",
            "grpc.example.com:443/",
            ":443",
            "::1:50051",
            "[::1]",
            "exa mple.com:443",
            "user@grpc.example.com:443",
            "https://",
            "https://grpc.example.com/health",
            "grpc://grpc.example.com:443",
            "dns:///grpc.example.com:443",
        ];

        for endpoint in malformed {
            let reason = format!(
                "gRPC endpoint `{endpoint}` must be a host and a port, such as \
                 grpc.example.com:443"
            );

            assert_eq!(read(endpoint, true), Err(reason));
        }
    }
}
