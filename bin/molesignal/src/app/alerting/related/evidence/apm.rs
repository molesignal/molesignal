// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What APM says around an incident: the insights about its services, the
//! error groups first seen then, and the slowest of the traces APM kept as
//! examples of their requests.

use domain::{
    alerting::rca_evidence::{EvidenceKind, EvidenceLink, EvidenceSource, RcaEvidence},
    apm::ServiceIdentity,
};
use kernel::Result;
use serde::Serialize;

use super::Scope;
use crate::app::apm::{ApmQueryRequest, ApmQueryService, ErrorIssueFilter, Insight};

/// The most insights, new error groups and sample traces told.
const TOLD_INSIGHTS: usize = 8;
const TOLD_ERROR_GROUPS: usize = 5;
const TRACED_SERVICES: usize = 4;
const TRACES_PER_SERVICE: usize = 2;
/// Error groups read to find those first seen in the window, the most frequent first.
const ERROR_GROUPS_READ: usize = 100;
/// The most characters of an error's message told.
const MESSAGE_CHARS: usize = 160;

fn request(scope: &Scope<'_>, service: Option<&str>) -> ApmQueryRequest {
    ApmQueryRequest {
        from: scope.window.start.0,
        to: scope.window.end.0,
        service: service.map(str::to_owned),
        limit: Some(ERROR_GROUPS_READ),
        ..ApmQueryRequest::default()
    }
}

fn tells(scope: &Scope<'_>, service: &ServiceIdentity) -> bool {
    scope.services.contains(&service.name)
}

fn link_to(service: &ServiceIdentity) -> EvidenceLink {
    EvidenceLink {
        namespace: Some(service.namespace.clone()),
        environment: Some(service.environment.clone()),
        ..EvidenceLink::default()
    }
}

/// A value's name as it is written out, such as `slow_dependency`.
fn written<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub(super) async fn insights(apm: &ApmQueryService, scope: &Scope<'_>) -> Result<Vec<RcaEvidence>> {
    let context = apm.context(
        scope.incident.org_id.clone(),
        request(scope, None),
        &["request_count"],
        "request_count",
    )?;

    // Insights come the most pressing first.
    Ok(apm
        .insights(&context)
        .await?
        .insights
        .iter()
        .filter(|insight| tells(scope, &insight.service))
        .take(TOLD_INSIGHTS)
        .map(insight_evidence)
        .collect())
}

pub(super) fn insight_evidence(insight: &Insight) -> RcaEvidence {
    let mut piece = RcaEvidence::new(EvidenceKind::Insight, EvidenceSource::Apm);
    let kind = written(insight.kind);

    piece.service = Some(insight.service.name.clone());

    piece.detail = Some(match insight.metric {
        Some(metric) => format!("{kind}.{metric}"),
        None => kind,
    });

    piece.subject = insight
        .transaction
        .as_ref()
        .map(|transaction| transaction.name.clone())
        .or_else(|| {
            insight
                .dependency
                .as_ref()
                .map(|dependency| dependency.target.clone())
        })
        .or_else(|| {
            insight
                .version
                .as_ref()
                .map(|version| match &insight.previous_version {
                    Some(previous) => format!("{version} after {previous}"),
                    None => version.clone(),
                })
        });

    piece.at = insight.since;
    piece.figures.insert("value".into(), insight.value);

    let optional = [
        ("previous", insight.previous),
        ("lastWeek", insight.last_week),
        ("calls", insight.calls.map(|calls| calls as f64)),
        ("p95Time", insight.p95_time.map(|time| time as f64)),
    ];

    for (name, value) in optional {
        if let Some(value) = value {
            piece.figures.insert(name.into(), value);
        }
    }

    piece.link = Some(link_to(&insight.service));
    piece
}

pub(super) async fn new_error_groups(
    apm: &ApmQueryService,
    scope: &Scope<'_>,
) -> Result<Vec<RcaEvidence>> {
    let context = apm.context(
        scope.incident.org_id.clone(),
        request(scope, None),
        &["occurrence_count", "error_rate", "last_seen"],
        "occurrence_count",
    )?;

    let groups = apm.errors(&context, ErrorIssueFilter::All).await?;

    Ok(groups
        .page
        .items
        .iter()
        .filter(|group| group.first_seen_at >= scope.window.start && tells(scope, &group.service))
        .take(TOLD_ERROR_GROUPS)
        .map(|group| {
            let mut piece = RcaEvidence::new(EvidenceKind::NewErrorGroup, EvidenceSource::Apm);
            let mut subject = group.error.error_type.clone();

            if let Some(transaction) = &group.error.transaction_name {
                subject.push_str(&format!(" in {transaction}"));
            }

            if let Some(message) = group
                .representative_message
                .as_deref()
                .map(str::trim)
                .filter(|message| !message.is_empty())
            {
                subject.push_str(&format!(": {}", super::brief::cut(message, MESSAGE_CHARS)));
            }

            piece.service = Some(group.service.name.clone());
            piece.subject = Some(subject);
            piece.at = Some(group.first_seen_at);

            piece
                .figures
                .insert("occurrences".into(), group.occurrence_count as f64);

            piece.link = Some(EvidenceLink {
                fingerprint: Some(group.error.fingerprint.clone()),
                ..link_to(&group.service)
            });

            piece
        })
        .collect())
}

pub(super) async fn sample_traces(
    apm: &ApmQueryService,
    scope: &Scope<'_>,
) -> Result<Vec<RcaEvidence>> {
    let mut evidence = Vec::new();

    for service in scope.services.iter().take(TRACED_SERVICES) {
        let context = apm.context(
            scope.incident.org_id.clone(),
            request(scope, Some(service)),
            &["request_count"],
            "request_count",
        )?;

        let mut exemplars = apm.recent_exemplars(&context, 8).await?;
        exemplars.retain(|exemplar| !exemplar.trace_id.is_empty());
        exemplars.sort_by_key(|exemplar| std::cmp::Reverse(exemplar.duration));

        evidence.extend(
            exemplars
                .into_iter()
                .take(TRACES_PER_SERVICE)
                .map(|exemplar| {
                    let mut piece =
                        RcaEvidence::new(EvidenceKind::SampleTrace, EvidenceSource::Traces);

                    piece.service = Some(service.clone());
                    piece.at = Some(exemplar.event_time);

                    piece
                        .figures
                        .insert("duration".into(), exemplar.duration as f64);

                    piece.link = Some(EvidenceLink {
                        trace_id: Some(exemplar.trace_id),
                        ..EvidenceLink::default()
                    });

                    piece
                }),
        );
    }

    Ok(evidence)
}
