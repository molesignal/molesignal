// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Runs that did not happen.
//!
//! A task no Agent took before its deadline is expired and reported as a skipped run, with the
//! reason the location gave: no Agent online, none that runs that kind of check, none recent
//! enough to run it as written, or all of them busy. Without it the task would stay pending, the
//! check would stop running at that location, and nothing would say so.

use std::collections::BTreeMap;

use super::{PROBE_PROTOCOL_VERSION, SyntheticService, dispatch::AGENT_SILENCE_MICROS};
use crate::{
    domain::synthetics::{
        AgentStatus, ProbeAgent, ProbeCapability, ProbeOutcome, ProbeTask, SyntheticResult,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// Why no Agent took a task. The words are what the result's `skip_reason` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    NoAgentOnline,
    NoCapableAgent,
    AgentTooOld,
    AgentsBusy,
}

impl SkipReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoAgentOnline => "no_agent_online",
            Self::NoCapableAgent => "no_capable_agent",
            Self::AgentTooOld => "agent_too_old",
            Self::AgentsBusy => "agents_busy",
        }
    }
}

impl SyntheticService {
    /// Expires the tasks no Agent took in time and records each as a skipped run.
    pub async fn expire_unclaimed_tasks(&self, now: TimestampMicros, limit: u32) -> Result<u64> {
        let tasks = self
            .repository
            .expire_unclaimed(now, limit.min(1000))
            .await?;
        let mut recorded = 0;

        for task in tasks {
            let agents = self
                .repository
                .list_agents(&task.organization_id, &task.location_id)
                .await?;

            let reason = skip_reason(&task, &agents, now);
            self.process_result(skipped_result(&task, reason, now))
                .await?;
            recorded += 1;
        }

        Ok(recorded)
    }
}

/// Why the Agents of the task's location left it: the first thing missing on the way from an
/// Agent online to one free to run the check.
pub fn skip_reason(task: &ProbeTask, agents: &[ProbeAgent], now: TimestampMicros) -> SkipReason {
    let online = agents
        .iter()
        .filter(|agent| {
            matches!(agent.status, AgentStatus::Online | AgentStatus::Degraded)
                && agent
                    .last_heartbeat_at
                    .is_some_and(|beat| now.0.saturating_sub(beat.0) <= AGENT_SILENCE_MICROS)
        })
        .collect::<Vec<_>>();

    let capability = ProbeCapability::parse(task.spec.kind().as_str());

    let capable = online
        .iter()
        .filter(|agent| capability.is_some_and(|wanted| agent.capabilities.contains(&wanted)))
        .collect::<Vec<_>>();

    let required = task.spec.required_feature_level();

    if online.is_empty() {
        SkipReason::NoAgentOnline
    } else if capable.is_empty() {
        SkipReason::NoCapableAgent
    } else if capable.iter().all(|agent| agent.feature_level < required) {
        SkipReason::AgentTooOld
    } else {
        SkipReason::AgentsBusy
    }
}

fn skipped_result(task: &ProbeTask, reason: SkipReason, now: TimestampMicros) -> SyntheticResult {
    SyntheticResult {
        id: Id::new(),
        organization_id: task.organization_id.clone(),
        monitor_id: task.monitor_id.clone(),
        monitor_revision_id: task.monitor_revision_id.clone(),
        location_id: task.location_id.clone(),
        agent_id: None,
        task_id: task.id.clone(),
        is_test: task.is_test,
        result_sequence: None,
        scheduled_at: task.scheduled_at,
        started_at: now,
        finished_at: now,
        received_at: now,
        outcome: ProbeOutcome::Skipped,
        attempts: Vec::new(),
        assertions: Vec::new(),
        artifacts: Vec::new(),
        secret_versions: BTreeMap::new(),
        protocol_version: PROBE_PROTOCOL_VERSION,
        metadata: BTreeMap::from([
            ("skip_reason".to_string(), reason.as_str().to_string()),
            (
                "required_feature_level".to_string(),
                task.spec.required_feature_level().to_string(),
            ),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{SkipReason, skip_reason};
    use crate::{
        domain::synthetics::{
            AgentCapacity, AgentStatus, IcmpSpec, MonitorSpec, ProbeAgent, ProbeCapability,
            ProbeTask, ProbeTaskState,
        },
        shared::{ids::Id, time::TimestampMicros},
    };

    const NOW: TimestampMicros = TimestampMicros(1_000_000_000_000);

    fn icmp_task() -> ProbeTask {
        ProbeTask {
            id: Id::from_string("task"),
            organization_id: Id::from_string("org"),
            monitor_id: Id::from_string("monitor"),
            monitor_revision_id: Id::from_string("revision"),
            location_id: Id::from_string("location"),
            spec: MonitorSpec::Icmp(IcmpSpec {
                host: "192.0.2.1".into(),
                count: 3,
                interval_millis: 200,
                max_packet_loss_ratio: 0.0,
                max_mean_rtt_millis: None,
            }),
            is_test: false,
            state: ProbeTaskState::Pending,
            scheduled_at: NOW,
            deadline_at: NOW,
            timeout_millis: 5_000,
            max_attempts: 1,
            inject_trace_context: true,
            traceroute_on_failure: true,
            lease_agent_id: None,
            lease_token_hash: None,
            leased_until: None,
            created_at: NOW,
            updated_at: NOW,
        }
    }

    fn agent(
        capabilities: Vec<ProbeCapability>,
        feature_level: u32,
        beat_ago_secs: i64,
    ) -> ProbeAgent {
        ProbeAgent {
            id: Id::from_string("agent"),
            organization_id: None,
            location_id: Id::from_string("location"),
            name: "agent".into(),
            hostname: "probe".into(),
            status: AgentStatus::Online,
            agent_version: "0.1.0".into(),
            protocol_version: 1,
            feature_level,
            capabilities,
            capacity: AgentCapacity {
                max_concurrent: 1,
                max_browser_concurrent: 0,
                available: 1,
                available_browser: 0,
            },
            labels: BTreeMap::new(),
            public_key_der: Vec::new(),
            certificate_serial: None,
            certificate_expires_at: None,
            last_heartbeat_at: Some(TimestampMicros(NOW.0 - beat_ago_secs * 1_000_000)),
            last_result_sequence: 0,
            revoked_at: None,
            created_at: NOW,
            updated_at: NOW,
        }
    }

    #[test]
    fn names_the_first_thing_missing() {
        let task = icmp_task();

        assert_eq!(skip_reason(&task, &[], NOW), SkipReason::NoAgentOnline);
        assert_eq!(
            skip_reason(&task, &[agent(vec![ProbeCapability::Icmp], 1, 3600)], NOW),
            SkipReason::NoAgentOnline
        );
        assert_eq!(
            skip_reason(&task, &[agent(vec![ProbeCapability::Http], 1, 5)], NOW),
            SkipReason::NoCapableAgent
        );
        assert_eq!(
            skip_reason(&task, &[agent(vec![ProbeCapability::Icmp], 0, 5)], NOW),
            SkipReason::AgentTooOld
        );
        assert_eq!(
            skip_reason(&task, &[agent(vec![ProbeCapability::Icmp], 1, 5)], NOW),
            SkipReason::AgentsBusy
        );
    }
}
