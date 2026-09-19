// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use serde_json::json;

use super::{
    super::validation::validate_message, render::materialize_candidate, service::complete_match,
};
use crate::{
    app::status_page::{StatusPageIncidentInput, StatusPageIncidentUpdateInput, StatusPageService},
    domain::status_page::{
        AutomationCandidate, AutomationCandidateState, AutomationOutboxItem,
        AutomationSourceObservation, PublicIncidentStatus, StatusPageIncident,
        StatusPageIncidentKind, StatusPageIncidentUpdate, StatusPagePublicationState,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const OUTBOX_BATCH_LIMIT: u32 = 200;

impl StatusPageService {
    pub async fn observe_automation_source(
        &self,
        observation: AutomationSourceObservation,
    ) -> Result<Vec<AutomationCandidate>> {
        if !observation.active || observation.muted {
            return self.recover_automation_source(observation).await;
        }
        let rules = self
            .repository
            .list_matching_automation_rules(&observation)
            .await?;
        let mut matched_pages = HashSet::new();
        let mut candidates = Vec::new();
        for rule in rules
            .into_iter()
            .filter(|rule| complete_match(rule, &observation))
        {
            if !matched_pages.insert(rule.rule.status_page_id.clone()) {
                continue;
            }
            let candidate = materialize_candidate(&rule, &observation)?;
            let automatic = candidate.automatic;
            let persisted = self
                .repository
                .observe_automation_source(candidate, observation.clone(), automatic)
                .await?;
            if persisted.state == AutomationCandidateState::Published && persisted.automatic {
                self.enqueue_candidate_work(&persisted, "publish", observation.observed_at)
                    .await?;
            }
            candidates.push(persisted);
        }
        Ok(candidates)
    }

    pub async fn recover_automation_source(
        &self,
        mut observation: AutomationSourceObservation,
    ) -> Result<Vec<AutomationCandidate>> {
        observation.active = false;
        let candidates = self
            .repository
            .recover_automation_source(observation.clone())
            .await?;
        for candidate in &candidates {
            if candidate.state == AutomationCandidateState::Published {
                self.enqueue_candidate_work(candidate, "resolve", observation.observed_at)
                    .await?;
            }
        }
        Ok(candidates)
    }

    pub async fn process_due_automation_candidates(&self, limit: u32) -> Result<u32> {
        let now = TimestampMicros::now();
        let candidates = self
            .repository
            .list_due_automation_candidates(now, limit.min(500))
            .await?;
        let count = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
        for candidate in candidates {
            if candidate.automatic {
                self.enqueue_candidate_work(&candidate, "publish", candidate.due_at)
                    .await?;
            } else {
                let candidate = self.ensure_automation_draft(candidate, now).await?;
                self.enqueue_candidate_work(&candidate, "notify_publisher", candidate.due_at)
                    .await?;
            }
        }
        Ok(count)
    }

    pub async fn process_automation_outbox(&self, limit: u32) -> Result<u32> {
        let now = TimestampMicros::now();
        let items = self
            .repository
            .claim_automation_outbox(now, limit.min(OUTBOX_BATCH_LIMIT))
            .await?;
        let count = u32::try_from(items.len()).unwrap_or(u32::MAX);
        for item in items {
            let result = self.process_automation_item(&item).await;
            let completed_at = TimestampMicros::now();
            match result {
                Ok(()) => {
                    self.repository
                        .complete_automation_outbox(&item.id, completed_at)
                        .await?;
                }
                Err(error) => {
                    let retry_at = TimestampMicros(
                        completed_at
                            .0
                            .saturating_add(retry_delay_micros(item.attempts)),
                    );
                    self.repository
                        .fail_automation_outbox(&item.id, &error.to_string(), retry_at)
                        .await?;
                    tracing::warn!(
                        candidate_id = %item.candidate_id,
                        outbox_id = %item.id,
                        kind = item.kind,
                        error = %error,
                        "status-page automation work failed"
                    );
                }
            }
        }
        Ok(count)
    }

    async fn process_automation_item(&self, item: &AutomationOutboxItem) -> Result<()> {
        let candidate = self
            .repository
            .get_automation_candidate(&item.organization_id, &item.candidate_id)
            .await?;
        match item.kind.as_str() {
            "publish" => self
                .publish_automation_candidate(candidate)
                .await
                .map(|_| ()),
            "resolve" => self
                .resolve_automation_candidate(candidate)
                .await
                .map(|_| ()),
            "notify_publisher" => {
                tracing::info!(
                    organization_id = %candidate.organization_id,
                    status_page_id = %candidate.status_page_id,
                    candidate_id = %candidate.id,
                    "status-page automation Candidate requires approval"
                );
                if candidate.state == AutomationCandidateState::Failed {
                    self.repository
                        .mark_automation_candidate(
                            &candidate.organization_id,
                            &candidate.id,
                            &[AutomationCandidateState::Failed],
                            AutomationCandidateState::PendingApproval,
                            candidate.status_incident_id.as_ref(),
                            None,
                            Some("publisher notification retried"),
                            TimestampMicros::now(),
                        )
                        .await?;
                }
                Ok(())
            }
            _ => Err(Error::internal(
                "unknown status-page automation Outbox kind",
            )),
        }
    }

    async fn ensure_automation_draft(
        &self,
        candidate: AutomationCandidate,
        now: TimestampMicros,
    ) -> Result<AutomationCandidate> {
        let incident_id = candidate
            .status_incident_id
            .clone()
            .unwrap_or_else(|| candidate.id.clone());
        match self
            .repository
            .get_status_incident(
                &candidate.organization_id,
                &candidate.status_page_id,
                &incident_id,
            )
            .await
        {
            Ok(_) => {}
            Err(Error::NotFound(_)) => {
                let incident = self
                    .materialize_incident(
                        &candidate.organization_id,
                        &candidate.status_page_id,
                        incident_id.clone(),
                        candidate.created_at,
                        now,
                        candidate_input(&candidate, StatusPagePublicationState::Draft),
                    )
                    .await?;
                if let Err(error) = self.repository.create_status_incident(incident).await
                    && !matches!(error, Error::Conflict(_))
                {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
        self.repository
            .mark_automation_candidate(
                &candidate.organization_id,
                &candidate.id,
                &[
                    AutomationCandidateState::Delayed,
                    AutomationCandidateState::Failed,
                ],
                AutomationCandidateState::PendingApproval,
                Some(&incident_id),
                None,
                Some("sustained delay elapsed"),
                now,
            )
            .await
    }

    async fn publish_automation_candidate(
        &self,
        candidate: AutomationCandidate,
    ) -> Result<AutomationCandidate> {
        if matches!(
            candidate.state,
            AutomationCandidateState::Cancelled
                | AutomationCandidateState::Rejected
                | AutomationCandidateState::Resolved
        ) {
            return Ok(candidate);
        }
        let now = TimestampMicros::now();
        let incident_id = candidate
            .status_incident_id
            .clone()
            .unwrap_or_else(|| candidate.id.clone());
        let existing = self
            .repository
            .get_status_incident(
                &candidate.organization_id,
                &candidate.status_page_id,
                &incident_id,
            )
            .await;
        match existing {
            Ok(incident) if incident.publication_state == StatusPagePublicationState::Draft => {
                self.publish_incident_draft(
                    &candidate.organization_id,
                    &candidate.status_page_id,
                    &incident_id,
                    draft_input(&incident),
                )
                .await?;
            }
            Ok(incident) if incident.status == PublicIncidentStatus::Resolved => {}
            Ok(_) if !candidate.automatic => {}
            Ok(incident) => {
                let duplicate = self.automation_incident_update_is_duplicate(&candidate, &incident);
                let incident = self
                    .sync_automation_incident_metadata(&candidate, incident, now)
                    .await?;
                if !duplicate {
                    self.append_incident_update(
                        &candidate.organization_id,
                        &candidate.status_page_id,
                        &incident_id,
                        StatusPageIncidentUpdateInput {
                            status: incident.status,
                            message: candidate.message.clone(),
                        },
                    )
                    .await?;
                }
            }
            Err(Error::NotFound(_)) => {
                let incident = self
                    .materialize_incident(
                        &candidate.organization_id,
                        &candidate.status_page_id,
                        incident_id.clone(),
                        candidate.created_at,
                        now,
                        candidate_input(&candidate, StatusPagePublicationState::Published),
                    )
                    .await?;
                self.repository.create_status_incident(incident).await?;
            }
            Err(error) => return Err(error),
        }
        let published = self
            .repository
            .mark_automation_candidate(
                &candidate.organization_id,
                &candidate.id,
                &[
                    AutomationCandidateState::Delayed,
                    AutomationCandidateState::PendingApproval,
                    AutomationCandidateState::Approved,
                    AutomationCandidateState::Published,
                    AutomationCandidateState::Failed,
                    AutomationCandidateState::Cancelled,
                ],
                AutomationCandidateState::Published,
                Some(&incident_id),
                None,
                Some("customer-visible Incident published"),
                now,
            )
            .await?;
        let detail = self
            .repository
            .get_automation_candidate_detail(&published.organization_id, &published.id)
            .await?;
        if !detail
            .sources
            .iter()
            .any(|source| source.active && !source.muted)
        {
            self.enqueue_candidate_work(&published, "resolve", TimestampMicros::now())
                .await?;
        }
        Ok(published)
    }

    async fn resolve_automation_candidate(
        &self,
        candidate: AutomationCandidate,
    ) -> Result<AutomationCandidate> {
        if !matches!(
            candidate.state,
            AutomationCandidateState::Published | AutomationCandidateState::Failed
        ) {
            return Ok(candidate);
        }
        let incident_id = candidate
            .status_incident_id
            .clone()
            .ok_or_else(|| Error::internal("published automation Candidate has no Incident"))?;
        validate_message(&candidate.resolved_message)?;
        let now = TimestampMicros::now();
        Ok(self
            .repository
            .resolve_automation_candidate_if_inactive(
                &candidate.organization_id,
                &candidate.id,
                StatusPageIncidentUpdate {
                    id: Id::new(),
                    org_id: candidate.organization_id.clone(),
                    status_page_id: candidate.status_page_id.clone(),
                    incident_id,
                    status: PublicIncidentStatus::Resolved,
                    message: candidate.resolved_message.clone(),
                    created_at: now,
                },
            )
            .await?
            .unwrap_or(candidate))
    }

    pub(super) async fn enqueue_candidate_work(
        &self,
        candidate: &AutomationCandidate,
        kind: &str,
        observed_at: TimestampMicros,
    ) -> Result<AutomationOutboxItem> {
        self.repository
            .enqueue_automation_outbox(
                candidate,
                AutomationOutboxItem {
                    id: Id::new(),
                    organization_id: candidate.organization_id.clone(),
                    candidate_id: candidate.id.clone(),
                    kind: kind.to_string(),
                    idempotency_key: format!(
                        "status-automation:{}:{kind}:{}",
                        candidate.id, observed_at.0
                    ),
                    payload: json!({ "observed_at_micros": observed_at.0 }),
                    attempts: 0,
                    available_at: observed_at,
                    created_at: TimestampMicros::now(),
                },
            )
            .await
    }
}

fn candidate_input(
    candidate: &AutomationCandidate,
    publication_state: StatusPagePublicationState,
) -> StatusPageIncidentInput {
    StatusPageIncidentInput {
        source_incident_id: None,
        kind: StatusPageIncidentKind::Incident,
        title: candidate.title.clone(),
        impact: candidate.impact,
        status: PublicIncidentStatus::Investigating,
        publication_state,
        message: Some(candidate.message.clone()),
        component_ids: candidate.component_ids.clone(),
        started_at: Some(candidate.created_at),
    }
}

fn draft_input(incident: &StatusPageIncident) -> StatusPageIncidentInput {
    StatusPageIncidentInput {
        source_incident_id: incident.source_incident_id.clone(),
        kind: incident.kind,
        title: incident.title.clone(),
        impact: incident.impact,
        status: incident.status,
        publication_state: StatusPagePublicationState::Published,
        message: incident.draft_message.clone(),
        component_ids: incident.component_ids.clone(),
        started_at: Some(incident.started_at),
    }
}

fn retry_delay_micros(attempt: u32) -> i64 {
    let seconds = 5_i64.saturating_mul(1_i64 << attempt.min(10));
    seconds.min(30 * 60) * 1_000_000
}
