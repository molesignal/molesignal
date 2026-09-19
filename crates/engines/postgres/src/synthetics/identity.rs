// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use chrono::{Datelike as _, Duration, Utc};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SerialNumber, SubjectPublicKeyInfo,
};
use sqlx::Row;

use crate::{
    infra::{cipher::CipherRootKey, persistence::sqlx_err},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const AUTHORITY_ID: &str = "probe-root-v1";

pub struct IssuedProbeCertificate {
    pub serial: String,
    pub certificate_chain_pem: String,
    pub expires_at: TimestampMicros,
}

#[derive(Clone)]
pub struct ProbeServerTls {
    pub certificate_chain_pem: String,
    pub private_key_pem: String,
    pub ca_certificate_pem: String,
}

pub struct ProbeCertificateAuthority {
    issuer: Issuer<'static, KeyPair>,
    ca_certificate_pem: String,
    server_tls: ProbeServerTls,
    certificate_days: u32,
}

impl ProbeCertificateAuthority {
    pub async fn load_or_create(
        pool: &sqlx::PgPool,
        cipher: &CipherRootKey,
        server_names: &[String],
        certificate_days: u32,
    ) -> Result<Self> {
        let stored = sqlx::query(
            "SELECT private_key_nonce, private_key_ciphertext, certificate_pem
             FROM synthetic_probe_certificate_authorities WHERE id = $1",
        )
        .bind(AUTHORITY_ID)
        .fetch_optional(pool)
        .await
        .map_err(sqlx_err)?;
        let (private_key_der, ca_certificate_pem) = if let Some(row) = stored {
            let nonce: Vec<u8> = row.try_get("private_key_nonce").map_err(sqlx_err)?;
            let ciphertext: Vec<u8> = row.try_get("private_key_ciphertext").map_err(sqlx_err)?;
            let private_key_der = cipher
                .open(&nonce, &ciphertext)
                .map_err(|error| Error::internal(format!("open Probe CA key: {error}")))?;
            let certificate_pem: String = row.try_get("certificate_pem").map_err(sqlx_err)?;
            (private_key_der, certificate_pem)
        } else {
            create_authority(pool, cipher).await?;
            let row = sqlx::query(
                "SELECT private_key_nonce, private_key_ciphertext, certificate_pem
                 FROM synthetic_probe_certificate_authorities WHERE id = $1",
            )
            .bind(AUTHORITY_ID)
            .fetch_one(pool)
            .await
            .map_err(sqlx_err)?;
            let nonce: Vec<u8> = row.try_get("private_key_nonce").map_err(sqlx_err)?;
            let ciphertext: Vec<u8> = row.try_get("private_key_ciphertext").map_err(sqlx_err)?;
            let key = cipher
                .open(&nonce, &ciphertext)
                .map_err(|error| Error::internal(format!("open new Probe CA key: {error}")))?;
            let certificate_pem: String = row.try_get("certificate_pem").map_err(sqlx_err)?;
            (key, certificate_pem)
        };
        let key = KeyPair::try_from(private_key_der)
            .map_err(|error| Error::internal(format!("parse Probe CA key: {error}")))?;
        let issuer = Issuer::new(authority_params(), key);
        let server_tls = issue_server_certificate(&issuer, &ca_certificate_pem, server_names)?;
        Ok(Self {
            issuer,
            ca_certificate_pem,
            server_tls,
            certificate_days,
        })
    }

    pub fn issue_agent(
        &self,
        agent_id: &Id,
        public_key_der: &[u8],
    ) -> Result<IssuedProbeCertificate> {
        use rand::TryRng as _;

        let public_key = SubjectPublicKeyInfo::from_der(public_key_der)
            .map_err(|error| Error::invalid(format!("invalid Agent public key: {error}")))?;
        let (not_before, not_after, expires_at) = certificate_window(self.certificate_days)?;
        let mut serial_bytes = [0u8; 16];
        rand::rngs::SysRng
            .try_fill_bytes(&mut serial_bytes)
            .map_err(|error| Error::internal(format!("generate certificate serial: {error}")))?;
        serial_bytes[0] &= 0x7f;
        let serial = SerialNumber::from_slice(&serial_bytes);
        let mut params = CertificateParams::new(Vec::<String>::new())
            .map_err(|error| Error::internal(format!("create Agent certificate: {error}")))?;
        params
            .distinguished_name
            .push(DnType::CommonName, agent_id.as_str());
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.serial_number = Some(serial.clone());
        params.not_before = not_before;
        params.not_after = not_after;
        let certificate = params
            .signed_by(&public_key, &self.issuer)
            .map_err(|error| Error::internal(format!("sign Agent certificate: {error}")))?;
        Ok(IssuedProbeCertificate {
            serial: serial.to_string().to_ascii_lowercase(),
            certificate_chain_pem: format!("{}{}", certificate.pem(), self.ca_certificate_pem),
            expires_at,
        })
    }

    pub fn server_tls(&self) -> ProbeServerTls {
        self.server_tls.clone()
    }

    pub fn ca_certificate_pem(&self) -> &str {
        &self.ca_certificate_pem
    }
}

async fn create_authority(pool: &sqlx::PgPool, cipher: &CipherRootKey) -> Result<()> {
    let key = KeyPair::generate()
        .map_err(|error| Error::internal(format!("generate Probe CA key: {error}")))?;
    let certificate = authority_params()
        .self_signed(&key)
        .map_err(|error| Error::internal(format!("self-sign Probe CA: {error}")))?;
    let (nonce, ciphertext) = cipher
        .seal(&key.serialize_der())
        .map_err(|error| Error::internal(format!("seal Probe CA key: {error}")))?;
    sqlx::query(
        "INSERT INTO synthetic_probe_certificate_authorities
            (id, private_key_nonce, private_key_ciphertext, certificate_pem, created_at_micros)
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO NOTHING",
    )
    .bind(AUTHORITY_ID)
    .bind(nonce)
    .bind(ciphertext)
    .bind(certificate.pem())
    .bind(TimestampMicros::now().0)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

fn authority_params() -> CertificateParams {
    let mut params = CertificateParams::new(Vec::<String>::new())
        .expect("empty Probe CA subject alternative names are valid");
    params
        .distinguished_name
        .push(DnType::CommonName, "MoleSignal Probe CA");
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
) -> Result<ProbeServerTls> {
    let key = KeyPair::generate()
        .map_err(|error| Error::internal(format!("generate Probe server key: {error}")))?;
    let mut params = CertificateParams::new(server_names.to_vec())
        .map_err(|error| Error::invalid(format!("invalid Probe server name: {error}")))?;
    params
        .distinguished_name
        .push(DnType::CommonName, &server_names[0]);
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    let (not_before, not_after, _) = certificate_window(90)?;
    params.not_before = not_before;
    params.not_after = not_after;
    let certificate = params
        .signed_by(&key, issuer)
        .map_err(|error| Error::internal(format!("sign Probe server certificate: {error}")))?;
    Ok(ProbeServerTls {
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
        .ok_or_else(|| Error::internal("invalid Probe certificate expiry"))?
        .and_utc();
    Ok((
        not_before,
        not_after,
        TimestampMicros::from_datetime(expires_at),
    ))
}
