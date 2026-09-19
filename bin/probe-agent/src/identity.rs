// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{fs, path::Path};

use anyhow::{Context as _, Result};
use rcgen::{KeyPair, PublicKeyData as _};
use serde::{Deserialize, Serialize};

const IDENTITY_FILE: &str = "identity.json";
const PRIVATE_KEY_FILE: &str = "agent-key.pem";
const CERTIFICATE_FILE: &str = "agent-certificate.pem";
const CA_FILE: &str = "probe-ca.pem";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredIdentity {
    pub agent_id: String,
    pub location_id: String,
    pub control_endpoint: String,
    pub certificate_expires_at_micros: i64,
    pub protocol_version: u32,
}

pub struct AgentIdentity {
    pub metadata: StoredIdentity,
    pub private_key_pem: String,
    pub certificate_chain_pem: String,
    pub ca_certificate_pem: String,
}

pub struct PendingKey {
    key: KeyPair,
}

impl PendingKey {
    pub fn generate() -> Result<Self> {
        Ok(Self {
            key: KeyPair::generate().context("generate Agent private key")?,
        })
    }

    pub fn public_key_der(&self) -> Vec<u8> {
        self.key.subject_public_key_info()
    }

    pub fn private_key_pem(&self) -> String {
        self.key.serialize_pem()
    }
}

impl AgentIdentity {
    pub fn load(state_dir: &Path) -> Result<Self> {
        let metadata = serde_json::from_slice(
            &fs::read(state_dir.join(IDENTITY_FILE)).context("read Agent identity metadata")?,
        )
        .context("parse Agent identity metadata")?;
        Ok(Self {
            metadata,
            private_key_pem: read_utf8(&state_dir.join(PRIVATE_KEY_FILE))?,
            certificate_chain_pem: read_utf8(&state_dir.join(CERTIFICATE_FILE))?,
            ca_certificate_pem: read_utf8(&state_dir.join(CA_FILE))?,
        })
    }

    pub fn persist(&self, state_dir: &Path) -> Result<()> {
        fs::create_dir_all(state_dir).context("create Agent state directory")?;
        restrict_directory(state_dir)?;
        write_private(
            &state_dir.join(IDENTITY_FILE),
            &serde_json::to_vec_pretty(&self.metadata)?,
        )?;
        write_private(
            &state_dir.join(PRIVATE_KEY_FILE),
            self.private_key_pem.as_bytes(),
        )?;
        write_private(
            &state_dir.join(CERTIFICATE_FILE),
            self.certificate_chain_pem.as_bytes(),
        )?;
        write_private(&state_dir.join(CA_FILE), self.ca_certificate_pem.as_bytes())
    }

    pub fn install_rotated(
        &mut self,
        state_dir: &Path,
        pending: PendingKey,
        certificate_chain_pem: String,
        ca_certificate_pem: String,
        expires_at_micros: i64,
    ) -> Result<()> {
        self.private_key_pem = pending.private_key_pem();
        self.certificate_chain_pem = certificate_chain_pem;
        self.ca_certificate_pem = ca_certificate_pem;
        self.metadata.certificate_expires_at_micros = expires_at_micros;
        self.persist(state_dir)
    }
}

fn read_utf8(path: &Path) -> Result<String> {
    String::from_utf8(fs::read(path).with_context(|| format!("read {}", path.display()))?)
        .with_context(|| format!("{} is not UTF-8", path.display()))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes).with_context(|| format!("write {}", temporary.display()))?;
    restrict_file(&temporary)?;
    fs::rename(&temporary, path).with_context(|| format!("install {}", path.display()))
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).context("chmod Agent state")
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).context("chmod Agent identity")
}

#[cfg(not(unix))]
fn restrict_file(path: &Path) -> Result<()> {
    if !path.exists() {
        anyhow::bail!("Agent identity file was not created")
    }
    Ok(())
}
