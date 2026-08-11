// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Shared task dispatch operations used by external Agents and the Standalone Embedded Runner.

use base64::Engine as _;
use rand::TryRng as _;
use sha2::{Digest, Sha256};

use super::{ResolvedProbeSecret, SyntheticService, probe::collect_secret_references};
use crate::{
    domain::synthetics::{ProbeAgent, ProbeLocation, ProbeTask},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl SyntheticService {
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
