// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use kernel::{Error, Result, ids::Id};
use rcgen::SubjectPublicKeyInfo;

use crate::{
    certificate_authority::{
        AuthorityProfile, CertificateAuthority, ClientSubject, IssuedCertificate, ServerTls,
    },
    cipher::CipherRootKey,
};

const PROFILE: AuthorityProfile = AuthorityProfile {
    id: "probe-root-v1",
    common_name: "MoleSignal Probe CA",
};

pub type IssuedProbeCertificate = IssuedCertificate;
pub type ProbeServerTls = ServerTls;

/// The CA of Probe Agents: their client certificates and the Probe listeners' server certificate.
pub struct ProbeCertificateAuthority(CertificateAuthority);

impl ProbeCertificateAuthority {
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

    pub fn issue_agent(
        &self,
        agent_id: &Id,
        public_key_der: &[u8],
    ) -> Result<IssuedProbeCertificate> {
        let public_key = SubjectPublicKeyInfo::from_der(public_key_der)
            .map_err(|error| Error::invalid(format!("invalid Agent public key: {error}")))?;

        self.0.issue_client(
            ClientSubject {
                common_name: agent_id.as_str(),
                organization: None,
                uri: None,
            },
            &public_key,
        )
    }

    pub fn server_tls(&self) -> ProbeServerTls {
        self.0.server_tls()
    }

    pub fn ca_certificate_pem(&self) -> &str {
        self.0.ca_certificate_pem()
    }
}
