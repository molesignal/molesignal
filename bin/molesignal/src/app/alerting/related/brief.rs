// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a model is told about an incident besides its own facts: what its rule's investigation
//! guide says, what the service catalog says of its services, the past incidents most like it,
//! and the changes reported around the time it began. Only what the organization keeps about
//! incidents, rules, services and changes is read here, never telemetry, so a brief may go
//! wherever the incident itself may.

use std::sync::Arc;

use domain::alerting::incident::Incident;
use kernel::time::TimestampMicros;
use service_graph::ServiceGraphRepository;

use super::{
    BEFORE, IncidentChange, IncidentChanges, RelatedIncidents, SimilarIncident, downstream_of,
    incident_services,
};
use crate::app::{
    alerting::rule_guide::guide_facts,
    apm::{ApmQueryService, catalog_facts},
};

/// How many similar incidents a brief tells.
const TOLD_SIMILAR: usize = 3;
/// How many changes a brief tells: those nearest the time the incident began.
const TOLD_CHANGES: usize = 10;
/// The most characters of a change's description that are told.
const DESCRIPTION_CHARS: usize = 200;
/// The most characters of the rule's guide and of the catalog's facts that are told, so that a
/// long guide leaves room for the similar incidents and the changes.
const GUIDE_CHARS: usize = 2_500;
const CATALOG_CHARS: usize = 1_500;

/// The parts of an incident's brief; a part that could not be read is empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IncidentBrief {
    /// What the rule's investigation guide says, written for a model.
    pub guide: Option<String>,
    /// What the service catalog says of the incident's services.
    pub catalog: Option<String>,
    /// Past incidents like it, the most alike first. Whoever tells them keeps only those the
    /// reader may see; a brief tells the first few.
    pub similar: Vec<SimilarIncident>,
    /// The changes reported around the time it began.
    pub changes: Option<IncidentChanges>,
}

/// Where incident briefs are read from.
#[derive(Clone)]
pub struct IncidentBriefing {
    related: Arc<RelatedIncidents>,
    apm: Arc<ApmQueryService>,
    graph: Arc<dyn ServiceGraphRepository>,
}

impl IncidentBriefing {
    pub fn new(
        related: Arc<RelatedIncidents>,
        apm: Arc<ApmQueryService>,
        graph: Arc<dyn ServiceGraphRepository>,
    ) -> Self {
        Self {
            related,
            apm,
            graph,
        }
    }

    /// The brief of `incident`, best effort: a part that cannot be read is left out and logged.
    pub async fn brief(&self, incident: &Incident) -> IncidentBrief {
        let related = &self.related;

        let rule = related
            .rules
            .get(&incident.rule_id)
            .await
            .ok()
            .filter(|rule| rule.org_id == incident.org_id);

        let mut services: Vec<String> = incident_services(incident).into_iter().collect();
        services.sort();

        let catalog = match catalog_facts(related.catalog.as_ref(), &incident.org_id, &services)
            .await
        {
            Ok(facts) => facts,
            Err(error) => {
                tracing::warn!(incident = %incident.id.0, %error, "the catalog facts of an incident could not be read");
                None
            }
        };

        let similar = related.similar(incident).await.unwrap_or_else(|error| {
            tracing::warn!(incident = %incident.id.0, %error, "the incidents like an incident could not be read");
            Vec::new()
        });

        let downstream =
            downstream_of(&self.apm, self.graph.as_ref(), &incident.org_id, incident).await;

        let changes = match related.changes(incident, &downstream).await {
            Ok(changes) => Some(changes),
            Err(error) => {
                tracing::warn!(incident = %incident.id.0, %error, "the changes around an incident could not be read");
                None
            }
        };

        IncidentBrief {
            guide: rule.as_ref().and_then(guide_facts),
            catalog,
            similar,
            changes,
        }
    }
}

impl IncidentBrief {
    /// The brief as text for a model, its parts in order and the empty ones left out; empty when
    /// every part is.
    pub fn render(&self) -> String {
        let mut parts = Vec::new();

        if let Some(guide) = told(self.guide.as_deref()) {
            parts.push(format!(
                "Investigation guide of the alert rule:\n{}\nWhen the guide lists queries, run \
                 them over the incident's time range before anything else.",
                bounded(guide, GUIDE_CHARS)
            ));
        }

        if let Some(catalog) = told(self.catalog.as_deref()) {
            parts.push(format!(
                "What the service catalog says of its services:\n{}",
                bounded(catalog, CATALOG_CHARS)
            ));
        }

        if !self.similar.is_empty() {
            let lines = self
                .similar
                .iter()
                .take(TOLD_SIMILAR)
                .map(similar_line)
                .collect::<Vec<_>>()
                .join("\n");

            parts.push(format!(
                "Incidents of the last 90 days most like it:\n{lines}"
            ));
        }

        if let Some(changes) = &self.changes {
            parts.push(changes_part(changes));
        }

        parts.join("\n\n")
    }
}

fn told(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

fn similar_line(similar: &SimilarIncident) -> String {
    let mut line = format!(
        "- {} · {:?} · {}",
        minute(similar.created_at),
        similar.severity,
        one_line(&similar.summary)
    );

    match similar.resolved_at {
        Some(resolved) => line.push_str(&format!(
            " · resolved after {}",
            duration(resolved.0 - similar.created_at.0)
        )),
        None => line.push_str(&format!(" · still {:?}", similar.status)),
    }

    if !similar.reasons.is_empty() {
        line.push_str(&format!(" · alike in {}", similar.reasons.join(", ")));
    }

    if similar.notes > 0 {
        line.push_str(&format!(" · {} notes", similar.notes));
    }

    if let Some(root_cause) = told(similar.root_cause.as_deref()) {
        line.push_str(&format!(" · root cause: {}", one_line(root_cause)));
    }

    line
}

fn changes_part(changes: &IncidentChanges) -> String {
    let window = format!("from {} until {}", minute(changes.from), minute(changes.to));

    if changes.changes.is_empty() {
        return format!(
            "Changes: none was reported for its services or the services they call {window}."
        );
    }

    // The changes nearest the time the incident began, told in time order.
    let began = changes.from.0 + BEFORE;
    let mut nearest: Vec<&IncidentChange> = changes.changes.iter().collect();
    nearest.sort_by_key(|change| (change.at.0 - began).abs());
    nearest.truncate(TOLD_CHANGES);
    nearest.sort_by_key(|change| change.at.0);

    let lines = nearest
        .into_iter()
        .map(change_line)
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "Changes reported {window} for its services, and for the services they call (marked \
         downstream):\n{lines}"
    )
}

fn change_line(change: &IncidentChange) -> String {
    if change.kind == "rule" {
        return format!(
            "- {} · the alert rule \"{}\" was edited",
            minute(change.at),
            change.rule_name.as_deref().unwrap_or_default()
        );
    }

    let mut line = format!(
        "- {} · {}",
        minute(change.at),
        change.kind.replace('_', " ")
    );

    if let Some(service) = &change.service {
        line.push_str(&format!(" · {service}"));

        if change.downstream {
            line.push_str(" (downstream)");
        }
    }

    if let Some(version) = &change.version {
        line.push_str(&format!(" {version}"));
    }

    if let Some(status) = change.status {
        line.push_str(&format!(" ({})", status.as_str()));
    }

    if let Some(commit) = &change.commit {
        line.push_str(&format!(" · commit {commit}"));
    }

    if let Some(author) = &change.author {
        line.push_str(&format!(" · by {author}"));
    }

    if let Some(description) = told(change.description.as_deref()) {
        line.push_str(&format!(" · {}", one_line(description)));
    }

    line
}

/// A time to the minute, in UTC.
fn minute(at: TimestampMicros) -> String {
    chrono::DateTime::from_timestamp(at.0.div_euclid(1_000_000), 0)
        .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| at.0.to_string())
}

/// How long something took, roughly.
fn duration(micros: i64) -> String {
    let minutes = micros.max(0) / 60_000_000;

    match minutes {
        0 => "under a minute".into(),
        1..=59 => format!("{minutes}m"),
        60..=1_439 => format!("{}h {}m", minutes / 60, minutes % 60),
        _ => format!("{}d {}h", minutes / 1_440, (minutes % 1_440) / 60),
    }
}

/// Text cut to `most` characters, saying so when it was.
fn bounded(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }

    let kept = text.chars().take(most).collect::<String>();
    format!("{kept}\n[cut]")
}

/// Text on one line, cut to a length a brief can carry.
fn one_line(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if line.chars().count() <= DESCRIPTION_CHARS {
        return line;
    }

    let cut = line.chars().take(DESCRIPTION_CHARS).collect::<String>();
    format!("{cut}…")
}

#[cfg(test)]
#[path = "brief_tests.rs"]
mod tests;
