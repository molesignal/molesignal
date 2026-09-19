// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use crate::{
    domain::{
        alerting::incident::Severity,
        status_page::{
            ActiveAutomationRule, AutomationCandidate, AutomationCandidateState,
            AutomationPublicationMode, AutomationSourceObservation, IncidentImpact,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn materialize_candidate(
    rule: &ActiveAutomationRule,
    observation: &AutomationSourceObservation,
) -> Result<AutomationCandidate> {
    let action = &rule.revision.action;
    let now = observation.observed_at;
    let title = render_template(&action.templates.title, observation)?;
    if title.chars().count() > 200 {
        return Err(Error::invalid(
            "rendered automation title exceeds 200 characters",
        ));
    }
    let is_follow_up = observation
        .labels
        .get("previous_synthetic_state")
        .is_some_and(|state| matches!(state.as_str(), "degraded" | "failing"));
    let message = render_template(
        if is_follow_up {
            &action.templates.update
        } else {
            &action.templates.investigating
        },
        observation,
    )?;
    let resolved_message = render_template(&action.templates.resolved, observation)?;
    let correlation = render_template(&action.correlation_key_template, observation)?;
    if correlation.chars().count() > 255 {
        return Err(Error::invalid(
            "rendered automation correlation key exceeds 255 characters",
        ));
    }
    let state = observation
        .labels
        .get("synthetic_state")
        .map(String::as_str);
    let impact = if state == Some("degraded") || observation.severity <= Severity::Warning {
        action.impact_map.degraded
    } else {
        action.impact_map.failing
    };
    if impact == IncidentImpact::Maintenance {
        return Err(Error::invalid(
            "automation Incident impact cannot be maintenance",
        ));
    }
    Ok(AutomationCandidate {
        id: Id::new(),
        organization_id: observation.organization_id.clone(),
        status_page_id: rule.rule.status_page_id.clone(),
        rule_revision_id: rule.revision.id.clone(),
        correlation_key: correlation,
        state: AutomationCandidateState::Delayed,
        title,
        message,
        resolved_message,
        impact,
        component_ids: action.component_ids.clone(),
        automatic: action.publication_mode == AutomationPublicationMode::Automatic,
        status_incident_id: None,
        due_at: TimestampMicros(
            now.0
                .saturating_add(i64::from(action.sustained_delay_seconds) * 1_000_000),
        ),
        created_at: now,
        updated_at: now,
        last_error: None,
    })
}

fn render_template(template: &str, observation: &AutomationSourceObservation) -> Result<String> {
    let mut rendered = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some(start) = remaining.find("{{") {
        rendered.push_str(&remaining[..start]);
        let after = &remaining[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| Error::invalid("automation template has an unclosed token"))?;
        let token = after[..end].trim();
        let value = match token {
            "source.id" | "monitor.id" | "incident.id" => observation.source_id.as_str(),
            "source.kind" => observation.source_kind.as_str(),
            "severity" => observation.severity.as_str(),
            token if token.starts_with("labels.") => observation
                .labels
                .get(&token[7..])
                .map(String::as_str)
                .unwrap_or_default(),
            _ => {
                return Err(Error::invalid(format!(
                    "unsupported automation token: {token}"
                )));
            }
        };
        rendered.push_str(value);
        remaining = &after[end + 2..];
    }
    rendered.push_str(remaining);
    let rendered = rendered.trim().to_string();
    if rendered.is_empty() || rendered.chars().count() > 4000 {
        return Err(Error::invalid(
            "rendered automation template must contain 1 to 4000 characters",
        ));
    }
    Ok(rendered)
}
