// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The settings an HTTP journey shares across its steps: a client certificate, a DNS server and
//! host bindings.

use std::net::IpAddr;

use rustls::{
    InconsistentKeys,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject as _},
    sign::CertifiedKey,
};

use crate::{
    domain::synthetics::{ClientCertificate, ClientKey, HttpJourneySpec},
    shared::{Error, Result},
};

const MAX_HOST_BINDINGS: usize = 20;
const MAX_BOUND_ADDRESSES: usize = 16;

/// The most PEM a certificate chain or a private key can take.
const MAX_PEM_LENGTH: usize = 64 * 1024;

pub(super) fn validate_http_settings(spec: &HttpJourneySpec) -> Result<()> {
    if let Some(certificate) = &spec.client_certificate {
        validate_client_certificate(certificate)?;
    }

    if let Some(server) = spec.dns_server.as_deref().map(str::trim)
        && (server.contains('/') || server.contains(char::is_whitespace))
    {
        return Err(Error::invalid(
            "DNS server is a host or an address, with an optional port",
        ));
    }

    if spec.hosts.len() > MAX_HOST_BINDINGS {
        return Err(Error::invalid(format!(
            "an HTTP check can bind at most {MAX_HOST_BINDINGS} host names"
        )));
    }

    for binding in &spec.hosts {
        let name = binding.name.trim();

        if name.is_empty() || name.contains(['/', ':', ' ']) {
            return Err(Error::invalid(format!(
                "host binding `{}` is not a host name",
                binding.name
            )));
        }

        if binding.addresses.is_empty() || binding.addresses.len() > MAX_BOUND_ADDRESSES {
            return Err(Error::invalid(format!(
                "host binding `{name}` needs 1 to {MAX_BOUND_ADDRESSES} addresses"
            )));
        }

        if let Some(address) = binding
            .addresses
            .iter()
            .find(|address| address.trim().parse::<IpAddr>().is_err())
        {
            return Err(Error::invalid(format!(
                "host binding `{name}` names `{address}`, which is not an IP address"
            )));
        }
    }

    Ok(())
}

/// A chain of PEM certificates, and a new private key that goes with the first of them. A kept
/// key was checked when it was given; should the certificate change without it, the Agent says
/// so when it runs the check.
fn validate_client_certificate(certificate: &ClientCertificate) -> Result<()> {
    let text = certificate.certificate.as_bytes();

    if text.len() > MAX_PEM_LENGTH {
        return Err(Error::invalid("the client certificate exceeds 64 KiB"));
    }

    // The certificate is stored as written, so a private key must not come along with it.
    if PrivateKeyDer::from_pem_slice(text).is_ok() {
        return Err(Error::invalid(
            "the client certificate holds a private key: give the key on its own",
        ));
    }

    let chain = CertificateDer::pem_slice_iter(text)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error::invalid("the client certificate is not PEM"))?;

    if chain.is_empty() {
        return Err(Error::invalid(
            "the client certificate holds no certificate",
        ));
    }

    let pem = match &certificate.private_key {
        ClientKey::New { pem } => pem,
        ClientKey::Kept { revision_id } if revision_id.0.trim().is_empty() => {
            return Err(Error::invalid(
                "a kept client private key names its revision",
            ));
        }
        ClientKey::Kept { .. } => return Ok(()),
    };

    if pem.len() > MAX_PEM_LENGTH {
        return Err(Error::invalid("the client private key exceeds 64 KiB"));
    }

    if pem.contains("ENCRYPTED PRIVATE KEY") {
        return Err(Error::invalid(
            "the client private key is encrypted: give it without its passphrase",
        ));
    }

    let key = PrivateKeyDer::from_pem_slice(pem.as_bytes())
        .map_err(|_| Error::invalid("the client private key is not a PEM private key"))?;

    let signing_key = rustls::crypto::aws_lc_rs::default_provider()
        .key_provider
        .load_private_key(key)
        .map_err(|_| Error::invalid("the client private key is of a kind TLS cannot use"))?;

    match CertifiedKey::new(chain, signing_key).keys_match() {
        Err(rustls::Error::InconsistentKeys(InconsistentKeys::KeyMismatch)) => Err(Error::invalid(
            "the client private key does not go with the certificate",
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use rcgen::{CertificateParams, KeyPair};

    use super::validate_http_settings;
    use crate::{
        domain::synthetics::{
            ClientCertificate, ClientKey, HostBinding, HttpJourneySpec, HttpVersion,
        },
        shared::ids::Id,
    };

    fn spec(hosts: Vec<HostBinding>) -> HttpJourneySpec {
        HttpJourneySpec {
            steps: Vec::new(),
            follow_redirects: true,
            max_redirects: 5,
            verify_tls: true,
            http_version: HttpVersion::Http2,
            client_certificate: None,
            dns_server: Some("1.1.1.1:53".into()),
            hosts,
        }
    }

    fn binding(name: &str, addresses: &[&str]) -> HostBinding {
        HostBinding {
            name: name.into(),
            addresses: addresses
                .iter()
                .map(|address| (*address).to_string())
                .collect(),
        }
    }

    fn with_certificate(certificate: String, private_key: ClientKey) -> HttpJourneySpec {
        HttpJourneySpec {
            client_certificate: Some(ClientCertificate {
                certificate,
                private_key,
            }),
            ..spec(Vec::new())
        }
    }

    #[test]
    fn a_host_is_bound_to_ip_addresses() {
        let valid = |bindings| validate_http_settings(&spec(bindings)).is_ok();

        assert!(valid(vec![binding(
            "api.example.com",
            &["192.0.2.10", "2001:db8::1"]
        )]));
        assert!(!valid(vec![binding(
            "api.example.com",
            &["origin.example.com"]
        )]));
        assert!(!valid(vec![binding(
            "https://api.example.com",
            &["192.0.2.10"]
        )]));
        assert!(!valid(vec![binding("api.example.com", &[])]));
    }

    #[test]
    fn a_client_certificate_comes_with_its_own_key() {
        let key = KeyPair::generate().expect("key");
        let other = KeyPair::generate().expect("other key");

        let certificate = CertificateParams::new(vec!["probe".to_string()])
            .expect("params")
            .self_signed(&key)
            .expect("certificate")
            .pem();

        let new = |key: &KeyPair| ClientKey::New {
            pem: key.serialize_pem(),
        };

        let valid = |certificate: &str, private_key| {
            validate_http_settings(&with_certificate(certificate.into(), private_key)).is_ok()
        };

        let kept = ClientKey::Kept {
            revision_id: Id("revision".into()),
        };

        assert!(valid(&certificate, new(&key)));
        assert!(valid(&certificate, kept));

        // Another key, a key in the certificate, or anything that is not PEM is turned down.
        let combined = format!("{certificate}{}", key.serialize_pem());
        let not_pem = ClientKey::New {
            pem: "not PEM".into(),
        };

        assert!(!valid(&certificate, new(&other)));
        assert!(!valid(&combined, new(&key)));
        assert!(!valid("not PEM", new(&key)));
        assert!(!valid(&certificate, not_pem));
    }
}
