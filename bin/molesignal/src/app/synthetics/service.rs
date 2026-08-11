// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use super::{
    model::{
        CreateLocationInput, CreateMonitorInput, CreateSecretInput, UpdateAgentConfigurationInput,
        build_revision, normalize_agent_labels, validate_code, validate_name, validate_tags,
    },
    next_due_at, validate_revision,
};
use crate::{
    domain::synthetics::{
        ActiveMonitorRevision, AgentStatus, LocationExecution, LocationHealth, LocationLifecycle,
        LocationScope, MonitorLifecycle, MonitorRevision, MonitorState, ProbeAgent, ProbeLocation,
        SecretMaterial, SyntheticMonitor, SyntheticRepository, SyntheticSecret,
        SyntheticSecretVersion,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct SyntheticService {
    pub(super) repository: Arc<dyn SyntheticRepository>,
    pub(super) transition_sink: Option<Arc<dyn super::SyntheticTransitionSink>>,
    register_endpoint: String,
    control_endpoint: String,
    probe_ca_certificate_pem: String,
}

impl SyntheticService {
    pub fn new(repository: Arc<dyn SyntheticRepository>) -> Self {
        Self {
            repository,
            transition_sink: None,
            register_endpoint: String::new(),
            control_endpoint: String::new(),
            probe_ca_certificate_pem: String::new(),
        }
    }

    pub fn with_probe_configuration(
        mut self,
        register_endpoint: String,
        control_endpoint: String,
        ca_certificate_pem: String,
    ) -> Self {
        self.register_endpoint = register_endpoint;
        self.control_endpoint = control_endpoint;
        self.probe_ca_certificate_pem = ca_certificate_pem;
        self
    }

    pub fn with_transition_sink(mut self, sink: Arc<dyn super::SyntheticTransitionSink>) -> Self {
        self.transition_sink = Some(sink);
        self
    }

    pub async fn create_location(
        &self,
        org_id: &Id,
        input: CreateLocationInput,
    ) -> Result<ProbeLocation> {
        validate_name(&input.name, 255, "Location name")?;
        validate_code(&input.code)?;
        let now = TimestampMicros::now();
        self.repository
            .create_location(ProbeLocation {
                id: Id::new(),
                organization_id: Some(org_id.clone()),
                name: input.name.trim().to_string(),
                code: input.code.trim().to_ascii_lowercase(),
                description: input.description.trim().to_string(),
                scope: LocationScope::Organization,
                execution: LocationExecution::AgentPool,
                lifecycle: LocationLifecycle::Active,
                health: LocationHealth::Unknown,
                system_managed: false,
                egress_policy: input.egress_policy,
                created_at: now,
                updated_at: now,
            })
            .await
    }

    pub async fn list_locations(&self, org_id: &Id) -> Result<Vec<ProbeLocation>> {
        self.repository.list_locations(org_id).await
    }

    pub async fn set_location_lifecycle(
        &self,
        org_id: &Id,
        location_id: &Id,
        lifecycle: LocationLifecycle,
    ) -> Result<ProbeLocation> {
        let mut location = self.repository.get_location(org_id, location_id).await?;
        if location.organization_id.as_ref() != Some(org_id) || location.system_managed {
            return Err(Error::forbidden(
                "platform Locations cannot be changed by an organization",
            ));
        }
        location.lifecycle = lifecycle;
        location.updated_at = TimestampMicros::now();
        self.repository.update_location(location).await
    }

    pub async fn list_agents(&self, org_id: &Id, location_id: &Id) -> Result<Vec<ProbeAgent>> {
        self.repository.get_location(org_id, location_id).await?;
        self.repository.list_agents(org_id, location_id).await
    }

    pub async fn list_all_agents(&self, org_id: &Id) -> Result<Vec<ProbeAgent>> {
        self.repository.list_all_agents(org_id).await
    }

    pub async fn update_agent_configuration(
        &self,
        org_id: &Id,
        actor_id: &Id,
        agent_id: &Id,
        input: UpdateAgentConfigurationInput,
    ) -> Result<ProbeAgent> {
        validate_name(&input.name, 255, "Agent name")?;
        let current = self.repository.get_agent(org_id, agent_id).await?;
        let labels = normalize_agent_labels(input.labels, &current.labels)?;
        self.repository
            .update_agent_configuration(
                org_id,
                actor_id,
                agent_id,
                input.name.trim(),
                &labels,
                TimestampMicros::now(),
            )
            .await
    }

    pub async fn revoke_agent(&self, org_id: &Id, agent_id: &Id) -> Result<ProbeAgent> {
        let mut agent = self.repository.get_agent(org_id, agent_id).await?;
        if agent.organization_id.as_ref() != Some(org_id) {
            return Err(Error::forbidden(
                "platform Probe Agents cannot be revoked by an organization",
            ));
        }
        let now = TimestampMicros::now();
        agent.status = AgentStatus::Revoked;
        agent.revoked_at = Some(now);
        agent.updated_at = now;
        self.repository.update_agent(agent).await
    }

    pub async fn create_register_token(
        &self,
        org_id: &Id,
        actor_id: &Id,
        location_id: &Id,
        ttl_minutes: u32,
    ) -> Result<crate::domain::synthetics::ProbeRegisterInstructions> {
        use base64::Engine as _;
        use rand::TryRng as _;
        use sha2::{Digest, Sha256};

        if self.register_endpoint.is_empty()
            || self.control_endpoint.is_empty()
            || self.probe_ca_certificate_pem.is_empty()
        {
            return Err(Error::unavailable("Probe registration is not configured"));
        }
        let location = self.repository.get_location(org_id, location_id).await?;
        if location.organization_id.as_ref() != Some(org_id)
            || location.execution != LocationExecution::AgentPool
            || location.lifecycle != LocationLifecycle::Active
        {
            return Err(Error::invalid(
                "registration requires an active organization AgentPool Location",
            ));
        }
        let ttl_minutes = ttl_minutes.clamp(5, 60);
        let now = TimestampMicros::now();
        let expires_at = TimestampMicros(
            now.0
                .saturating_add(i64::from(ttl_minutes) * 60 * 1_000_000),
        );
        let mut raw = [0u8; 32];
        rand::rngs::SysRng
            .try_fill_bytes(&mut raw)
            .map_err(|error| Error::internal(format!("generate register token: {error}")))?;
        let plaintext = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
        let hash = Sha256::digest(plaintext.as_bytes()).to_vec();
        let token = self
            .repository
            .create_register_token(
                crate::domain::synthetics::ProbeRegisterToken {
                    id: Id::new(),
                    organization_id: org_id.clone(),
                    location_id: location_id.clone(),
                    expires_at,
                    created_by: actor_id.clone(),
                    created_at: now,
                },
                hash,
            )
            .await?;
        let ca_base64 = base64::engine::general_purpose::STANDARD
            .encode(self.probe_ca_certificate_pem.as_bytes());
        let ca_sha256 = hex::encode(Sha256::digest(self.probe_ca_certificate_pem.as_bytes()));
        let command = format!(
            "docker run --rm -v molesignal-probe:/var/lib/molesignal-probe \
             ghcr.io/molesignal/probe-agent:latest register \
             --endpoint {} --token {} --ca-certificate-base64 {}",
            self.register_endpoint, plaintext, ca_base64
        );
        Ok(crate::domain::synthetics::ProbeRegisterInstructions {
            token,
            register_token: plaintext,
            register_endpoint: self.register_endpoint.clone(),
            control_endpoint: self.control_endpoint.clone(),
            ca_certificate_pem: self.probe_ca_certificate_pem.clone(),
            ca_sha256,
            command,
        })
    }

    pub async fn create_secret(
        &self,
        org_id: &Id,
        actor_id: &Id,
        input: CreateSecretInput,
    ) -> Result<SyntheticSecret> {
        validate_name(&input.name, 255, "Secret name")?;
        let now = TimestampMicros::now();
        self.repository
            .create_secret(
                SyntheticSecret {
                    id: Id::new(),
                    organization_id: org_id.clone(),
                    name: input.name.trim().to_string(),
                    description: input.description.trim().to_string(),
                    current_version: 1,
                    created_by: actor_id.clone(),
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                },
                SecretMaterial(input.value.into_bytes()),
            )
            .await
    }

    pub async fn rotate_secret(
        &self,
        org_id: &Id,
        actor_id: &Id,
        secret_id: &Id,
        value: String,
    ) -> Result<SyntheticSecret> {
        let secret = self.repository.get_secret(org_id, secret_id).await?;
        self.repository
            .rotate_secret(
                SyntheticSecretVersion {
                    secret_id: secret.id,
                    organization_id: org_id.clone(),
                    version: secret.current_version.saturating_add(1),
                    created_by: actor_id.clone(),
                    created_at: TimestampMicros::now(),
                },
                SecretMaterial(value.into_bytes()),
            )
            .await
    }

    pub async fn list_secrets(&self, org_id: &Id) -> Result<Vec<SyntheticSecret>> {
        self.repository.list_secrets(org_id).await
    }

    pub async fn create_monitor(
        &self,
        org_id: &Id,
        actor_id: &Id,
        input: CreateMonitorInput,
    ) -> Result<ActiveMonitorRevision> {
        validate_name(&input.name, 255, "Monitor name")?;
        validate_tags(&input.tags)?;
        let now = TimestampMicros::now();
        let monitor_id = Id::new();
        let revision = build_revision(org_id, actor_id, &monitor_id, 1, input.clone(), now)?;
        self.repository
            .create_monitor(
                SyntheticMonitor {
                    id: monitor_id,
                    organization_id: org_id.clone(),
                    name: input.name.trim().to_string(),
                    description: input.description.trim().to_string(),
                    kind: input.spec.kind(),
                    lifecycle: MonitorLifecycle::Draft,
                    state: MonitorState::Unknown,
                    team_id: input.team_id,
                    tags: input.tags,
                    active_revision_id: None,
                    draft_revision_id: Some(revision.id.clone()),
                    next_due_at: None,
                    created_by: actor_id.clone(),
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                },
                revision,
            )
            .await
    }

    pub async fn create_draft_revision(
        &self,
        org_id: &Id,
        actor_id: &Id,
        monitor_id: &Id,
        input: CreateMonitorInput,
    ) -> Result<MonitorRevision> {
        let monitor = self.repository.get_monitor(org_id, monitor_id).await?;
        if monitor.kind != input.spec.kind() {
            return Err(Error::invalid(
                "Monitor type cannot change between Revisions",
            ));
        }
        let number = self
            .repository
            .list_revisions(org_id, monitor_id)
            .await?
            .len() as u32
            + 1;
        let revision = build_revision(
            org_id,
            actor_id,
            monitor_id,
            number,
            input,
            TimestampMicros::now(),
        )?;
        self.repository.create_draft_revision(revision).await
    }

    pub async fn publish_revision(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
    ) -> Result<ActiveMonitorRevision> {
        let now = TimestampMicros::now();
        let revision = self
            .repository
            .get_revision(org_id, monitor_id, revision_id)
            .await?;
        validate_revision(&revision)?;
        let next = next_due_at(monitor_id, &revision.schedule, now)?;
        self.repository
            .activate_revision(org_id, monitor_id, revision_id, next, now)
            .await
    }

    pub async fn set_lifecycle(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        lifecycle: MonitorLifecycle,
    ) -> Result<SyntheticMonitor> {
        let monitor = self.repository.get_monitor(org_id, monitor_id).await?;
        let next = if lifecycle == MonitorLifecycle::Active {
            let active = self
                .repository
                .get_active_revision(org_id, monitor_id)
                .await?
                .ok_or_else(|| Error::conflict("Monitor has no published Revision"))?;
            next_due_at(
                monitor_id,
                &active.revision.schedule,
                TimestampMicros::now(),
            )?
        } else {
            None
        };
        self.repository
            .update_monitor_runtime(
                org_id,
                monitor_id,
                lifecycle,
                monitor.state,
                next,
                TimestampMicros::now(),
            )
            .await
    }

    pub async fn list_monitors(&self, org_id: &Id) -> Result<Vec<SyntheticMonitor>> {
        self.repository.list_monitors(org_id).await
    }

    pub async fn get_monitor(&self, org_id: &Id, monitor_id: &Id) -> Result<SyntheticMonitor> {
        self.repository.get_monitor(org_id, monitor_id).await
    }

    pub async fn get_revision(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
    ) -> Result<MonitorRevision> {
        self.repository
            .get_revision(org_id, monitor_id, revision_id)
            .await
    }

    pub async fn list_revisions(
        &self,
        org_id: &Id,
        monitor_id: &Id,
    ) -> Result<Vec<MonitorRevision>> {
        self.repository.list_revisions(org_id, monitor_id).await
    }
}
