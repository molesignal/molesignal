// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One node session (§5–§7, §9): Hello and reconciliation, heartbeats and renewals, offers, accepts
//! and rejects, results and their acknowledgements, configuration, drain and revoke.

use std::collections::HashSet;

use domain::node_agent::{
    Capabilities, CollectorState, ConfigState, ConfigStatus, Health, LeaseKey, NodeAgent, NodeTask,
    ProtocolVersion, RejectReason, SessionReport, TaskEvent, TaskStatus, Transition,
    task::ResultReport,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde_json::json;

use super::{Actor, NodeAgentService, enroll::unsupported_protocol};

/// Most leases or pending results one message may name.
const MAX_KEYS: usize = 256;

#[derive(Debug, Clone)]
pub struct Authenticated {
    pub node: NodeAgent,
    /// The session used the current certificate, not the one a rotation replaced.
    pub used_current: bool,
}

#[derive(Debug, Clone)]
pub struct HelloInput {
    pub node_id: String,
    pub protocol_version: String,
    pub agent_version: String,
    pub capabilities: Capabilities,
    pub applied_config_revision: u64,
    pub active_leases: Vec<LeaseKey>,
    pub pending_results: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HeartbeatInput {
    pub seq: u64,
    pub applied_config_revision: u64,
    pub health: Health,
    pub collector_states: Vec<CollectorState>,
    pub active_leases: Vec<LeaseKey>,
}

#[derive(Debug, Clone)]
pub struct TaskResultInput {
    pub result_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub status: TaskStatus,
    pub summary: String,
    pub artifact_refs: Vec<String>,
}

/// What the control plane sends, in order.
#[derive(Debug)]
pub enum Outbound {
    Welcome {
        protocol_version: String,
        heartbeat_interval_secs: u32,
        desired_config_revision: u64,
        server_time: TimestampMicros,
    },
    HeartbeatAck {
        seq: u64,
        desired_config_revision: u64,
        renewals: Vec<(LeaseKey, TimestampMicros)>,
        server_time: TimestampMicros,
    },
    ConfigUpdate {
        revision: u64,
        config: Vec<u8>,
        signature: Vec<u8>,
    },
    LeaseOffer {
        task: Box<NodeTask>,
        server_time: TimestampMicros,
    },
    LeaseCancel {
        key: LeaseKey,
        reason: String,
    },
    ResultAck {
        result_id: String,
    },
    Drain {
        deadline: TimestampMicros,
    },
    Revoke {
        reason: String,
    },
    /// End the session cleanly.
    End,
    /// End the session with this error.
    Close(Error),
}

/// The state of one live session.
#[derive(Debug, Clone)]
pub struct Session {
    pub org_id: Id,
    pub node_id: Id,
    capabilities: Capabilities,
    config_sent: u64,
    applied_config_revision: u64,
    drain_sent: bool,
    cancels_sent: HashSet<LeaseKey>,
}

impl Session {
    fn key_is_valid(key: &LeaseKey) -> bool {
        !key.task_id.as_str().is_empty() && key.task_id.as_str().len() <= 64 && key.attempt > 0
    }
}

fn cancel(key: LeaseKey, reason: &str) -> Outbound {
    Outbound::LeaseCancel {
        key,
        reason: reason.into(),
    }
}

impl NodeAgentService {
    /// Answers a `Hello`: `None` and closing messages when the session cannot go on.
    pub async fn open_session(
        &self,
        auth: Authenticated,
        hello: HelloInput,
    ) -> Result<(Option<Session>, Vec<Outbound>)> {
        let node = auth.node;
        let now = TimestampMicros::now();

        if hello.node_id != node.id.as_str() {
            return Err(Error::unauthorized(
                "the Hello names another node than the certificate",
            ));
        }

        let version = ProtocolVersion::parse(&hello.protocol_version)?;

        if !version.compatible_with(ProtocolVersion::CURRENT) {
            let welcome = self.welcome(node.desired_config_revision(), now);
            let close = Outbound::Close(unsupported_protocol(&hello.protocol_version));
            return Ok((None, vec![welcome, close]));
        }

        if let Some(reason) = &node.revoke_reason {
            let revoke = Outbound::Revoke {
                reason: reason.clone(),
            };

            return Ok((None, vec![revoke, Outbound::End]));
        }

        if node.certificate_expires_at <= now {
            return Err(Error::unauthorized("the node certificate expired"));
        }

        let report = SessionReport {
            agent_version: Some(hello.agent_version),
            protocol_version: Some(hello.protocol_version),
            capabilities: Some(hello.capabilities.clone()),
            applied_config_revision: hello.applied_config_revision,
            health: None,
            collector_states: None,
            connected: true,
            certificate_confirmed: auth.used_current,
        };

        let node = self
            .repository
            .record_session(&node.org_id, &node.id, &report, now)
            .await?;

        let mut session = Session {
            org_id: node.org_id.clone(),
            node_id: node.id.clone(),
            capabilities: hello.capabilities,
            config_sent: 0,
            applied_config_revision: hello.applied_config_revision,
            drain_sent: false,
            cancels_sent: HashSet::new(),
        };

        let mut out = vec![self.welcome(node.desired_config_revision(), now)];

        // Reconciliation: leases the control plane no longer gives the node are cancelled; the
        // others are renewed.
        let (_, cancels) = self.renew(&session, hello.active_leases, now).await?;

        for message in &cancels {
            if let Outbound::LeaseCancel { key, .. } = message {
                session.cancels_sent.insert(key.clone());
            }
        }

        out.extend(cancels);

        for result_id in hello.pending_results.into_iter().take(MAX_KEYS) {
            if self.result_recorded(&session, &result_id).await? {
                out.push(Outbound::ResultAck { result_id });
            }
        }

        out.extend(self.poll(&mut session).await?);
        Ok((Some(session), out))
    }

    fn welcome(&self, desired_config_revision: u64, now: TimestampMicros) -> Outbound {
        Outbound::Welcome {
            protocol_version: ProtocolVersion::CURRENT.to_string(),
            heartbeat_interval_secs: self.timings.heartbeat_interval_secs,
            desired_config_revision,
            server_time: now,
        }
    }

    pub async fn heartbeat(
        &self,
        session: &mut Session,
        input: HeartbeatInput,
    ) -> Result<Vec<Outbound>> {
        let now = TimestampMicros::now();

        let report = SessionReport {
            applied_config_revision: input.applied_config_revision,
            health: Some(input.health),
            collector_states: Some(input.collector_states.into_iter().take(64).collect()),
            ..SessionReport::default()
        };

        let node = self
            .repository
            .record_session(&session.org_id, &session.node_id, &report, now)
            .await?;

        session.applied_config_revision = input.applied_config_revision;
        let (renewals, cancels) = self.renew(session, input.active_leases, now).await?;

        let mut out = vec![Outbound::HeartbeatAck {
            seq: input.seq,
            desired_config_revision: node.desired_config_revision(),
            renewals,
            server_time: now,
        }];

        out.extend(cancels);
        Ok(out)
    }

    /// Renews the leases a node holds; those it should not hold get a cancel.
    async fn renew(
        &self,
        session: &Session,
        leases: Vec<LeaseKey>,
        now: TimestampMicros,
    ) -> Result<(Vec<(LeaseKey, TimestampMicros)>, Vec<Outbound>)> {
        let mut renewals = Vec::new();
        let mut cancels = Vec::new();

        for key in leases
            .into_iter()
            .filter(Session::key_is_valid)
            .take(MAX_KEYS)
        {
            let event = TaskEvent::Renew {
                attempt: key.attempt,
                lease_ttl_micros: self.timings.lease_ttl_micros,
            };

            let applied = self
                .repository
                .apply_task_event(
                    &session.org_id,
                    Some(&session.node_id),
                    &key.task_id,
                    event,
                    now,
                )
                .await?;

            match applied {
                Some((_, Transition::Leased { lease_expires_at })) => {
                    renewals.push((key, lease_expires_at));
                }
                _ => cancels.push(cancel(key, "the control plane no longer holds this lease")),
            }
        }

        Ok((renewals, cancels))
    }

    pub async fn lease_accept(&self, session: &Session, key: LeaseKey) -> Result<Vec<Outbound>> {
        if !Session::key_is_valid(&key) {
            return Err(Error::invalid("invalid lease key"));
        }

        let event = TaskEvent::Accept {
            attempt: key.attempt,
            lease_ttl_micros: self.timings.lease_ttl_micros,
        };

        let applied = self
            .repository
            .apply_task_event(
                &session.org_id,
                Some(&session.node_id),
                &key.task_id,
                event,
                TimestampMicros::now(),
            )
            .await?;

        Ok(match applied {
            Some((_, Transition::Leased { .. })) => Vec::new(),
            _ => vec![cancel(key, "the lease is no longer offered")],
        })
    }

    pub async fn lease_reject(
        &self,
        session: &Session,
        key: LeaseKey,
        reason: RejectReason,
    ) -> Result<Vec<Outbound>> {
        if !Session::key_is_valid(&key) {
            return Err(Error::invalid("invalid lease key"));
        }

        let event = TaskEvent::Reject {
            attempt: key.attempt,
            reason,
        };

        self.repository
            .apply_task_event(
                &session.org_id,
                Some(&session.node_id),
                &key.task_id,
                event,
                TimestampMicros::now(),
            )
            .await?;

        Ok(Vec::new())
    }

    /// Records a result once. Every well-formed result is acknowledged, so the node stops resending
    /// it, even when it is a repeat or names a task this node does not hold.
    pub async fn task_result(
        &self,
        session: &Session,
        input: TaskResultInput,
    ) -> Result<Vec<Outbound>> {
        let key = LeaseKey {
            task_id: Id::from_string(input.task_id),
            attempt: input.attempt,
        };

        if !Session::key_is_valid(&key) || key.result_id() != input.result_id {
            return Err(Error::invalid("result_id must be <task_id>/<attempt>"));
        }

        let report = ResultReport {
            result_id: input.result_id.clone(),
            attempt: input.attempt,
            status: input.status,
            summary: input.summary.chars().take(16 * 1024).collect(),
            artifact_refs: input
                .artifact_refs
                .into_iter()
                .filter(|reference| reference.len() <= 512)
                .take(64)
                .collect(),
        };

        let applied = self
            .repository
            .apply_task_event(
                &session.org_id,
                Some(&session.node_id),
                &key.task_id,
                TaskEvent::Result(report),
                TimestampMicros::now(),
            )
            .await?;

        if applied.is_none() {
            tracing::warn!(
                node_id = session.node_id.as_str(),
                result_id = input.result_id,
                "a node reported a result of a task it does not hold"
            );
        }

        Ok(vec![Outbound::ResultAck {
            result_id: input.result_id,
        }])
    }

    async fn result_recorded(&self, session: &Session, result_id: &str) -> Result<bool> {
        let Some(key) = LeaseKey::parse_result_id(result_id).filter(Session::key_is_valid) else {
            return Ok(false);
        };

        let task = match self
            .repository
            .get_task(&session.org_id, &key.task_id)
            .await
        {
            Ok(task) => task,
            Err(Error::NotFound(_)) => return Ok(false),
            Err(error) => return Err(error),
        };

        Ok(task.node_id == session.node_id
            && task
                .result
                .is_some_and(|result| result.result_id == result_id))
    }

    pub async fn config_status(
        &self,
        session: &Session,
        revision: u64,
        state: ConfigState,
        reason: Option<String>,
    ) -> Result<Vec<Outbound>> {
        let status = ConfigStatus {
            revision,
            state,
            reason: reason.map(|reason| reason.chars().take(1024).collect()),
            reported_at: TimestampMicros::now(),
        };

        self.repository
            .record_config_status(&session.org_id, &session.node_id, &status)
            .await?;

        Ok(Vec::new())
    }

    pub async fn drained(&self, session: &Session) -> Result<Vec<Outbound>> {
        self.repository
            .mark_drained(&session.org_id, &session.node_id, TimestampMicros::now())
            .await?;

        self.audit
            .record(
                &session.org_id,
                &Actor::node(session.node_id.as_str()),
                "node_agent.drained",
                ("node_agent", session.node_id.as_str()),
                json!({}),
            )
            .await;

        Ok(Vec::new())
    }

    /// What changed since the last look: revocation, drain, configuration, cancels and new offers.
    pub async fn poll(&self, session: &mut Session) -> Result<Vec<Outbound>> {
        let now = TimestampMicros::now();
        let node = self
            .repository
            .get_node(&session.org_id, &session.node_id)
            .await?;

        if let Some(reason) = node.revoke_reason {
            return Ok(vec![Outbound::Revoke { reason }, Outbound::End]);
        }

        if node.certificate_expires_at <= now {
            return Ok(vec![Outbound::Close(Error::unauthorized(
                "the node certificate expired",
            ))]);
        }

        let mut out = Vec::new();

        match (node.drain_deadline, session.drain_sent) {
            (Some(deadline), false) => {
                session.drain_sent = true;
                out.push(Outbound::Drain { deadline });
            }
            // The drain was lifted: a new session starts the node over without it.
            (None, true) => {
                return Ok(vec![Outbound::Close(Error::unavailable(
                    "the drain was lifted; reconnect",
                ))]);
            }
            _ => {}
        }

        if let Some(desired) = &node.desired_config
            && desired.revision > session.config_sent.max(session.applied_config_revision)
        {
            let config = serde_json::to_vec(&desired.config)
                .map_err(|error| Error::internal(format!("encode node config: {error}")))?;

            let signature = self.authority.sign_config(desired.revision, &config)?;
            session.config_sent = desired.revision;

            out.push(Outbound::ConfigUpdate {
                revision: desired.revision,
                config,
                signature,
            });
        }

        let cancels = self
            .repository
            .cancel_requested_leases(&session.org_id, &session.node_id)
            .await?;

        for key in cancels {
            if session.cancels_sent.insert(key.clone()) {
                out.push(cancel(key, "the task was cancelled"));
            }
        }

        if node.drain_requested_at.is_none() {
            let quota = self.repository.task_quota(&session.org_id).await?;
            let max_in_flight = quota.node_slots(session.capabilities.max_running_tasks, 0);

            let offered = self
                .repository
                .offer_tasks(
                    &session.org_id,
                    &session.node_id,
                    &session.capabilities.task_kinds,
                    max_in_flight,
                    self.timings.offer_ttl_micros,
                    now,
                )
                .await?;

            out.extend(offered.into_iter().map(|task| Outbound::LeaseOffer {
                task: Box::new(task),
                server_time: now,
            }));
        }

        Ok(out)
    }
}
