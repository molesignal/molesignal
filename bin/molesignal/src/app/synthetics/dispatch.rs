// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Shared task dispatch operations used by external Agents and the Standalone Embedded Runner.

use base64::Engine as _;
use rand::TryRng as _;
use sha2::{Digest, Sha256};

use super::{
    CLIENT_KEY_REFERENCE, ResolvedProbeSecret, SyntheticService, probe::collect_secret_references,
};
use crate::{
    domain::synthetics::{
        ClientKey, MonitorSpec, ProbeAgent, ProbeLocation, ProbeTask, SecretMaterial,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// An Agent beats every 15 s; one silent for four beats is taken to be gone.
pub(super) const AGENT_SILENCE_MICROS: i64 = 60 * 1_000_000;

impl SyntheticService {
    /// The Embedded Runner's heartbeat. It never crosses the Probe control stream, so it beats
    /// here, and its Location reads as online for as long as the process runs.
    pub async fn record_embedded_heartbeat(&self, agent: &ProbeAgent) -> Result<()> {
        self.repository
            .touch_agent(&agent.id, TimestampMicros::now())
            .await
    }

    /// Agents that stopped beating go offline, and Locations say how many of theirs are left.
    pub async fn refresh_probe_liveness(&self, now: TimestampMicros) -> Result<u64> {
        let stale_before = TimestampMicros(now.0.saturating_sub(AGENT_SILENCE_MICROS));
        self.repository.refresh_liveness(now, stale_before).await
    }

    pub async fn lease_next_probe_task(
        &self,
        agent: &ProbeAgent,
    ) -> Result<Option<(ProbeTask, String)>> {
        let mut bytes = [0u8; 32];

        rand::rngs::SysRng
            .try_fill_bytes(&mut bytes)
            .map_err(|error| Error::internal(format!("generate Probe lease: {error}")))?;

        let plaintext = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let now = TimestampMicros::now();
        let leased_until = TimestampMicros(now.0.saturating_add(60 * 1_000_000));

        Ok(self
            .repository
            .lease_task(agent, now, leased_until, lease_hash(&plaintext))
            .await?
            .map(|task| (task, plaintext)))
    }

    pub async fn acknowledge_probe_task(
        &self,
        agent: &ProbeAgent,
        task_id: &Id,
        lease_token: &str,
        accepted: bool,
    ) -> Result<()> {
        self.repository
            .acknowledge_task(
                task_id,
                &agent.id,
                &lease_hash(lease_token),
                accepted,
                TimestampMicros::now(),
            )
            .await
    }

    pub async fn renew_probe_task_lease(
        &self,
        agent: &ProbeAgent,
        task_id: &Id,
        lease_token: &str,
        requested_until: TimestampMicros,
    ) -> Result<TimestampMicros> {
        self.repository
            .renew_task_lease(
                task_id,
                &agent.id,
                &lease_hash(lease_token),
                requested_until,
                TimestampMicros::now(),
            )
            .await
    }

    pub async fn resolve_probe_task_secrets(
        &self,
        task: &ProbeTask,
    ) -> Result<Vec<ResolvedProbeSecret>> {
        let references = collect_secret_references(&task.spec)?;
        let mut resolved = Vec::with_capacity(references.len());

        for (reference, secret_id) in references {
            let (version, material) = self
                .repository
                .resolve_secret(&task.organization_id, &secret_id, None)
                .await?;

            resolved.push(ResolvedProbeSecret {
                reference,
                secret_id,
                version: version.version,
                material,
            });
        }

        // The private key of a client certificate is sealed into the revision that holds it.
        if let MonitorSpec::Http(spec) = &task.spec
            && let Some(certificate) = &spec.client_certificate
        {
            let (revision_id, material) = match &certificate.private_key {
                ClientKey::Kept { revision_id } => (
                    revision_id.clone(),
                    self.repository
                        .open_client_key(&task.organization_id, revision_id)
                        .await?,
                ),
                ClientKey::New { pem } => (
                    task.monitor_revision_id.clone(),
                    SecretMaterial(pem.clone().into_bytes()),
                ),
            };

            resolved.push(ResolvedProbeSecret {
                reference: CLIENT_KEY_REFERENCE.to_string(),
                secret_id: revision_id,
                version: 0,
                material,
            });
        }

        Ok(resolved)
    }

    pub async fn probe_task_location(&self, task: &ProbeTask) -> Result<ProbeLocation> {
        self.repository
            .get_location(&task.organization_id, &task.location_id)
            .await
    }
}

fn lease_hash(lease_token: &str) -> String {
    hex::encode(Sha256::digest(lease_token.as_bytes()))
}
