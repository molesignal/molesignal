// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Node identities: certificates of the Node Agent CA, bound to one organization.

use kernel::{Error, Result, ids::Id};
use postgres::{
    certificate_authority::{
        AuthorityProfile, CertificateAuthority, ClientSubject, IssuedCertificate, ServerTls,
    },
    cipher::CipherRootKey,
};
use rcgen::CertificateSigningRequestParams;
use x509_cert::der::Decode as _;

const PROFILE: AuthorityProfile = AuthorityProfile {
    id: "node-agent-root-v1",
    common_name: "MoleSignal Node Agent CA",
};

/// Largest CSR accepted, PEM.
const MAX_CSR_LENGTH: usize = 16 * 1024;

/// What a client certificate says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    pub serial: String,
    pub node_id: String,
    pub org_id: String,
}

pub struct NodeAgentAuthority(CertificateAuthority);

impl NodeAgentAuthority {
    pub async fn load_or_create(
        pool: &sqlx::PgPool,
        cipher: &CipherRootKey,
        server_names: &[String],
        certificate_days: u32,
    ) -> Result<Self> {
        CertificateAuthority::load_or_create(pool, cipher, PROFILE, server_names, certificate_days)
            .await
            .map(Self)
    }

    /// A CA that lives only in memory, for tests.
    pub fn ephemeral(server_names: &[String], certificate_days: u32) -> Result<Self> {
        CertificateAuthority::ephemeral(PROFILE, server_names, certificate_days).map(Self)
    }

    /// Signs the CSR as the identity of `node_id` in `org_id`. The CSR's own subject is ignored.
    pub fn issue_node(
        &self,
        org_id: &Id,
        node_id: &Id,
        csr_pem: &str,
    ) -> Result<IssuedCertificate> {
        if csr_pem.trim().is_empty() || csr_pem.len() > MAX_CSR_LENGTH {
            return Err(Error::invalid("a PEM CSR of at most 16 KiB is required"));
        }

        // Parsing checks the CSR's signature: whoever sent it holds the private key.
        let csr = CertificateSigningRequestParams::from_pem(csr_pem)
            .map_err(|error| Error::invalid(format!("invalid CSR: {error}")))?;

        self.0.issue_client(
            ClientSubject {
                common_name: node_id.as_str(),
                organization: Some(org_id.as_str()),
                uri: Some(format!(
                    "molesignal://node-agent/{}/{}",
                    org_id.as_str(),
                    node_id.as_str()
                )),
            },
            &csr.public_key,
        )
    }

    /// The signature agents check a configuration update against: the 8-byte big-endian revision
    /// followed by the configuration.
    pub fn sign_config(&self, revision: u64, config: &[u8]) -> Result<Vec<u8>> {
        let mut message = Vec::with_capacity(8 + config.len());
        message.extend_from_slice(&revision.to_be_bytes());
        message.extend_from_slice(config);
        self.0.sign(&message)
    }

    pub fn server_tls(&self) -> ServerTls {
        self.0.server_tls()
    }

    pub fn ca_certificate_pem(&self) -> &str {
        self.0.ca_certificate_pem()
    }
}

/// Reads the serial, node (`CN`) and organization (`O`) of a peer certificate the TLS layer
/// already verified against the Node Agent CA.
pub fn peer_identity(certificate_der: &[u8]) -> Result<PeerIdentity> {
    let certificate = x509_cert::Certificate::from_der(certificate_der)
        .map_err(|_| Error::unauthorized("invalid node client certificate"))?;

    let serial = certificate
        .tbs_certificate
        .serial_number
        .to_string()
        .to_ascii_lowercase();

    let mut node_id = None;
    let mut org_id = None;

    for rdn in certificate.tbs_certificate.subject.0.iter() {
        for attribute in rdn.0.iter() {
            let value = attribute
                .value
                .decode_as::<x509_cert::der::asn1::Utf8StringRef<'_>>()
                .map(|value| value.as_str().to_string())
                .ok();

            match attribute.oid.to_string().as_str() {
                "2.5.4.3" => node_id = value,
                "2.5.4.10" => org_id = value,
                _ => {}
            }
        }
    }

    match (node_id, org_id) {
        (Some(node_id), Some(org_id)) => Ok(PeerIdentity {
            serial,
            node_id,
            org_id,
        }),
        _ => Err(Error::unauthorized(
            "the client certificate names no node and organization",
        )),
    }
}
