// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An incident attached to a question: its own facts, then its brief — what its rule's
//! investigation guide says, what the service catalog says of its services, the past incidents
//! like it that the asker may read, and the changes reported around the time it began. The same
//! context serves `@` references, investigations started from an incident, automations an
//! incident starts, and `/rca`.

use std::time::Duration;

use domain::{alerting::incident::Incident, iam::IamContext};

use crate::{
    api::{AppState, rca::incident_facts},
    app::alerting::related::{IncidentBrief, IncidentBriefing, SimilarIncident},
};

/// The most characters an incident's context gives the model: more than other references take,
/// as it carries its brief too.
pub(super) const FACTS_CHARS: usize = 8_000;
/// How long the brief may take to read before the question goes on with the incident's own facts.
const BRIEF_WAIT: Duration = Duration::from_secs(8);

/// What the model is told about an incident the asker may read.
pub(crate) async fn context(state: &AppState, ctx: &IamContext, incident: &Incident) -> String {
    let briefing = IncidentBriefing::new(
        state.alerting.related.clone(),
        state.telemetry.apm_query.clone(),
        state.telemetry.service_graph.clone(),
    );

    let mut brief = match tokio::time::timeout(BRIEF_WAIT, briefing.brief(incident)).await {
        Ok(brief) => brief,
        Err(_) => {
            tracing::warn!(incident = %incident.id.0, "an incident's brief took too long to read");
            IncidentBrief::default()
        }
    };

    brief.similar = readable(state, ctx, brief.similar).await;

    let facts = incident_facts(incident);
    let told = brief.render();

    if told.is_empty() {
        facts
    } else {
        format!("{}\n\n{told}", facts.trim_end())
    }
}

/// The similar incidents the asker may read, in the same order.
async fn readable(
    state: &AppState,
    ctx: &IamContext,
    similar: Vec<SimilarIncident>,
) -> Vec<SimilarIncident> {
    let mut kept = Vec::with_capacity(similar.len());

    for incident in similar {
        if super::allowed(state, ctx, "alerts.incidents.read", "alert", &incident.id.0).await {
            kept.push(incident);
        }
    }

    kept
}
