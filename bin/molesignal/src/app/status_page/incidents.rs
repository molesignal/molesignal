// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use super::{
    MAX_COMPONENTS_PER_PAGE, StatusPageIncidentInput, StatusPageService, deduplicate_ids,
    updates::validate_next_update, validation::validate_incident_input,
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

/// New maintenance starts by itself at its planned start and reminds
/// subscribers an hour before it, unless the request says otherwise.
const DEFAULT_MAINTENANCE_AUTO_START: bool = true;
const DEFAULT_MAINTENANCE_REMINDER_MINUTES: i32 = 60;

/// Whether an event starts by itself, and how many minutes ahead it reminds
/// subscribers. An incident does neither.
fn planned_options(input: &StatusPageIncidentInput) -> (bool, i32) {
    match input.kind {
        StatusPageIncidentKind::Maintenance => (
            input.auto_start.unwrap_or(DEFAULT_MAINTENANCE_AUTO_START),
            input
                .reminder_minutes
                .unwrap_or(DEFAULT_MAINTENANCE_REMINDER_MINUTES),
        ),
        StatusPageIncidentKind::Incident => (false, 0),
    }
}

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
        input.keep_planned_options_of(&existing);

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
        input.keep_planned_options_of(&existing);
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
        input.keep_planned_options_of(&existing);
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

    pub(super) async fn materialize_incident(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: Id,
        created_at: TimestampMicros,
        now: TimestampMicros,
        input: StatusPageIncidentInput,
    ) -> Result<StatusPageIncident> {
        validate_incident_input(&input)?;
        let (auto_start, reminder_minutes) = planned_options(&input);
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
            validate_next_update(input.kind, input.status, input.next_update_at, now)?;

            vec![StatusPageIncidentUpdate {
                id: Id::new(),
                org_id: org_id.clone(),
                status_page_id: page_id.clone(),
                incident_id: incident_id.clone(),
                status: input.status,
                message: message.unwrap_or_default().to_string(),
                created_at: now,
                next_update_at: input.next_update_at,
                notify_subscribers: input.notify_subscribers,
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
            auto_start,
            reminder_minutes,
            published_at: (input.publication_state == StatusPagePublicationState::Published)
                .then_some(now),
            created_at,
            updated_at: now,
            postmortem: None,
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
            // An event affects components; a group shows what its components show.
            .filter(|component| {
                !component.is_group() && component.lifecycle == ComponentLifecycle::Active
            })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::status_page::{IncidentImpact, PublicIncidentStatus};

    fn input(kind: StatusPageIncidentKind) -> StatusPageIncidentInput {
        StatusPageIncidentInput {
            source_incident_id: None,
            kind,
            title: "Database upgrade".into(),
            impact: IncidentImpact::Maintenance,
            status: PublicIncidentStatus::Scheduled,
            publication_state: StatusPagePublicationState::Published,
            message: Some("Writes can take a few seconds longer.".into()),
            component_ids: Vec::new(),
            started_at: None,
            auto_start: None,
            reminder_minutes: None,
            next_update_at: None,
            notify_subscribers: true,
        }
    }

    fn maintenance(auto_start: bool, reminder_minutes: i32) -> StatusPageIncident {
        StatusPageIncident {
            id: Id("maintenance".into()),
            org_id: Id("org".into()),
            status_page_id: Id("page".into()),
            source_incident_id: None,
            kind: StatusPageIncidentKind::Maintenance,
            title: "Database upgrade".into(),
            impact: IncidentImpact::Maintenance,
            status: PublicIncidentStatus::Scheduled,
            publication_state: StatusPagePublicationState::Published,
            draft_message: None,
            component_ids: Vec::new(),
            updates: Vec::new(),
            started_at: TimestampMicros(10),
            ended_at: None,
            auto_start,
            reminder_minutes,
            published_at: Some(TimestampMicros(1)),
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
            postmortem: None,
        }
    }

    #[test]
    fn new_maintenance_starts_by_itself_and_reminds_an_hour_ahead() {
        let named_nothing = input(StatusPageIncidentKind::Maintenance);
        assert_eq!(planned_options(&named_nothing), (true, 60));

        let mut opted_out = input(StatusPageIncidentKind::Maintenance);
        opted_out.auto_start = Some(false);
        opted_out.reminder_minutes = Some(0);
        assert_eq!(planned_options(&opted_out), (false, 0));
    }

    #[test]
    fn an_incident_neither_starts_by_itself_nor_reminds() {
        let mut incident = input(StatusPageIncidentKind::Incident);
        incident.auto_start = Some(true);
        incident.reminder_minutes = Some(120);

        assert_eq!(planned_options(&incident), (false, 0));
    }

    #[test]
    fn an_edit_that_names_no_option_keeps_what_the_maintenance_has() {
        let mut unnamed = input(StatusPageIncidentKind::Maintenance);
        unnamed.keep_planned_options_of(&maintenance(false, 0));
        assert_eq!(planned_options(&unnamed), (false, 0));

        let mut named = input(StatusPageIncidentKind::Maintenance);
        named.reminder_minutes = Some(1_440);
        named.keep_planned_options_of(&maintenance(false, 0));
        assert_eq!(planned_options(&named), (false, 1_440));
    }
}
