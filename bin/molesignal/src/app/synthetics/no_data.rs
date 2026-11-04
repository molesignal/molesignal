// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Checks that stop getting results.
//!
//! A run no Agent took in time is recorded as skipped (see [`super::unclaimed`]). Its location
//! keeps the state its last run gave it and counts the run as missed; the next run it makes
//! resets the count. A check has no data when as many locations as its location policy needs
//! have missed as many runs in a row as fail a location. The sinks hear of it when a location
//! reaches that count or comes back from it, and open or resolve an incident.

use std::collections::HashMap;

use serde::Serialize;

use super::{SyntheticService, aggregation::locations_needed};
use crate::{
    domain::synthetics::{
        ActiveMonitorRevision, MissedRun, MonitorLocationState, MonitorRevision, SyntheticMonitor,
        SyntheticResult,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// Whether a check gets results, told to the sinks when one of its locations reaches or leaves
/// the run of missed runs that fails a location.
#[derive(Debug, Clone)]
pub struct NoDataChange {
    pub monitor: SyntheticMonitor,
    pub revision: MonitorRevision,
    /// Whether the check has no data and alerts on it ([`MonitorRevision::alert_on_no_data`]).
    pub no_data: bool,
    /// The names of the locations missing runs.
    pub locations: Vec<String>,
    /// The most runs in a row one of them missed.
    pub missed_runs: u32,
    /// Why the run that told of it was skipped, when a skipped run did.
    pub skip_reason: Option<String>,
    /// When the locations missing runs last ran: the stretch without results began after it.
    pub since: TimestampMicros,
}

/// Where a check's published revision is missing runs now, for its page.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingRuns {
    /// Whether the locations missing runs are as many as the location policy needs.
    pub no_data: bool,
    /// The locations that missed as many runs in a row as fail a location.
    pub locations: Vec<MissingLocation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingLocation {
    pub location_id: Id,
    /// Runs missed in a row.
    pub missed_runs: u32,
}

impl SyntheticService {
    /// Where the check's published revision is missing runs.
    pub async fn missing_runs(&self, org_id: &Id, monitor_id: &Id) -> Result<MissingRuns> {
        let Some(active) = self
            .repository
            .get_active_revision(org_id, monitor_id)
            .await?
        else {
            return Ok(MissingRuns::default());
        };

        let states = self
            .repository
            .list_location_states(org_id, monitor_id, &active.revision.id)
            .await?;

        let (missing, no_data) = missing_locations(&states, &active.revision);

        Ok(MissingRuns {
            no_data,
            locations: missing
                .into_iter()
                .map(|state| MissingLocation {
                    location_id: state.location_id.clone(),
                    missed_runs: state.consecutive_missing,
                })
                .collect(),
        })
    }

    /// A location missed a run: counts it, and from the run that fails a location on, tells the
    /// sinks whether the check has no data.
    pub(super) async fn count_missed_run(
        &self,
        active: &ActiveMonitorRevision,
        result: &SyntheticResult,
    ) -> Result<()> {
        let location = self
            .repository
            .record_missed_run(MissedRun {
                organization_id: result.organization_id.clone(),
                monitor_id: result.monitor_id.clone(),
                monitor_revision_id: result.monitor_revision_id.clone(),
                location_id: result.location_id.clone(),
                missed_at: result.received_at,
            })
            .await?;

        if location.consecutive_missing < missing_threshold(&active.revision) {
            return Ok(());
        }

        let states = self
            .repository
            .list_location_states(
                &result.organization_id,
                &result.monitor_id,
                &result.monitor_revision_id,
            )
            .await?;

        self.settle_no_data(active, &states, result.metadata.get("skip_reason").cloned())
            .await;

        Ok(())
    }

    /// Tells the sinks whether the check has no data, as its locations now stand.
    pub(super) async fn settle_no_data(
        &self,
        active: &ActiveMonitorRevision,
        states: &[MonitorLocationState],
        skip_reason: Option<String>,
    ) {
        let Some(sink) = &self.transition_sink else {
            return;
        };

        let (missing, no_data) = missing_locations(states, &active.revision);

        let change = NoDataChange {
            monitor: active.monitor.clone(),
            revision: active.revision.clone(),
            no_data: no_data && active.revision.alert_on_no_data,
            locations: self
                .location_names(&active.monitor.organization_id, &missing)
                .await,
            missed_runs: missing
                .iter()
                .map(|state| state.consecutive_missing)
                .max()
                .unwrap_or(0),
            skip_reason,
            since: missing
                .iter()
                .filter_map(|state| state.last_observed_at)
                .max()
                .unwrap_or(active.revision.created_at),
        };

        if let Err(error) = sink.on_no_data(&change).await {
            tracing::error!(
                monitor_id = %active.monitor.id,
                no_data = change.no_data,
                %error,
                "synthetic no-data sink failed"
            );
        }
    }

    /// The names of the locations, by id where they cannot be read.
    async fn location_names(&self, org_id: &Id, states: &[&MonitorLocationState]) -> Vec<String> {
        if states.is_empty() {
            return Vec::new();
        }

        let names = match self.repository.list_locations(org_id).await {
            Ok(locations) => locations
                .into_iter()
                .map(|location| (location.id, location.name))
                .collect::<HashMap<_, _>>(),
            Err(error) => {
                tracing::warn!(%error, "synthetic locations could not be read");
                HashMap::new()
            }
        };

        states
            .iter()
            .map(|state| {
                names
                    .get(&state.location_id)
                    .cloned()
                    .unwrap_or_else(|| state.location_id.to_string())
            })
            .collect()
    }
}

/// Runs in a row a location misses before it counts as missing: as many as fail it.
pub(super) fn missing_threshold(revision: &MonitorRevision) -> u32 {
    revision.consecutive_failures.max(1)
}

/// The revision's locations missing runs, and whether they are as many as its location policy
/// needs.
fn missing_locations<'a>(
    states: &'a [MonitorLocationState],
    revision: &MonitorRevision,
) -> (Vec<&'a MonitorLocationState>, bool) {
    let threshold = missing_threshold(revision);

    let missing = states
        .iter()
        .filter(|state| revision.location_ids.contains(&state.location_id))
        .filter(|state| state.consecutive_missing >= threshold)
        .collect::<Vec<_>>();

    let expected = revision.location_ids.len();
    let no_data =
        expected > 0 && missing.len() >= locations_needed(&revision.location_policy, expected);

    (missing, no_data)
}

#[cfg(test)]
mod tests {
    use super::missing_locations;
    use crate::{
        domain::synthetics::{
            IcmpSpec, MonitorLocationState, MonitorRevision, MonitorSchedule, MonitorSpec,
            MonitorState, MultiLocationPolicy,
        },
        shared::{ids::Id, time::TimestampMicros},
    };

    fn revision(policy: MultiLocationPolicy, locations: &[&str]) -> MonitorRevision {
        MonitorRevision {
            id: Id::from_string("revision"),
            organization_id: Id::from_string("org"),
            monitor_id: Id::from_string("check"),
            number: 1,
            spec: MonitorSpec::Icmp(IcmpSpec {
                host: "192.0.2.1".into(),
                count: 3,
                interval_millis: 200,
                max_packet_loss_ratio: 0.0,
                max_mean_rtt_millis: None,
            }),
            schedule: MonitorSchedule::Interval { every_seconds: 60 },
            timeout_millis: 5_000,
            max_retries: 0,
            consecutive_failures: 3,
            consecutive_recoveries: 2,
            freshness_seconds: 300,
            location_policy: policy,
            location_ids: locations.iter().copied().map(Id::from_string).collect(),
            escalation_policy_id: None,
            alert_on_degraded: false,
            alert_on_flaky: false,
            alert_on_no_data: true,
            inject_trace_context: true,
            traceroute_on_failure: true,
            last_test_result_id: None,
            last_test_passed_at: None,
            created_by: Id::from_string("user"),
            created_at: TimestampMicros(0),
            content_hash: String::new(),
        }
    }

    fn missed(location: &str, runs: u32) -> MonitorLocationState {
        MonitorLocationState {
            organization_id: Id::from_string("org"),
            monitor_id: Id::from_string("check"),
            monitor_revision_id: Id::from_string("revision"),
            location_id: Id::from_string(location),
            current_state: MonitorState::Healthy,
            candidate_state: None,
            candidate_count: 0,
            consecutive_missing: runs,
            last_result_id: None,
            last_observed_at: None,
            updated_at: TimestampMicros(0),
        }
    }

    fn names(missing: &[&MonitorLocationState]) -> Vec<String> {
        missing
            .iter()
            .map(|state| state.location_id.to_string())
            .collect()
    }

    #[test]
    fn a_location_is_missing_once_it_misses_as_many_runs_as_fail_it() {
        let majority = revision(MultiLocationPolicy::Majority, &["fra", "sin", "iad"]);

        let one = [missed("fra", 2), missed("sin", 3)];
        let (missing, no_data) = missing_locations(&one, &majority);
        assert_eq!(names(&missing), ["sin"]);
        assert!(!no_data);

        // Two of three is a majority.
        let two = [missed("fra", 4), missed("sin", 3)];
        let (missing, no_data) = missing_locations(&two, &majority);
        assert_eq!(names(&missing), ["fra", "sin"]);
        assert!(no_data);
    }

    #[test]
    fn the_location_policy_says_how_many_have_to_miss() {
        let states = [missed("fra", 5), missed("sin", 0)];

        let any = revision(MultiLocationPolicy::Any, &["fra", "sin"]);
        assert!(missing_locations(&states, &any).1);

        let all = revision(MultiLocationPolicy::All, &["fra", "sin"]);
        assert!(!missing_locations(&states, &all).1);

        let quorum = revision(MultiLocationPolicy::Quorum { required: 2 }, &["fra", "sin"]);
        assert!(!missing_locations(&states, &quorum).1);
    }

    #[test]
    fn only_the_revisions_locations_count() {
        // A location the revision no longer runs from says nothing, and nor does a check
        // without locations.
        let one = revision(MultiLocationPolicy::Any, &["sin"]);
        assert!(!missing_locations(&[missed("fra", 9)], &one).1);

        let heartbeat = revision(MultiLocationPolicy::Any, &[]);
        assert!(!missing_locations(&[missed("fra", 9)], &heartbeat).1);
    }
}
