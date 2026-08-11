// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use super::{
    MAX_COMPONENTS_PER_PAGE, StatusPageIncidentInput, StatusPageIncidentUpdateInput,
    StatusPageService, deduplicate_ids,
    validation::{validate_incident_input, validate_message, validate_transition},
};
use crate::{
    domain::{
        alerting::incident::IncidentStatus,
        status_page::{
            ComponentLifecycle, StatusPageEventList, StatusPageEventView, StatusPageIncident,
            StatusPageIncidentKind, StatusPageIncidentUpdate, StatusPageLifecycle,
            StatusPagePublicationState,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl StatusPageService {
    pub async fn get_event(
        &self,
        org_id: &Id,
        page_id: &Id,
        event_id: &Id,
    ) -> Result<StatusPageIncident> {
        self.repository
            .get_status_incident(org_id, page_id, event_id)
            .await
    }

    pub async fn create_incident(
        &self,
        org_id: &Id,
        page_id: &Id,
        input: StatusPageIncidentInput,
    ) -> Result<StatusPageIncident> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "events cannot be created on an archived status page",
            ));
        }
        let now = TimestampMicros::now();
        let incident = self
            .materialize_incident(org_id, page_id, Id::new(), now, now, input)
            .await?;
        self.repository.create_status_incident(incident).await
    }

    pub async fn update_incident_draft(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
        mut input: StatusPageIncidentInput,
    ) -> Result<StatusPageIncident> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "drafts cannot be changed on an archived status page",
            ));
        }
        let existing = self
            .repository
            .get_status_incident(org_id, page_id, incident_id)
            .await?;
        if existing.publication_state != StatusPagePublicationState::Draft {
            return Err(Error::conflict(
                "only a draft event can be edited as a draft",
            ));
        }
        input.publication_state = StatusPagePublicationState::Draft;
        let incident = self
            .materialize_incident(
                org_id,
                page_id,
                existing.id,
                existing.created_at,
                TimestampMicros::now(),
                input,
            )
            .await?;
        self.repository.update_status_incident_draft(incident).await
    }

    pub async fn publish_incident_draft(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
        mut input: StatusPageIncidentInput,
    ) -> Result<StatusPageIncident> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "drafts cannot be published on an archived status page",
            ));
        }
        let existing = self
            .repository
            .get_status_incident(org_id, page_id, incident_id)
            .await?;
        if existing.publication_state != StatusPagePublicationState::Draft {
            return Err(Error::conflict("event is already published"));
        }
        input.publication_state = StatusPagePublicationState::Published;
        let now = TimestampMicros::now();
        let mut incident = self
            .materialize_incident(
                org_id,
                page_id,
                existing.id,
                existing.created_at,
                now,
                input,
            )
            .await?;
        let initial_update = incident
            .updates
            .pop()
            .ok_or_else(|| Error::invalid("an initial update is required to publish"))?;
        self.repository
            .publish_status_incident(incident, initial_update)
            .await
    }

    pub async fn update_scheduled_maintenance(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
        mut input: StatusPageIncidentInput,
    ) -> Result<StatusPageIncident> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "maintenance cannot be changed on an archived status page",
            ));
        }
        let existing = self
            .repository
            .get_status_incident(org_id, page_id, incident_id)
            .await?;
        if existing.kind != StatusPageIncidentKind::Maintenance
            || existing.publication_state != StatusPagePublicationState::Published
            || existing.status != crate::domain::status_page::PublicIncidentStatus::Scheduled
        {
            return Err(Error::conflict(
                "only scheduled maintenance can be rescheduled",
            ));
        }
        input.kind = StatusPageIncidentKind::Maintenance;
        input.impact = crate::domain::status_page::IncidentImpact::Maintenance;
        input.status = crate::domain::status_page::PublicIncidentStatus::Scheduled;
        input.publication_state = StatusPagePublicationState::Published;
        input.source_incident_id = None;
        let now = TimestampMicros::now();
        let mut revised = self
            .materialize_incident(
                org_id,
                page_id,
                existing.id,
                existing.created_at,
                now,
                input,
            )
            .await?;
        revised.published_at = existing.published_at;
        let update = revised
            .updates
            .pop()
            .ok_or_else(|| Error::invalid("a maintenance update message is required"))?;
        self.repository
            .update_scheduled_maintenance(revised, update)
            .await
    }

    async fn materialize_incident(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: Id,
        created_at: TimestampMicros,
        now: TimestampMicros,
        input: StatusPageIncidentInput,
    ) -> Result<StatusPageIncident> {
        validate_incident_input(&input)?;
        let source_started_at = self.validate_source_incident(org_id, &input).await?;
        let component_ids = self
            .validate_event_components(org_id, page_id, input.component_ids)
            .await?;
        if input.publication_state == StatusPagePublicationState::Published
            && component_ids.is_empty()
        {
            return Err(Error::invalid(
                "at least one non-archived affected component is required",
            ));
        }
        let started_at = input.started_at.or(source_started_at).unwrap_or(now);
        if input.kind == StatusPageIncidentKind::Incident
            && started_at.0 > now.0.saturating_add(60_000_000)
        {
            return Err(Error::invalid("an incident cannot start in the future"));
        }
        let message = input
            .message
            .as_deref()
            .map(str::trim)
            .filter(|message| !message.is_empty());
        let updates = if input.publication_state == StatusPagePublicationState::Published {
            vec![StatusPageIncidentUpdate {
                id: Id::new(),
                org_id: org_id.clone(),
                status_page_id: page_id.clone(),
                incident_id: incident_id.clone(),
                status: input.status,
                message: message.unwrap_or_default().to_string(),
                created_at: now,
            }]
        } else {
            Vec::new()
        };
        Ok(StatusPageIncident {
            id: incident_id,
            org_id: org_id.clone(),
            status_page_id: page_id.clone(),
            source_incident_id: input.source_incident_id,
            kind: input.kind,
            title: input.title.trim().to_string(),
            impact: input.impact,
            status: input.status,
            publication_state: input.publication_state,
            draft_message: if input.publication_state == StatusPagePublicationState::Draft {
                message.map(str::to_string)
            } else {
                None
            },
            component_ids,
            updates,
            started_at,
            ended_at: None,
            published_at: (input.publication_state == StatusPagePublicationState::Published)
                .then_some(now),
            created_at,
            updated_at: now,
        })
    }

    async fn validate_event_components(
        &self,
        org_id: &Id,
        page_id: &Id,
        component_ids: Vec<Id>,
    ) -> Result<Vec<Id>> {
        if component_ids.len() > MAX_COMPONENTS_PER_PAGE {
            return Err(Error::invalid(
                "an event cannot affect more than 500 components",
            ));
        }
        let component_ids = deduplicate_ids(component_ids);
        let available_component_ids: HashSet<String> = self
            .repository
            .list_components(org_id, page_id)
            .await?
            .into_iter()
            .filter(|component| component.lifecycle == ComponentLifecycle::Active)
            .map(|component| component.id.0)
            .collect();
        if component_ids
            .iter()
            .any(|component_id| !available_component_ids.contains(component_id.as_str()))
        {
            return Err(Error::not_found(
                "affected active status-page component not found",
            ));
        }
        Ok(component_ids)
    }

    async fn validate_source_incident(
        &self,
        org_id: &Id,
        input: &StatusPageIncidentInput,
    ) -> Result<Option<TimestampMicros>> {
        match (input.kind, input.source_incident_id.as_ref()) {
            (StatusPageIncidentKind::Incident, Some(source_id)) => {
                let source = self.incidents.get(source_id).await?;
                if &source.org_id != org_id {
                    return Err(Error::not_found("source incident not found"));
                }
                if matches!(
                    source.status,
                    IncidentStatus::Resolved | IncidentStatus::Closed
                ) {
                    return Err(Error::conflict(
                        "source incident must still be active when it is published",
                    ));
                }
                Ok(Some(source.created_at))
            }
            (StatusPageIncidentKind::Incident, None)
            | (StatusPageIncidentKind::Maintenance, None) => Ok(None),
            (StatusPageIncidentKind::Maintenance, Some(_)) => Err(Error::invalid(
                "planned maintenance cannot link to an internal incident",
            )),
        }
    }

    pub async fn append_incident_update(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
        input: StatusPageIncidentUpdateInput,
    ) -> Result<StatusPageIncident> {
        let page = self.repository.get_page(org_id, page_id).await?;
        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "events cannot be updated on an archived status page",
            ));
        }
        validate_message(&input.message)?;
        let incident = self
            .repository
            .get_status_incident(org_id, page_id, incident_id)
            .await?;
        if incident.publication_state != StatusPagePublicationState::Published {
            return Err(Error::conflict("publish the draft before adding updates"));
        }
        validate_transition(incident.kind, incident.status, input.status)?;
        let now = TimestampMicros::now();
        self.repository
            .append_status_update(
                StatusPageIncidentUpdate {
                    id: Id::new(),
                    org_id: org_id.clone(),
                    status_page_id: page_id.clone(),
                    incident_id: incident_id.clone(),
                    status: input.status,
                    message: input.message.trim().to_string(),
                    created_at: now,
                },
                incident.status,
                input.status.is_terminal_for(incident.kind).then_some(now),
                now,
            )
            .await
    }

    pub async fn list_events(
        &self,
        org_id: &Id,
        page_id: &Id,
        kind: StatusPageIncidentKind,
        view: StatusPageEventView,
    ) -> Result<StatusPageEventList> {
        self.repository.get_page(org_id, page_id).await?;
        let limit = match view {
            StatusPageEventView::Resolved | StatusPageEventView::Completed => 10,
            _ => 200,
        };
        self.repository
            .list_status_events(org_id, page_id, kind, view, limit)
            .await
    }
}
