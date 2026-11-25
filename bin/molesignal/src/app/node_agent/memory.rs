// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! In-memory repositories for tests: the same scoping and transitions as Postgres, one lock.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Mutex,
};

use async_trait::async_trait;
use domain::node_agent::{
    ConfigStatus, DesiredConfig, EnrollmentToken, LeaseKey, NodeAgent, NodeAgentRepository,
    NodeTask, QuotaUsage, SessionReport, TaskEvent, TaskFilter, TaskKind, TaskQuota, TaskState,
    Transition,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use postgres::repositories::audit_events::{
    AuditEvent, AuditEventRepository, AuditQuery, AuditRangeSummary,
};
use serde_json::Value;

#[derive(Default)]
struct State {
    tokens: Vec<(Vec<u8>, EnrollmentToken)>,
    nodes: Vec<NodeAgent>,
    tasks: BTreeMap<String, NodeTask>,
    quotas: HashMap<String, TaskQuota>,
}

#[derive(Default)]
pub(crate) struct MemoryRepository {
    state: Mutex<State>,
}

fn not_found(what: &str) -> Error {
    Error::not_found(format!("{what} not found"))
}

impl State {
    fn node_mut(&mut self, org_id: &Id, node_id: &Id) -> Result<&mut NodeAgent> {
        self.nodes
            .iter_mut()
            .find(|node| &node.org_id == org_id && &node.id == node_id)
            .ok_or_else(|| not_found("node"))
    }

    fn live_node_mut(&mut self, org_id: &Id, node_id: &Id) -> Result<&mut NodeAgent> {
        self.node_mut(org_id, node_id).and_then(|node| {
            (!node.is_revoked())
                .then_some(node)
                .ok_or_else(|| not_found("node"))
        })
    }

    fn end_node_tasks(
        &mut self,
        org_id: &Id,
        node_id: &Id,
        reason: &str,
        now: TimestampMicros,
    ) -> u64 {
        let mut ended = 0;

        for task in self.tasks.values_mut() {
            if &task.org_id == org_id && &task.node_id == node_id {
                let event = TaskEvent::NodeGone {
                    reason: reason.into(),
                };

                ended += u64::from(task.apply(event, now).changed());
            }
        }

        ended
    }
}

#[async_trait]
impl NodeAgentRepository for MemoryRepository {
    async fn create_enrollment_token(
        &self,
        token: &EnrollmentToken,
        token_hash: &[u8],
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.tokens.push((token_hash.to_vec(), token.clone()));
        Ok(())
    }

    async fn list_enrollment_tokens(&self, org_id: &Id) -> Result<Vec<EnrollmentToken>> {
        let state = self.state.lock().unwrap();

        Ok(state
            .tokens
            .iter()
            .filter(|(_, token)| &token.org_id == org_id)
            .map(|(_, token)| token.clone())
            .collect())
    }

    async fn revoke_enrollment_token(
        &self,
        org_id: &Id,
        token_id: &Id,
        now: TimestampMicros,
    ) -> Result<EnrollmentToken> {
        let mut state = self.state.lock().unwrap();

        let (_, token) = state
            .tokens
            .iter_mut()
            .find(|(_, token)| &token.org_id == org_id && &token.id == token_id)
            .ok_or_else(|| not_found("enrollment token"))?;

        token.revoked_at.get_or_insert(now);
        Ok(token.clone())
    }

    async fn enrollment_token_by_hash(&self, token_hash: &[u8]) -> Result<Option<EnrollmentToken>> {
        let state = self.state.lock().unwrap();

        Ok(state
            .tokens
            .iter()
            .find(|(hash, _)| hash == token_hash)
            .map(|(_, token)| token.clone()))
    }

    async fn node_by_host(&self, org_id: &Id, host_id: &str) -> Result<Option<NodeAgent>> {
        let state = self.state.lock().unwrap();

        Ok(state
            .nodes
            .iter()
            .find(|node| &node.org_id == org_id && node.host_id == host_id)
            .cloned())
    }

    async fn enroll_node(
        &self,
        token_hash: &[u8],
        node: &NodeAgent,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        let mut state = self.state.lock().unwrap();

        let (_, token) = state
            .tokens
            .iter_mut()
            .find(|(hash, token)| hash == token_hash && token.org_id == node.org_id)
            .ok_or_else(|| Error::unauthorized("invalid or expired enrollment token"))?;

        token.check_usable(now)?;
        token.uses += 1;
        token.last_used_at = Some(now);
        let token_id = token.id.clone();

        let stored = match state
            .nodes
            .iter_mut()
            .find(|existing| existing.org_id == node.org_id && existing.host_id == node.host_id)
        {
            Some(existing) => {
                let mut replaced = node.clone();
                replaced.id = existing.id.clone();
                replaced.desired_config = existing.desired_config.clone();
                replaced.applied_config_revision = existing.applied_config_revision;
                replaced.created_at = existing.created_at;
                replaced.enrollment_token_id = Some(token_id);
                *existing = replaced.clone();
                replaced
            }
            None => {
                let mut created = node.clone();
                created.enrollment_token_id = Some(token_id);
                state.nodes.push(created.clone());
                created
            }
        };

        state.end_node_tasks(&stored.org_id, &stored.id, "the node enrolled again", now);
        Ok(stored)
    }

    async fn node_by_certificate_serial(&self, serial: &str) -> Result<Option<NodeAgent>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .nodes
            .iter()
            .find(|node| node.accepts_certificate(serial))
            .cloned())
    }

    async fn get_node(&self, org_id: &Id, node_id: &Id) -> Result<NodeAgent> {
        self.state
            .lock()
            .unwrap()
            .node_mut(org_id, node_id)
            .map(|node| node.clone())
    }

    async fn list_nodes(&self, org_id: &Id) -> Result<Vec<NodeAgent>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .nodes
            .iter()
            .filter(|node| &node.org_id == org_id)
            .cloned()
            .collect())
    }

    async fn record_session(
        &self,
        org_id: &Id,
        node_id: &Id,
        report: &SessionReport,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        let mut state = self.state.lock().unwrap();
        let node = state.live_node_mut(org_id, node_id)?;

        if let Some(value) = &report.agent_version {
            node.agent_version = value.clone();
        }

        if let Some(value) = &report.protocol_version {
            node.protocol_version = value.clone();
        }

        if let Some(value) = &report.capabilities {
            node.capabilities = value.clone();
        }

        if let Some(value) = &report.health {
            node.health = value.clone();
        }

        if let Some(value) = &report.collector_states {
            node.collector_states = value.clone();
        }

        node.applied_config_revision = report.applied_config_revision;
        node.last_heartbeat_at = Some(now);

        if report.connected {
            node.last_connected_at = Some(now);
        }

        if report.certificate_confirmed {
            node.previous_certificate_serial = None;
        }

        node.updated_at = now;
        Ok(node.clone())
    }

    async fn rotate_certificate(
        &self,
        org_id: &Id,
        node_id: &Id,
        serial: &str,
        expires_at: TimestampMicros,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        let mut state = self.state.lock().unwrap();
        let node = state.live_node_mut(org_id, node_id)?;
        node.previous_certificate_serial = Some(std::mem::replace(
            &mut node.certificate_serial,
            serial.into(),
        ));
        node.certificate_expires_at = expires_at;
        node.updated_at = now;
        Ok(node.clone())
    }

    async fn set_drain(
        &self,
        org_id: &Id,
        node_id: &Id,
        deadline: Option<TimestampMicros>,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        let mut state = self.state.lock().unwrap();
        let node = state.live_node_mut(org_id, node_id)?;

        node.drain_requested_at = deadline.map(|_| node.drain_requested_at.unwrap_or(now));
        node.drain_deadline = deadline;
        node.drained_at = None;
        node.updated_at = now;
        Ok(node.clone())
    }

    async fn mark_drained(&self, org_id: &Id, node_id: &Id, now: TimestampMicros) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let node = state.live_node_mut(org_id, node_id)?;
        node.drained_at = node.drain_requested_at.map(|_| now);
        Ok(())
    }

    async fn revoke_node(
        &self,
        org_id: &Id,
        node_id: &Id,
        reason: &str,
        now: TimestampMicros,
    ) -> Result<(NodeAgent, u64)> {
        let mut state = self.state.lock().unwrap();
        let node = state.node_mut(org_id, node_id)?;
        node.revoked_at.get_or_insert(now);
        node.revoke_reason.get_or_insert_with(|| reason.into());
        let node = node.clone();
        let ended = state.end_node_tasks(org_id, node_id, "the node was revoked", now);
        Ok((node, ended))
    }

    async fn set_desired_config(
        &self,
        org_id: &Id,
        node_id: &Id,
        config: Value,
        updated_by: &str,
        now: TimestampMicros,
    ) -> Result<NodeAgent> {
        let mut state = self.state.lock().unwrap();
        let node = state.live_node_mut(org_id, node_id)?;

        node.desired_config = Some(DesiredConfig {
            revision: node.desired_config_revision() + 1,
            config,
            updated_by: Some(updated_by.into()),
            updated_at: now,
        });

        Ok(node.clone())
    }

    async fn record_config_status(
        &self,
        org_id: &Id,
        node_id: &Id,
        status: &ConfigStatus,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.live_node_mut(org_id, node_id)?.config_status = Some(status.clone());
        Ok(())
    }

    async fn task_quota(&self, org_id: &Id) -> Result<TaskQuota> {
        let state = self.state.lock().unwrap();
        Ok(state
            .quotas
            .get(org_id.as_str())
            .copied()
            .unwrap_or_default())
    }

    async fn set_task_quota(
        &self,
        org_id: &Id,
        quota: &TaskQuota,
        _: &str,
        _: TimestampMicros,
    ) -> Result<()> {
        self.state
            .lock()
            .unwrap()
            .quotas
            .insert(org_id.0.clone(), *quota);
        Ok(())
    }

    async fn create_task(&self, task: &NodeTask, quota: &TaskQuota) -> Result<NodeTask> {
        let mut state = self.state.lock().unwrap();
        let node = state.node_mut(&task.org_id, &task.node_id)?;

        if node.is_revoked() {
            return Err(Error::conflict("the node is revoked"));
        }

        if node.is_draining() {
            return Err(Error::conflict(
                "the node is draining and takes no new tasks",
            ));
        }

        let window_start = task.created_at.0 - TaskQuota::WINDOW_MICROS;
        let mut usage = QuotaUsage::default();

        for other in state
            .tasks
            .values()
            .filter(|other| other.org_id == task.org_id)
        {
            let same_node = other.node_id == task.node_id;
            let active = other.state.is_active();
            let recent = other.created_at.0 > window_start;
            usage.org_active += u32::from(active);
            usage.node_active += u32::from(active && same_node);
            usage.org_created_last_hour += u32::from(recent);
            usage.node_created_last_hour += u32::from(recent && same_node);
        }

        quota.admit(usage)?;
        state.tasks.insert(task.id.0.clone(), task.clone());
        Ok(task.clone())
    }

    async fn get_task(&self, org_id: &Id, task_id: &Id) -> Result<NodeTask> {
        let state = self.state.lock().unwrap();

        state
            .tasks
            .get(task_id.as_str())
            .filter(|task| &task.org_id == org_id)
            .cloned()
            .ok_or_else(|| not_found("task"))
    }

    async fn list_tasks(&self, org_id: &Id, filter: &TaskFilter) -> Result<Vec<NodeTask>> {
        let state = self.state.lock().unwrap();

        Ok(state
            .tasks
            .values()
            .filter(|task| &task.org_id == org_id)
            .filter(|task| {
                filter
                    .node_id
                    .as_ref()
                    .is_none_or(|node| &task.node_id == node)
            })
            .filter(|task| filter.states.is_empty() || filter.states.contains(&task.state))
            .take(filter.limit.max(1) as usize)
            .cloned()
            .collect())
    }

    async fn apply_task_event(
        &self,
        org_id: &Id,
        node_id: Option<&Id>,
        task_id: &Id,
        event: TaskEvent,
        now: TimestampMicros,
    ) -> Result<Option<(NodeTask, Transition)>> {
        let mut state = self.state.lock().unwrap();

        let Some(task) = state.tasks.get_mut(task_id.as_str()).filter(|task| {
            &task.org_id == org_id && node_id.is_none_or(|node| &task.node_id == node)
        }) else {
            return Ok(None);
        };

        let transition = task.apply(event, now);
        Ok(Some((task.clone(), transition)))
    }

    async fn offer_tasks(
        &self,
        org_id: &Id,
        node_id: &Id,
        kinds: &[TaskKind],
        max_in_flight: u32,
        lease_ttl_micros: i64,
        now: TimestampMicros,
    ) -> Result<Vec<NodeTask>> {
        let mut state = self.state.lock().unwrap();
        let node = state.node_mut(org_id, node_id)?;

        if node.is_revoked() || node.is_draining() {
            return Ok(Vec::new());
        }

        let mine = |task: &NodeTask| &task.org_id == org_id && &task.node_id == node_id;

        let in_flight = state
            .tasks
            .values()
            .filter(|task| {
                mine(task) && matches!(task.state, TaskState::Offered | TaskState::Running)
            })
            .count() as u32;

        let mut slots = max_in_flight.saturating_sub(in_flight);
        let mut offered = Vec::new();

        let mut queued = state
            .tasks
            .values_mut()
            .filter(|task| {
                mine(task) && task.state == TaskState::Queued && kinds.contains(&task.kind)
            })
            .collect::<Vec<_>>();

        queued.sort_by_key(|task| task.created_at);

        for task in queued {
            if slots == 0 {
                break;
            }

            if task
                .apply(TaskEvent::Offer { lease_ttl_micros }, now)
                .changed()
            {
                slots -= 1;
                offered.push(task.clone());
            }
        }

        Ok(offered)
    }

    async fn cancel_requested_leases(&self, org_id: &Id, node_id: &Id) -> Result<Vec<LeaseKey>> {
        let state = self.state.lock().unwrap();

        Ok(state
            .tasks
            .values()
            .filter(|task| &task.org_id == org_id && &task.node_id == node_id)
            .filter(|task| matches!(task.state, TaskState::Offered | TaskState::Running))
            .filter(|task| task.cancel_requested_at.is_some())
            .map(NodeTask::lease_key)
            .collect())
    }

    async fn expire_due_tasks(
        &self,
        org_id: Option<&Id>,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<NodeTask>> {
        let mut state = self.state.lock().unwrap();
        let mut changed = Vec::new();

        for task in state.tasks.values_mut() {
            if changed.len() as u32 >= limit || org_id.is_some_and(|org| &task.org_id != org) {
                continue;
            }

            if task.apply(TaskEvent::Expire, now).changed() {
                changed.push(task.clone());
            }
        }

        Ok(changed)
    }
}

/// Audit events kept in memory.
#[derive(Default)]
pub(crate) struct MemoryAudit {
    pub(crate) events: Mutex<Vec<AuditEvent>>,
}

impl MemoryAudit {
    pub(crate) fn actions(&self, org_id: &Id) -> Vec<String> {
        let events = self.events.lock().unwrap();

        events
            .iter()
            .filter(|event| &event.org_id == org_id)
            .map(|event| event.action.clone())
            .collect()
    }
}

#[async_trait]
impl AuditEventRepository for MemoryAudit {
    async fn record(&self, event: AuditEvent) -> Result<()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }

    async fn list_recent(&self, _: &Id, _: i64) -> Result<Vec<AuditEvent>> {
        Ok(Vec::new())
    }

    async fn query(&self, _: &Id, _: &AuditQuery) -> Result<Vec<AuditEvent>> {
        Ok(Vec::new())
    }

    async fn delete_before(&self, _: TimestampMicros, _: i64) -> Result<u64> {
        Ok(0)
    }

    async fn summarize_range(
        &self,
        _: Option<TimestampMicros>,
        _: TimestampMicros,
    ) -> Result<AuditRangeSummary> {
        Ok(AuditRangeSummary::default())
    }
}
