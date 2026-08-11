// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::{AutomationApprovalInput, model::validate_decision_note};
use crate::{
    app::status_page::{StatusPageIncidentInput, StatusPageService},
    domain::status_page::{
        AutomationCandidate, AutomationCandidateState, StatusPagePublicationState,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl StatusPageService {
    pub async fn approve_automation_candidate(
        &self,
        org_id: &Id,
        page_id: &Id,
        candidate_id: &Id,
        actor_id: &Id,
        input: AutomationApprovalInput,
    ) -> Result<AutomationCandidate> {
        let AutomationApprovalInput {
            title,
            impact,
            message,
            component_ids,
            note,
        } = input;
        let note = note
            .map(|note| note.trim().to_string())
            .filter(|note| !note.is_empty());
        validate_decision_note(note.as_deref())?;
        if title.trim().is_empty() || message.trim().is_empty() || component_ids.is_empty() {
            return Err(Error::invalid(
                "approval requires a title, customer update, and affected Components",
            ));
        }
        let candidate = self
            .get_automation_candidate(org_id, page_id, candidate_id)
            .await?;
        if candidate.state != AutomationCandidateState::PendingApproval {
            return Err(Error::conflict(
                "automation Candidate is no longer pending approval",
            ));
        }
        let incident_id = candidate
            .status_incident_id
            .clone()
            .ok_or_else(|| Error::internal("approval Candidate has no draft Incident"))?;
        let incident = self.get_event(org_id, page_id, &incident_id).await?;
        self.update_incident_draft(
            org_id,
            page_id,
            &incident_id,
            StatusPageIncidentInput {
                source_incident_id: incident.source_incident_id,
                kind: incident.kind,
                title,
                impact,
                status: incident.status,
                publication_state: StatusPagePublicationState::Draft,
                message: Some(message),
                component_ids,
                started_at: Some(incident.started_at),
            },
        )
        .await?;

        let now = TimestampMicros::now();
        let candidate = self
            .repository
            .mark_automation_candidate(
                org_id,
                candidate_id,
                &[AutomationCandidateState::PendingApproval],
                AutomationCandidateState::Approved,
                Some(&incident_id),
                Some(actor_id),
                note.as_deref().or(Some("approved for publication")),
                now,
            )
            .await?;
        self.enqueue_candidate_work(&candidate, "publish", now)
            .await?;
        Ok(candidate)
    }

    pub async fn reject_automation_candidate(
        &self,
        org_id: &Id,
        page_id: &Id,
        candidate_id: &Id,
        actor_id: &Id,
        note: Option<&str>,
    ) -> Result<AutomationCandidate> {
        let note = note.map(str::trim).filter(|note| !note.is_empty());
        validate_decision_note(note)?;
        let note = note.ok_or_else(|| Error::invalid("rejection note is required"))?;
        self.get_automation_candidate(org_id, page_id, candidate_id)
            .await?;
        self.repository
            .mark_automation_candidate(
                org_id,
                candidate_id,
                &[AutomationCandidateState::PendingApproval],
                AutomationCandidateState::Rejected,
                None,
                Some(actor_id),
                Some(note),
                TimestampMicros::now(),
            )
            .await
    }
}
