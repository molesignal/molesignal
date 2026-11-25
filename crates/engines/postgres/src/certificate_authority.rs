// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Private certificate authorities for agents that authenticate with mTLS.
//!
//! Each kind of agent has its own CA (a row of `certificate_authorities`, keyed by
//! [`AuthorityProfile::id`]), so a certificate issued to one kind is never trusted by the listener
//! of another. The CA key is sealed with the root key; the server certificate is issued fresh at
//! every start.

use chrono::{Datelike as _, Duration, Utc};
use kernel::{Error, Result, time::TimestampMicros};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, PublicKeyData, SanType, SerialNumber, SigningKey as _,
};
use sqlx::Row;

use crate::{cipher::CipherRootKey, persistence::sqlx_err};

/// Which CA: its row id and the common name of its certificate.
#[derive(Debug, Clone, Copy)]
pub struct AuthorityProfile {
    pub id: &'static str,
    pub common_name: &'static str,
}

/// What a client certificate names.
pub struct ClientSubject<'a> {
    pub common_name: &'a str,
    /// The organization the identity belongs to (`O=`).
    pub organization: Option<&'a str>,
    /// A URI subject alternative name, e.g. `molesignal://node-agent/<org>/<node>`.
    pub uri: Option<String>,
}

pub struct IssuedCertificate {
    /// Lower-case hexadecimal serial, as the listener reads it back from the peer certificate.
    pub serial: String,
    pub certificate_pem: String,
    /// The leaf followed by the CA certificate.
    pub certificate_chain_pem: String,
    pub expires_at: TimestampMicros,
}

#[derive(Clone)]
pub struct ServerTls {
    pub certificate_chain_pem: String,
    pub private_key_pem: String,
    pub ca_certificate_pem: String,
}

pub struct CertificateAuthority {
    issuer: Issuer<'static, KeyPair>,
    ca_certificate_pem: String,
    server_tls: ServerTls,
    certificate_days: u32,
}

impl CertificateAuthority {
    /// Loads the CA of `profile`, creating it on first use.
    pub async fn load_or_create(
        pool: &sqlx::PgPool,
        cipher: &CipherRootKey,
        profile: AuthorityProfile,
        server_names: &[String],
        certificate_days: u32,
    ) -> Result<Self> {
        if load(pool, cipher, profile).await?.is_none() {
            create(pool, cipher, profile).await?;
        }

        let (private_key_der, ca_certificate_pem) = load(pool, cipher, profile)
            .await?
            .ok_or_else(|| Error::internal(format!("CA `{}` was not stored", profile.id)))?;

        let key = KeyPair::try_from(private_key_der)
            .map_err(|error| Error::internal(format!("parse CA key: {error}")))?;

        Self::from_parts(
            profile,
            key,
            ca_certificate_pem,
            server_names,
            certificate_days,
        )
    }

    /// A CA that lives only in memory, for tests.
    pub fn ephemeral(
        profile: AuthorityProfile,
        server_names: &[String],
        certificate_days: u32,
    ) -> Result<Self> {
        let key = KeyPair::generate()
            .map_err(|error| Error::internal(format!("generate CA key: {error}")))?;

        let certificate = authority_params(profile)
            .self_signed(&key)
            .map_err(|error| Error::internal(format!("self-sign CA: {error}")))?;

        Self::from_parts(
            profile,
            key,
            certificate.pem(),
            server_names,
            certificate_days,
        )
    }

    fn from_parts(
        profile: AuthorityProfile,
        key: KeyPair,
        ca_certificate_pem: String,
        server_names: &[String],
        certificate_days: u32,
    ) -> Result<Self> {
        let issuer = Issuer::new(authority_params(profile), key);
        let server_tls = issue_server_certificate(&issuer, &ca_certificate_pem, server_names)?;

        Ok(Self {
            issuer,
            ca_certificate_pem,
            server_tls,
            certificate_days,
        })
    }

    /// Signs a client-authentication certificate for `public_key`.
    pub fn issue_client(
        &self,
        subject: ClientSubject<'_>,
        public_key: &impl PublicKeyData,
    ) -> Result<IssuedCertificate> {
        use rand::TryRng as _;

        let (not_before, not_after, expires_at) = certificate_window(self.certificate_days)?;
        let mut serial_bytes = [0u8; 16];

        rand::rngs::SysRng
            .try_fill_bytes(&mut serial_bytes)
            .map_err(|error| Error::internal(format!("generate certificate serial: {error}")))?;

        // Positive and without a leading zero byte, so the DER encoding keeps all 16 bytes and the
        // serial read back from the peer certificate is the one stored here.
        serial_bytes[0] = (serial_bytes[0] & 0x7f) | 0x01;
        let serial = SerialNumber::from_slice(&serial_bytes);
        let mut params = CertificateParams::new(Vec::<String>::new())
            .map_err(|error| Error::internal(format!("create client certificate: {error}")))?;

        params
            .distinguished_name
            .push(DnType::CommonName, subject.common_name);

        if let Some(organization) = subject.organization {
            params
                .distinguished_name
                .push(DnType::OrganizationName, organization);
        }

        if let Some(uri) = subject.uri {
            let uri = uri
                .try_into()
                .map_err(|error| Error::invalid(format!("invalid certificate URI: {error}")))?;

            params.subject_alt_names.push(SanType::URI(uri));
        }

        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.serial_number = Some(serial.clone());
        params.not_before = not_before;
        params.not_after = not_after;

        let certificate = params
            .signed_by(public_key, &self.issuer)
            .map_err(|error| Error::internal(format!("sign client certificate: {error}")))?;

        let certificate_pem = certificate.pem();

        Ok(IssuedCertificate {
            serial: serial.to_string().to_ascii_lowercase(),
            certificate_chain_pem: format!("{certificate_pem}{}", self.ca_certificate_pem),
            certificate_pem,
            expires_at,
        })
    }

    /// Signs `message` with the CA key (ECDSA P-256 / SHA-256, ASN.1 DER), so that an agent holding
    /// the CA certificate can check what the control plane sent.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
        self.issuer
            .key()
            .sign(message)
            .map_err(|error| Error::internal(format!("sign with CA key: {error}")))
    }

    pub fn server_tls(&self) -> ServerTls {
        self.server_tls.clone()
    }

    pub fn ca_certificate_pem(&self) -> &str {
        &self.ca_certificate_pem
    }
}

async fn load(
    pool: &sqlx::PgPool,
    cipher: &CipherRootKey,
    profile: AuthorityProfile,
) -> Result<Option<(Vec<u8>, String)>> {
    let Some(row) = sqlx::query(
        "SELECT private_key_nonce, private_key_ciphertext, certificate_pem
         FROM certificate_authorities WHERE id = $1",
    )
    .bind(profile.id)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?
    else {
        return Ok(None);
    };

    let nonce: Vec<u8> = row.try_get("private_key_nonce").map_err(sqlx_err)?;
    let ciphertext: Vec<u8> = row.try_get("private_key_ciphertext").map_err(sqlx_err)?;

    let private_key_der = cipher
        .open(&nonce, &ciphertext)
        .map_err(|error| Error::internal(format!("open CA key `{}`: {error}", profile.id)))?;

    let certificate_pem: String = row.try_get("certificate_pem").map_err(sqlx_err)?;
    Ok(Some((private_key_der, certificate_pem)))
}

async fn create(
    pool: &sqlx::PgPool,
    cipher: &CipherRootKey,
    profile: AuthorityProfile,
) -> Result<()> {
    let key = KeyPair::generate()
        .map_err(|error| Error::internal(format!("generate CA key: {error}")))?;

    let certificate = authority_params(profile)
        .self_signed(&key)
        .map_err(|error| Error::internal(format!("self-sign CA: {error}")))?;

    let (nonce, ciphertext) = cipher
        .seal(&key.serialize_der())
        .map_err(|error| Error::internal(format!("seal CA key: {error}")))?;

    // Two nodes starting at once: the first insert wins and both load it afterwards.
    sqlx::query(
        "INSERT INTO certificate_authorities
            (id, private_key_nonce, private_key_ciphertext, certificate_pem, created_at)
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO NOTHING",
    )
    .bind(profile.id)
    .bind(nonce)
    .bind(ciphertext)
    .bind(certificate.pem())
    .bind(TimestampMicros::now())
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

fn authority_params(profile: AuthorityProfile) -> CertificateParams {
    let mut params = CertificateParams::new(Vec::<String>::new())
        .expect("empty CA subject alternative names are valid");

    params
        .distinguished_name
        .push(DnType::CommonName, profile.common_name);

    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];

    params
}

fn issue_server_certificate(
    issuer: &Issuer<'_, KeyPair>,
    ca_certificate_pem: &str,
    server_names: &[String],
) -> Result<ServerTls> {
    let first = server_names
        .first()
        .ok_or_else(|| Error::invalid("a server certificate needs at least one name"))?;

    let key = KeyPair::generate()
        .map_err(|error| Error::internal(format!("generate server key: {error}")))?;
    let mut params = CertificateParams::new(server_names.to_vec())
        .map_err(|error| Error::invalid(format!("invalid server name: {error}")))?;

    params.distinguished_name.push(DnType::CommonName, first);
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    let (not_before, not_after, _) = certificate_window(90)?;
    params.not_before = not_before;
    params.not_after = not_after;

    let certificate = params
        .signed_by(&key, issuer)
        .map_err(|error| Error::internal(format!("sign server certificate: {error}")))?;

    Ok(ServerTls {
        certificate_chain_pem: format!("{}{}", certificate.pem(), ca_certificate_pem),
        private_key_pem: key.serialize_pem(),
        ca_certificate_pem: ca_certificate_pem.to_string(),
    })
}

fn certificate_window(
    days: u32,
) -> Result<(time::OffsetDateTime, time::OffsetDateTime, TimestampMicros)> {
    let today = Utc::now().date_naive();
    let before = today - Duration::days(1);
    let after = today + Duration::days(i64::from(days));
    let not_before = rcgen::date_time_ymd(before.year(), before.month() as u8, before.day() as u8);
    let not_after = rcgen::date_time_ymd(after.year(), after.month() as u8, after.day() as u8);

    let expires_at = after
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| Error::internal("invalid certificate expiry"))?
        .and_utc();

    Ok((
        not_before,
        not_after,
        TimestampMicros::from_datetime(expires_at),
    ))
}
