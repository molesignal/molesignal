// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use async_trait::async_trait;

use super::{
    ActiveMonitorRevision, LocationStateUpdate, MonitorLifecycle, MonitorLocationState,
    MonitorRevision, MonitorState, ProbeAgent, ProbeAgentToken, ProbeLocation, ProbeRegisterToken,
    ProbeRegistrationGrant, ProbeTask, SecretMaterial, StateObservation, SyntheticMonitor,
    SyntheticResult, SyntheticResultListQuery, SyntheticResultPage, SyntheticSecret,
    SyntheticSecretVersion, SyntheticStateTransition,
};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[async_trait]
pub trait SyntheticLocationRepository: Send + Sync {
    async fn create_location(&self, location: ProbeLocation) -> Result<ProbeLocation>;
    async fn update_location(&self, location: ProbeLocation) -> Result<ProbeLocation>;
    async fn get_location(&self, org_id: &Id, location_id: &Id) -> Result<ProbeLocation>;
    async fn list_locations(&self, org_id: &Id) -> Result<Vec<ProbeLocation>>;
}

#[async_trait]
pub trait SyntheticAgentRepository: Send + Sync {
    async fn create_agent(&self, agent: ProbeAgent) -> Result<ProbeAgent>;
    async fn update_agent(&self, agent: ProbeAgent) -> Result<ProbeAgent>;
    async fn update_agent_configuration(
        &self,
        org_id: &Id,
        actor_id: &Id,
        agent_id: &Id,
        name: &str,
        labels: &BTreeMap<String, String>,
        updated_at: TimestampMicros,
    ) -> Result<ProbeAgent>;
    async fn get_agent(&self, org_id: &Id, agent_id: &Id) -> Result<ProbeAgent>;
    async fn get_agent_by_certificate_serial(&self, serial: &str) -> Result<ProbeAgent>;
    async fn list_all_agents(&self, org_id: &Id) -> Result<Vec<ProbeAgent>>;
    async fn list_agents(&self, org_id: &Id, location_id: &Id) -> Result<Vec<ProbeAgent>>;
}

#[async_trait]
pub trait SyntheticRegisterRepository: Send + Sync {
    async fn create_register_token(
        &self,
        token: ProbeRegisterToken,
        token_hash: Vec<u8>,
    ) -> Result<ProbeRegisterToken>;
    async fn get_registration_grant(
        &self,
        token_hash: &[u8],
        now: TimestampMicros,
    ) -> Result<ProbeRegistrationGrant>;
    async fn register_probe(
        &self,
        token_hash: &[u8],
        agent: ProbeAgent,
        consumed_at: TimestampMicros,
    ) -> Result<ProbeAgent>;
}

#[async_trait]
pub trait SyntheticAgentTokenRepository: Send + Sync {
    async fn create_agent_token(
        &self,
        token: ProbeAgentToken,
        token_hash: Vec<u8>,
    ) -> Result<ProbeAgentToken>;
    async fn list_agent_tokens(&self, org_id: &Id) -> Result<Vec<ProbeAgentToken>>;
    async fn rotate_agent_token(
        &self,
        org_id: &Id,
        token_id: &Id,
        token_hash: Vec<u8>,
        token_prefix: &str,
        expires_at: Option<TimestampMicros>,
        rotated_at: TimestampMicros,
    ) -> Result<ProbeAgentToken>;
    async fn disable_agent_token(
        &self,
        org_id: &Id,
        token_id: &Id,
        disabled_at: TimestampMicros,
    ) -> Result<ProbeAgentToken>;
}

#[async_trait]
pub trait SyntheticSecretRepository: Send + Sync {
    async fn create_secret(
        &self,
        secret: SyntheticSecret,
        material: SecretMaterial,
    ) -> Result<SyntheticSecret>;
    async fn rotate_secret(
        &self,
        version: SyntheticSecretVersion,
        material: SecretMaterial,
    ) -> Result<SyntheticSecret>;
    async fn resolve_secret(
        &self,
        org_id: &Id,
        secret_id: &Id,
        version: Option<u32>,
    ) -> Result<(SyntheticSecretVersion, SecretMaterial)>;
    async fn get_secret(&self, org_id: &Id, secret_id: &Id) -> Result<SyntheticSecret>;
    async fn list_secrets(&self, org_id: &Id) -> Result<Vec<SyntheticSecret>>;
}

#[async_trait]
pub trait SyntheticMonitorRepository: Send + Sync {
    async fn create_monitor(
        &self,
        monitor: SyntheticMonitor,
        revision: MonitorRevision,
    ) -> Result<ActiveMonitorRevision>;
    async fn create_draft_revision(&self, revision: MonitorRevision) -> Result<MonitorRevision>;
    async fn get_revision(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
    ) -> Result<MonitorRevision>;
    async fn list_revisions(&self, org_id: &Id, monitor_id: &Id) -> Result<Vec<MonitorRevision>>;
    async fn activate_revision(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
        next_due_at: Option<TimestampMicros>,
        updated_at: TimestampMicros,
    ) -> Result<ActiveMonitorRevision>;
    async fn mark_revision_tested(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
        result_id: &Id,
        passed_at: TimestampMicros,
    ) -> Result<MonitorRevision>;
    async fn update_monitor_runtime(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        lifecycle: MonitorLifecycle,
        state: MonitorState,
        next_due_at: Option<TimestampMicros>,
        updated_at: TimestampMicros,
    ) -> Result<SyntheticMonitor>;
    async fn get_monitor(&self, org_id: &Id, monitor_id: &Id) -> Result<SyntheticMonitor>;
    async fn get_active_revision(
        &self,
        org_id: &Id,
        monitor_id: &Id,
    ) -> Result<Option<ActiveMonitorRevision>>;
    async fn list_monitors(&self, org_id: &Id) -> Result<Vec<SyntheticMonitor>>;
    async fn list_due_monitors(
        &self,
        due_at: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<ActiveMonitorRevision>>;
}

#[async_trait]
pub trait SyntheticTaskRepository: Send + Sync {
    async fn create_tasks(&self, tasks: Vec<ProbeTask>) -> Result<Vec<ProbeTask>>;
    async fn lease_task(
        &self,
        agent: &ProbeAgent,
        now: TimestampMicros,
        leased_until: TimestampMicros,
        lease_token_hash: String,
    ) -> Result<Option<ProbeTask>>;
    async fn complete_task(
        &self,
        org_id: &Id,
        task_id: &Id,
        agent_id: Option<&Id>,
        now: TimestampMicros,
    ) -> Result<()>;
    async fn expire_leases(&self, now: TimestampMicros, limit: u32) -> Result<u64>;
    async fn get_leased_task(
        &self,
        task_id: &Id,
        agent_id: &Id,
        lease_token_hash: &str,
        now: TimestampMicros,
    ) -> Result<ProbeTask>;
    async fn get_leased_task_by_token(
        &self,
        task_id: &Id,
        lease_token_hash: &str,
        now: TimestampMicros,
    ) -> Result<ProbeTask>;
    async fn acknowledge_task(
        &self,
        task_id: &Id,
        agent_id: &Id,
        lease_token_hash: &str,
        accepted: bool,
        now: TimestampMicros,
    ) -> Result<()>;
    async fn renew_task_lease(
        &self,
        task_id: &Id,
        agent_id: &Id,
        lease_token_hash: &str,
        requested_until: TimestampMicros,
        now: TimestampMicros,
    ) -> Result<TimestampMicros>;
}

#[async_trait]
pub trait SyntheticResultRepository: Send + Sync {
    async fn record_result(&self, result: SyntheticResult) -> Result<(SyntheticResult, bool)>;
    async fn list_results(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        before: Option<TimestampMicros>,
        limit: u32,
    ) -> Result<Vec<SyntheticResult>>;
    async fn list_results_page(
        &self,
        org_id: &Id,
        query: &SyntheticResultListQuery,
    ) -> Result<SyntheticResultPage>;
    async fn get_result(&self, org_id: &Id, result_id: &Id) -> Result<SyntheticResult>;
}

#[async_trait]
pub trait SyntheticStateRepository: Send + Sync {
    async fn apply_location_observation(
        &self,
        observation: StateObservation,
    ) -> Result<LocationStateUpdate>;
    async fn list_location_states(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
    ) -> Result<Vec<MonitorLocationState>>;
    async fn record_state_transition(
        &self,
        transition: SyntheticStateTransition,
    ) -> Result<SyntheticStateTransition>;
    async fn list_state_transitions(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        before: Option<TimestampMicros>,
        limit: u32,
    ) -> Result<Vec<SyntheticStateTransition>>;
}

pub trait SyntheticRepository:
    SyntheticLocationRepository
    + SyntheticAgentRepository
    + SyntheticAgentTokenRepository
    + SyntheticRegisterRepository
    + SyntheticSecretRepository
    + SyntheticMonitorRepository
    + SyntheticTaskRepository
    + SyntheticResultRepository
    + SyntheticStateRepository
{
}

impl<T> SyntheticRepository for T where
    T: SyntheticLocationRepository
        + SyntheticAgentRepository
        + SyntheticAgentTokenRepository
        + SyntheticRegisterRepository
        + SyntheticSecretRepository
        + SyntheticMonitorRepository
        + SyntheticTaskRepository
        + SyntheticResultRepository
        + SyntheticStateRepository
{
}
