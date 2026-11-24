// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puts the looks together service by service and grades each way a service
//! ties to other data. Pages and the agent read the same grades.

use std::collections::{HashMap, HashSet};

use domain::apm::{
    DEFAULT_DEPLOYMENT_ENVIRONMENT, ServiceChangeCount, ServiceIdentity, ServiceObservation,
};
use kernel::time::{TimeRange, TimestampMicros};
use serde::Serialize;

use super::{
    ChangeCorrelation, CorrelationGrades, CorrelationReport, LogCorrelation, LookState, NamedIn,
    ResourceCompleteness, ServiceCorrelation, UnmatchedName,
    looks::{LogLook, MetricLook},
};
use crate::app::apm::ServiceInventory;

/// The share of a service's lines that must carry a trace id, and the share of sampled trace
/// ids that must be found among the traces, for its logs to tie well to its traces.
pub const TRACED_SHARE: f64 = 0.9;
pub const MATCHED_SHARE: f64 = 0.8;
/// The most unmatched names a report lists, those with the most lines first.
const MAX_UNMATCHED: usize = 20;
/// Resource attributes that say where an instance runs.
const PLACEMENT_ATTRIBUTES: [&str; 4] =
    ["host.name", "k8s.pod.name", "k8s.node.name", "container.id"];

/// How well a service ties to other data one way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grade {
    Good,
    Partial,
    Missing,
    Unknown,
}

pub(super) struct Inputs {
    pub window: TimeRange,
    pub changes_since: TimestampMicros,
    pub inventory: ServiceInventory,
    /// `None` when the changes could not be read.
    pub changes: Option<Vec<ServiceChangeCount>>,
    pub logs: LogLook,
    pub metrics: MetricLook,
}

pub(super) fn assemble(inputs: Inputs) -> CorrelationReport {
    let Inputs {
        window,
        changes_since,
        inventory,
        changes,
        logs,
        metrics,
    } = inputs;

    let changes = changes.map(|changes| {
        changes
            .into_iter()
            .map(|change| (change.service.clone(), change))
            .collect::<HashMap<_, _>>()
    });

    let mut services = inventory
        .services
        .iter()
        .map(|observation| {
            let identity = &observation.service;
            let logs = log_correlation(&logs, &identity.name);
            let metrics = metric_state(&metrics, &identity.name);
            let resource = resource(&inventory, observation);

            let changes = changes.as_ref().map(|changes| {
                changes
                    .get(&identity.name)
                    .map_or_else(ChangeCorrelation::default, |change| ChangeCorrelation {
                        count: change.count,
                        last_at: Some(change.last_at),
                    })
            });

            ServiceCorrelation {
                service: identity.clone(),
                grades: CorrelationGrades {
                    trace_ids: trace_grade(&logs),
                    resource: resource_grade(resource),
                    metrics: state_grade(metrics),
                    changes: changes.map_or(Grade::Unknown, |changes| {
                        if changes.count > 0 {
                            Grade::Good
                        } else {
                            Grade::Missing
                        }
                    }),
                },
                logs,
                metrics,
                resource,
                changes: changes.unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();

    services.sort_by_key(|entry| entry.service.stable_key());

    let known = inventory
        .services
        .iter()
        .map(|observation| observation.service.name.as_str())
        .collect::<HashSet<_>>();

    CorrelationReport {
        from: window.start,
        to: window.end,
        changes_since,
        services,
        unmatched: unmatched(&known, &logs, &metrics),
    }
}

fn log_correlation(logs: &LogLook, name: &str) -> LogCorrelation {
    match logs.by_name.get(name).filter(|lines| lines.lines > 0) {
        Some(lines) => LogCorrelation {
            state: LookState::Reported,
            streams: lines.streams.iter().cloned().collect(),
            lines: lines.lines,
            traced: lines.traced,
            sampled: lines.sampled,
            matched: lines.matched,
        },
        None => LogCorrelation {
            // Nothing found while a stream could not be read: it may be in that stream.
            state: if logs.failed {
                LookState::Unknown
            } else {
                LookState::Missing
            },
            streams: Vec::new(),
            lines: 0,
            traced: 0,
            sampled: 0,
            matched: 0,
        },
    }
}

fn metric_state(metrics: &MetricLook, name: &str) -> LookState {
    if metrics.by_name.contains_key(name) {
        LookState::Reported
    } else if metrics.failed {
        LookState::Unknown
    } else {
        LookState::Missing
    }
}

fn resource(
    inventory: &ServiceInventory,
    observation: &ServiceObservation,
) -> ResourceCompleteness {
    let identity = &observation.service;

    let instances = inventory
        .instances
        .iter()
        .filter(|instance| same_service(&instance.service, identity))
        .collect::<Vec<_>>();

    ResourceCompleteness {
        environment: identity.environment != DEFAULT_DEPLOYMENT_ENVIRONMENT,
        version: inventory
            .versions
            .iter()
            .any(|version| same_service(&version.service, identity)),
        instance: observation.recent_instance_count > 0 || !instances.is_empty(),
        placement: instances.iter().any(|instance| {
            PLACEMENT_ATTRIBUTES.iter().any(|attribute| {
                instance
                    .attributes
                    .get(*attribute)
                    .is_some_and(|value| !value.trim().is_empty())
            })
        }),
    }
}

fn same_service(left: &ServiceIdentity, right: &ServiceIdentity) -> bool {
    left.stable_key() == right.stable_key()
}

/// Good when nearly every line carries a trace id and those sampled were found among the
/// traces; partial when some do; missing when none do or the service writes no logs.
pub(super) fn trace_grade(logs: &LogCorrelation) -> Grade {
    match logs.state {
        LookState::Unknown => Grade::Unknown,
        LookState::Missing => Grade::Missing,
        LookState::Reported if logs.traced == 0 => Grade::Missing,
        LookState::Reported => {
            let traced = logs.traced as f64 / logs.lines.max(1) as f64;

            // Nothing sampled leaves the share of lines alone to judge by.
            let matched = if logs.sampled > 0 {
                logs.matched as f64 / logs.sampled as f64
            } else {
                1.0
            };

            if traced >= TRACED_SHARE && matched >= MATCHED_SHARE {
                Grade::Good
            } else {
                Grade::Partial
            }
        }
    }
}

pub(super) fn resource_grade(resource: ResourceCompleteness) -> Grade {
    let said = [
        resource.environment,
        resource.version,
        resource.instance,
        resource.placement,
    ]
    .into_iter()
    .filter(|said| *said)
    .count();

    match said {
        4 => Grade::Good,
        0 => Grade::Missing,
        _ => Grade::Partial,
    }
}

fn state_grade(state: LookState) -> Grade {
    match state {
        LookState::Reported => Grade::Good,
        LookState::Missing => Grade::Missing,
        LookState::Unknown => Grade::Unknown,
    }
}

fn unmatched(known: &HashSet<&str>, logs: &LogLook, metrics: &MetricLook) -> Vec<UnmatchedName> {
    let mut names = logs
        .by_name
        .iter()
        .filter(|(name, lines)| lines.lines > 0 && !known.contains(name.as_str()))
        .map(|(name, lines)| UnmatchedName {
            name: name.clone(),
            signal: NamedIn::Logs,
            streams: lines.streams.iter().cloned().collect(),
            lines: Some(lines.lines),
        })
        .chain(
            metrics
                .by_name
                .iter()
                .filter(|(name, _)| !known.contains(name.as_str()))
                .map(|(name, streams)| UnmatchedName {
                    name: name.clone(),
                    signal: NamedIn::Metrics,
                    streams: streams.iter().cloned().collect(),
                    lines: None,
                }),
        )
        .collect::<Vec<_>>();

    names.sort_by(|left, right| {
        right
            .lines
            .unwrap_or(0)
            .cmp(&left.lines.unwrap_or(0))
            .then_with(|| left.signal.cmp(&right.signal))
            .then_with(|| left.name.cmp(&right.name))
    });
    names.truncate(MAX_UNMATCHED);
    names
}
