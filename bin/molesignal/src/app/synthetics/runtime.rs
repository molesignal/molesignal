// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::Serialize;

use super::{
    NoDataChange, SyntheticService, aggregate_locations, next_due_at, no_data::missing_threshold,
};
use crate::{
    domain::synthetics::{
        MonitorLifecycle, MonitorRevision, MonitorState, ProbeOutcome, ProbeTask, ProbeTaskState,
        StateObservation, SyntheticMonitor, SyntheticResult, SyntheticResultListQuery,
        SyntheticResultPage, SyntheticStateTransition,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessResultOutcome {
    pub result: SyntheticResult,
    pub historical_only: bool,
    pub location_transition: Option<SyntheticStateTransition>,
    pub aggregate_transition: Option<SyntheticStateTransition>,
}

#[async_trait]
pub trait SyntheticTransitionSink: Send + Sync {
    async fn on_transition(
        &self,
        transition: &SyntheticStateTransition,
        alert_on_degraded: bool,
        alert_on_flaky: bool,
    ) -> Result<()>;

    /// The check was paused, archived or made active again.
    async fn on_lifecycle(&self, _monitor: &SyntheticMonitor) -> Result<()> {
        Ok(())
    }

    /// The check may have stopped getting results, or started again.
    async fn on_no_data(&self, _change: &NoDataChange) -> Result<()> {
        Ok(())
    }
}

/// Hands every transition to several sinks; each sees it whatever the others
/// did, and the first failure is reported.
pub struct SyntheticTransitionSinks {
    sinks: Vec<std::sync::Arc<dyn SyntheticTransitionSink>>,
}

impl SyntheticTransitionSinks {
    pub fn new(sinks: Vec<std::sync::Arc<dyn SyntheticTransitionSink>>) -> Self {
        Self { sinks }
    }
}

#[async_trait]
impl SyntheticTransitionSink for SyntheticTransitionSinks {
    async fn on_transition(
        &self,
        transition: &SyntheticStateTransition,
        alert_on_degraded: bool,
        alert_on_flaky: bool,
    ) -> Result<()> {
        let mut first_error = None;

        for sink in &self.sinks {
            if let Err(error) = sink
                .on_transition(transition, alert_on_degraded, alert_on_flaky)
                .await
            {
                first_error.get_or_insert(error);
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    async fn on_lifecycle(&self, monitor: &SyntheticMonitor) -> Result<()> {
        let mut first_error = None;

        for sink in &self.sinks {
            if let Err(error) = sink.on_lifecycle(monitor).await {
                first_error.get_or_insert(error);
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    async fn on_no_data(&self, change: &NoDataChange) -> Result<()> {
        let mut first_error = None;

        for sink in &self.sinks {
            if let Err(error) = sink.on_no_data(change).await {
                first_error.get_or_insert(error);
            }
        }

        first_error.map_or(Ok(()), Err)
    }
}

impl SyntheticService {
    pub async fn create_test_run(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
    ) -> Result<Vec<ProbeTask>> {
        let revision = self
            .repository
            .get_revision(org_id, monitor_id, revision_id)
            .await?;

        self.queue_immediate_run(org_id, monitor_id, &revision, true)
            .await
    }

    pub async fn run_monitor(&self, org_id: &Id, monitor_id: &Id) -> Result<Vec<ProbeTask>> {
        let active = self
            .repository
            .get_active_revision(org_id, monitor_id)
            .await?
            .ok_or_else(|| Error::conflict("Monitor has no published Revision"))?;

        if active.monitor.lifecycle != MonitorLifecycle::Active {
            return Err(Error::conflict(
                "Only active Monitors can be run immediately",
            ));
        }

        self.queue_immediate_run(org_id, monitor_id, &active.revision, false)
            .await
    }

    async fn queue_immediate_run(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision: &MonitorRevision,
        is_test: bool,
    ) -> Result<Vec<ProbeTask>> {
        if revision.location_ids.is_empty() {
            return Err(Error::invalid(
                "Heartbeat runs are submitted through the Heartbeat endpoint",
            ));
        }

        let now = TimestampMicros::now();

        let deadline_at = TimestampMicros(
            now.0
                .saturating_add(i64::from(revision.freshness_seconds) * 1_000_000),
        );

        self.repository
            .create_tasks(
                revision
                    .location_ids
                    .iter()
                    .map(|location_id| ProbeTask {
                        id: Id::new(),
                        organization_id: org_id.clone(),
                        monitor_id: monitor_id.clone(),
                        monitor_revision_id: revision.id.clone(),
                        location_id: location_id.clone(),
                        spec: revision.spec.clone(),
                        is_test,
                        state: ProbeTaskState::Pending,
                        scheduled_at: now,
                        deadline_at,
                        timeout_millis: revision.timeout_millis,
                        max_attempts: revision.max_retries.saturating_add(1),
                        inject_trace_context: revision.inject_trace_context,
                        traceroute_on_failure: revision.traceroute_on_failure,
                        lease_agent_id: None,
                        lease_token_hash: None,
                        leased_until: None,
                        created_at: now,
                        updated_at: now,
                    })
                    .collect(),
            )
            .await
    }

    pub async fn expire_task_leases(&self, now: TimestampMicros, limit: u32) -> Result<u64> {
        self.repository.expire_leases(now, limit.min(1000)).await
    }

    pub async fn materialize_due_tasks(
        &self,
        now: TimestampMicros,
        limit: u32,
    ) -> Result<Vec<ProbeTask>> {
        let due = self.repository.list_due_monitors(now, limit).await?;
        let mut tasks = Vec::new();

        for active in due {
            let scheduled_at = active.monitor.next_due_at.unwrap_or(now);

            let deadline_at = TimestampMicros(
                scheduled_at
                    .0
                    .saturating_add(i64::from(active.revision.freshness_seconds) * 1_000_000),
            );

            let next = next_due_at(&active.monitor.id, &active.revision.schedule, now)?;

            let created = self
                .repository
                .create_tasks(
                    active
                        .revision
                        .location_ids
                        .iter()
                        .map(|location_id| ProbeTask {
                            id: Id::new(),
                            organization_id: active.monitor.organization_id.clone(),
                            monitor_id: active.monitor.id.clone(),
                            monitor_revision_id: active.revision.id.clone(),
                            location_id: location_id.clone(),
                            spec: active.revision.spec.clone(),
                            is_test: false,
                            state: ProbeTaskState::Pending,
                            scheduled_at,
                            deadline_at,
                            timeout_millis: active.revision.timeout_millis,
                            max_attempts: active.revision.max_retries.saturating_add(1),
                            inject_trace_context: active.revision.inject_trace_context,
                            traceroute_on_failure: active.revision.traceroute_on_failure,
                            lease_agent_id: None,
                            lease_token_hash: None,
                            leased_until: None,
                            created_at: now,
                            updated_at: now,
                        })
                        .collect(),
                )
                .await?;

            tasks.extend(created);

            self.repository
                .update_monitor_runtime(
                    &active.monitor.organization_id,
                    &active.monitor.id,
                    active.monitor.lifecycle,
                    active.monitor.state,
                    next,
                    now,
                )
                .await?;
        }

        Ok(tasks)
    }

    pub async fn process_result(&self, result: SyntheticResult) -> Result<ProcessResultOutcome> {
        let (result, inserted) = self.repository.record_result(result).await?;

        if !inserted {
            return Ok(historical(result));
        }

        if result.is_test {
            if matches!(
                result.outcome,
                ProbeOutcome::Healthy | ProbeOutcome::Flaky | ProbeOutcome::Degraded
            ) {
                self.repository
                    .mark_revision_tested(
                        &result.organization_id,
                        &result.monitor_id,
                        &result.monitor_revision_id,
                        &result.id,
                        result.received_at,
                    )
                    .await?;
            }

            return Ok(historical(result));
        }

        // A scheduled run is a row of the result stream, whichever revision it ran.
        if let Some(stream) = &self.result_stream {
            stream.push(result.clone());
        }

        let Some(active) = self
            .repository
            .get_active_revision(&result.organization_id, &result.monitor_id)
            .await?
        else {
            return Ok(historical(result));
        };

        // A paused or archived check stands as it was left: a run still due when it stopped says
        // nothing more, and opens no incident its stopping resolved.
        if active.monitor.lifecycle != MonitorLifecycle::Active {
            return Ok(historical(result));
        }

        let stale_after = result
            .scheduled_at
            .0
            .saturating_add(i64::from(active.revision.freshness_seconds) * 1_000_000);

        // A skipped run is recorded once its deadline has passed, never in time.
        let skipped = result.outcome == ProbeOutcome::Skipped;

        if active.revision.id != result.monitor_revision_id
            || (!skipped && result.received_at.0 > stale_after)
        {
            return Ok(historical(result));
        }

        // A run that did not happen leaves the location's state as its last run made it.
        if skipped {
            self.count_missed_run(&active, &result).await?;
            return Ok(historical(result));
        }

        let location_update = self
            .repository
            .apply_location_observation(StateObservation {
                organization_id: result.organization_id.clone(),
                monitor_id: result.monitor_id.clone(),
                monitor_revision_id: result.monitor_revision_id.clone(),
                location_id: result.location_id.clone(),
                observed_state: outcome_state(result.outcome),
                result_id: result.id.clone(),
                observed_at: result.finished_at,
                failure_threshold: active.revision.consecutive_failures,
                recovery_threshold: active.revision.consecutive_recoveries,
            })
            .await?;

        let states = self
            .repository
            .list_location_states(
                &result.organization_id,
                &result.monitor_id,
                &result.monitor_revision_id,
            )
            .await?;

        let aggregate = aggregate_locations(
            states.iter().map(|state| state.current_state),
            active.revision.location_ids.len(),
            &active.revision.location_policy,
        );

        let aggregate_transition = if aggregate != active.monitor.state {
            self.repository
                .update_monitor_runtime(
                    &result.organization_id,
                    &result.monitor_id,
                    active.monitor.lifecycle,
                    aggregate,
                    active.monitor.next_due_at,
                    result.received_at,
                )
                .await?;

            let transition = self
                .repository
                .record_state_transition(SyntheticStateTransition {
                    id: Id::new(),
                    organization_id: result.organization_id.clone(),
                    monitor_id: result.monitor_id.clone(),
                    monitor_revision_id: result.monitor_revision_id.clone(),
                    location_id: None,
                    previous_state: active.monitor.state,
                    current_state: aggregate,
                    correlation_key: format!("synthetic:{}", result.monitor_id),
                    result_id: Some(result.id.clone()),
                    started_at: result.finished_at,
                    ended_at: None,
                    created_at: result.received_at,
                })
                .await?;

            self.emit_transition(
                &transition,
                active.revision.alert_on_degraded,
                active.revision.alert_on_flaky,
            )
            .await;

            Some(transition)
        } else {
            None
        };

        // The location ran again after missing as many runs as fail it, or ran for the first time
        // under this revision: the check may have data again.
        if location_update
            .previous_missing
            .is_none_or(|missed| missed >= missing_threshold(&active.revision))
        {
            self.settle_no_data(&active, &states, None).await;
        }

        Ok(ProcessResultOutcome {
            result,
            historical_only: false,
            location_transition: location_update.transition,
            aggregate_transition,
        })
    }

    pub async fn list_results(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        before: Option<TimestampMicros>,
        limit: u32,
    ) -> Result<Vec<SyntheticResult>> {
        self.repository
            .list_results(org_id, monitor_id, before, limit)
            .await
    }

    pub async fn list_results_page(
        &self,
        org_id: &Id,
        query: &SyntheticResultListQuery,
    ) -> Result<SyntheticResultPage> {
        self.repository.list_results_page(org_id, query).await
    }

    pub async fn get_result(&self, org_id: &Id, result_id: &Id) -> Result<SyntheticResult> {
        self.repository.get_result(org_id, result_id).await
    }

    async fn emit_transition(
        &self,
        transition: &SyntheticStateTransition,
        alert_on_degraded: bool,
        alert_on_flaky: bool,
    ) {
        if let Some(sink) = &self.transition_sink
            && let Err(error) = sink
                .on_transition(transition, alert_on_degraded, alert_on_flaky)
                .await
        {
            tracing::error!(
                monitor_id = %transition.monitor_id,
                transition_id = %transition.id,
                error = %error,
                "synthetic transition sink failed; transition remains replayable"
            );
        }
    }
}

fn historical(result: SyntheticResult) -> ProcessResultOutcome {
    ProcessResultOutcome {
        result,
        historical_only: true,
        location_transition: None,
        aggregate_transition: None,
    }
}

fn outcome_state(outcome: ProbeOutcome) -> MonitorState {
    match outcome {
        ProbeOutcome::Healthy => MonitorState::Healthy,
        ProbeOutcome::Flaky => MonitorState::Flaky,
        ProbeOutcome::Degraded => MonitorState::Degraded,
        ProbeOutcome::Failing => MonitorState::Failing,
        ProbeOutcome::Unknown | ProbeOutcome::Skipped => MonitorState::Unknown,
    }
}
