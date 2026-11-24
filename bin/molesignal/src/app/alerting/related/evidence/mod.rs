// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The evidence a root-cause analysis of an incident is given, read around
//! the time it began for its services and the services they call: what APM
//! found worth a look, error groups first seen then, sample traces,
//! error-level log lines against the same stretch a week before, patterns of
//! error-level logs the stretch just before did not have, the fields the
//! failed requests differ from the others by, and the values of business keys
//! (tenants, plans) whose requests failed; then the changes and similar
//! incidents its brief tells. Each piece is labelled so the analysis can cite
//! it.
//!
//! Unlike the brief, this reads telemetry, so it goes only to the analysis:
//! the figures it keeps are shown to readers allowed to read their source.

mod apm;
mod business;
mod logs;
mod patterns;
mod render;
mod skew;

#[cfg(test)]
mod tests;

use std::{collections::BTreeSet, sync::Arc, time::Duration};

use domain::{
    alerting::{
        incident::Incident,
        rca_evidence::{EvidenceKind, EvidenceLink, EvidenceSource, RcaEvidence, label_evidence},
    },
    business_context::BusinessContextRepository,
    stream::StreamRepository,
};
use kernel::time::{TimeRange, TimestampMicros};
pub use render::render_evidence;

use super::{AFTER, BEFORE, IncidentBrief, brief, incident_services};
use crate::app::{apm::ApmQueryService, query::QueryService};

/// The most services evidence is read for: the incident's own first, then those they call.
const MOST_SERVICES: usize = 6;
/// How long each kind of evidence may take to read before the rest goes on without it.
const LOOK_WAIT: Duration = Duration::from_secs(4);
/// The most changes and similar incidents told as evidence.
const TOLD_CHANGES: usize = 8;
const TOLD_SIMILAR: usize = 3;

/// Where an incident's evidence is read from.
#[derive(Clone)]
pub struct EvidenceReader {
    apm: Arc<ApmQueryService>,
    query: Arc<QueryService>,
    streams: Arc<dyn StreamRepository>,
    business: Arc<dyn BusinessContextRepository>,
}

/// The stretch and the services evidence is read over.
struct Scope<'a> {
    incident: &'a Incident,
    window: TimeRange,
    services: Vec<String>,
}

impl EvidenceReader {
    pub fn new(
        apm: Arc<ApmQueryService>,
        query: Arc<QueryService>,
        streams: Arc<dyn StreamRepository>,
        business: Arc<dyn BusinessContextRepository>,
    ) -> Self {
        Self {
            apm,
            query,
            streams,
            business,
        }
    }

    /// The telemetry evidence around `incident`, best effort: a kind that cannot be read in time
    /// is left out and logged. `downstream` are the services its services call.
    pub async fn read(
        &self,
        incident: &Incident,
        downstream: &[String],
        now: TimestampMicros,
    ) -> Vec<RcaEvidence> {
        let scope = Scope {
            incident,
            window: incident_window(incident, now),
            services: services_of(incident, downstream),
        };

        if scope.services.is_empty() {
            return Vec::new();
        }

        let streams = self.streams.as_ref();

        let (insights, errors, traces, logs, patterns, skewed, business) = tokio::join!(
            within("insights", apm::insights(&self.apm, &scope)),
            within("new error groups", apm::new_error_groups(&self.apm, &scope)),
            within("sample traces", apm::sample_traces(&self.apm, &scope)),
            within(
                "error logs",
                logs::error_logs(&self.apm, &self.query, streams, &scope)
            ),
            within(
                "new log patterns",
                patterns::new_patterns(&self.query, streams, &scope)
            ),
            within(
                "fields failed requests differ by",
                skew::skewed_fields(&self.query, streams, &scope)
            ),
            within(
                "business impact",
                business::impact(&self.query, streams, self.business.as_ref(), &scope)
            ),
        );

        [insights, errors, skewed, logs, patterns, business, traces].concat()
    }
}

/// From the time the incident began less [`BEFORE`] to [`AFTER`] past it, no later than now.
pub fn incident_window(incident: &Incident, now: TimestampMicros) -> TimeRange {
    let began = incident.created_at.0;
    let end = (began + AFTER).min(now.0.max(began));

    TimeRange::new(TimestampMicros(began - BEFORE), TimestampMicros(end))
}

/// The incident's services, then those they call, each once, at most [`MOST_SERVICES`].
fn services_of(incident: &Incident, downstream: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut services = incident_services(incident)
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    seen.extend(services.iter().cloned());
    services.extend(
        downstream
            .iter()
            .filter(|service| !service.trim().is_empty() && seen.insert((*service).clone()))
            .cloned(),
    );
    services.truncate(MOST_SERVICES);
    services
}

/// A look within [`LOOK_WAIT`]; nothing when it fails or runs out of time.
async fn within(
    what: &'static str,
    look: impl Future<Output = kernel::Result<Vec<RcaEvidence>>>,
) -> Vec<RcaEvidence> {
    match tokio::time::timeout(LOOK_WAIT, look).await {
        Ok(Ok(evidence)) => evidence,
        Ok(Err(error)) => {
            tracing::warn!(%error, what, "an incident's evidence could not be read");
            Vec::new()
        }
        Err(_) => {
            tracing::warn!(what, "an incident's evidence took too long to read");
            Vec::new()
        }
    }
}

/// The changes and similar incidents a brief tells, as evidence: the changes nearest the time
/// the incident began, in time order, then the most similar incidents.
pub fn brief_evidence(brief: &IncidentBrief, began: TimestampMicros) -> Vec<RcaEvidence> {
    let mut evidence = Vec::new();

    if let Some(changes) = &brief.changes {
        let mut nearest = changes.changes.iter().collect::<Vec<_>>();
        nearest.sort_by_key(|change| (change.at.0 - began.0).abs());
        nearest.truncate(TOLD_CHANGES);
        nearest.sort_by_key(|change| change.at.0);

        evidence.extend(nearest.into_iter().map(|change| {
            let mut piece = RcaEvidence::new(EvidenceKind::Change, EvidenceSource::Incident);

            piece.service = change.service.clone();
            piece.subject = Some(brief::change_text(change));
            piece.detail = Some(change.kind.to_owned());
            piece.at = Some(change.at);
            piece
        }));
    }

    evidence.extend(brief.similar.iter().take(TOLD_SIMILAR).map(|similar| {
        let mut piece = RcaEvidence::new(EvidenceKind::SimilarIncident, EvidenceSource::Incident);

        piece.subject = Some(brief::similar_text(similar));
        piece.at = Some(similar.created_at);

        piece.link = Some(EvidenceLink {
            incident_id: Some(similar.id.0.clone()),
            ..EvidenceLink::default()
        });

        piece
    }));

    evidence
}

/// Every piece labelled in the order told: telemetry first, then what the brief tells.
pub fn labelled(telemetry: Vec<RcaEvidence>, told: Vec<RcaEvidence>) -> Vec<RcaEvidence> {
    label_evidence([telemetry, told].concat())
}
