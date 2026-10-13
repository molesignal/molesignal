// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How an HTTP check secures its connection.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use hyper::Uri;
use rustls::{
    ClientConfig, DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, aws_lc_rs},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use rustls_platform_verifier::Verifier;
use tokio_rustls::TlsConnector;

/// The TLS client for a server that is about to be sent a request. It offers HTTP/1.1, the one
/// protocol a check speaks.
pub(super) fn connector(verify: bool) -> Result<TlsConnector> {
    let mut config = config(verify)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsConnector::from(Arc::new(config)))
}

/// The TLS client for a proxy that is about to be asked for a tunnel. It offers no protocol: what
/// the connection carries is settled with the target, inside the tunnel.
pub(super) fn connector_for_tunnel(verify: bool) -> Result<TlsConnector> {
    Ok(TlsConnector::from(Arc::new(config(verify)?)))
}

/// What a TLS client of an HTTP check goes by.
///
/// It trusts the certificates the operating system trusts, read afresh for every request, or any
/// certificate at all for a check that does not verify them.
fn config(verify: bool) -> Result<ClientConfig> {
    let provider = CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(aws_lc_rs::default_provider()));

    let verifier: Arc<dyn ServerCertVerifier> = if verify {
        Arc::new(Verifier::new(provider.clone()).context("cannot load the system trust store")?)
    } else {
        Arc::new(AnyCertificate)
    };

    Ok(ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

/// The name the server of `uri` has to hold a certificate for.
pub(super) fn server_name(uri: &Uri) -> Result<ServerName<'static>> {
    let host = uri.host().unwrap_or_default();

    // A URI brackets an IPv6 address, a certificate does not.
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);

    ServerName::try_from(host.to_owned()).context("invalid TLS server name")
}

/// Takes whatever certificate a server presents, and its word for holding the key.
#[derive(Debug)]
struct AnyCertificate;

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _certificate: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _certificate: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    // No signature is checked, so a server may sign with any scheme there is.
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::ECDSA_SHA1_Legacy,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}
