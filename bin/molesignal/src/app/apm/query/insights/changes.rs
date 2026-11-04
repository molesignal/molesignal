// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The changes near when a move began: what pipelines and tools reported
//! for the service, or for a service it calls, from [`BEFORE`] before to
//! [`AFTER`] after; and the versions its spans first carried then, unless a
//! pipeline reported the same deployment.

use std::collections::HashSet;

use super::model::{ChangeSource, Insight, NearbyChange};
use crate::{
    domain::apm::{ChangeKind, DeploymentEvent, DeploymentStatus, VersionObservation},
    shared::time::TimestampMicros,
};

/// How long before a move a change may have caused it, and how long after
/// its first sign a change is still near: buckets close a little late.
pub const BEFORE: i64 = 30 * 60_000_000;
pub const AFTER: i64 = 10 * 60_000_000;
/// The most changes an insight names.
const MOST_CHANGES: usize = 5;

/// Names on `insight` the changes near `around`: of its service, or of the
/// services in `callees`, which it calls.
pub(super) fn attach(
    insight: &mut Insight,
    around: TimestampMicros,
    reported: &[DeploymentEvent],
    observed: &[VersionObservation],
    callees: &HashSet<String>,
) {
    let near = |at: TimestampMicros| at.0 >= around.0 - BEFORE && at.0 <= around.0 + AFTER;
    let service = &insight.service;
    let mut changes = reported
        .iter()
        .filter(|event| near(event.deployed_at))
        .filter(|event| event.environment.is_empty() || event.environment == service.environment)
        .filter_map(|event| {
            let own = event.service == service.name
                && (event.namespace.is_empty() || event.namespace == service.namespace);

            (own || callees.contains(&event.service)).then(|| NearbyChange {
                kind: event.kind,
                source: ChangeSource::Reported,
                service: event.service.clone(),
                version: event.version.clone(),
                status: event.status,
                at: event.deployed_at,
                downstream: !own,
                commit: event.commit.clone(),
                description: event.description.clone(),
            })
        })
        .collect::<Vec<_>>();

    let reported_versions = changes
        .iter()
        .filter(|change| change.kind == ChangeKind::Deployment && !change.downstream)
        .map(|change| change.version.clone())
        .collect::<HashSet<_>>();

    changes.extend(
        observed
            .iter()
            .filter(|version| version.service == *service && near(version.first_seen_at))
            .filter(|version| !reported_versions.contains(&version.version))
            .map(|version| NearbyChange {
                kind: ChangeKind::Deployment,
                source: ChangeSource::Observed,
                service: service.name.clone(),
                version: version.version.clone(),
                status: DeploymentStatus::Succeeded,
                at: version.first_seen_at,
                downstream: false,
                commit: None,
                description: None,
            }),
    );

    changes.sort_by_key(|change| ((change.at.0 - around.0).abs(), change.service.clone()));
    changes.truncate(MOST_CHANGES);
    insight.changes = changes;
}
