// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Updates on a published event: what changed, by when the next one comes,
//! and whether subscribers are told.

use super::{
    StatusPageIncidentUpdateInput, StatusPageService,
    validation::{validate_message, validate_transition},
};
use crate::{
    domain::status_page::{
        PublicIncidentStatus, StatusPageIncident, StatusPageIncidentKind, StatusPageIncidentUpdate,
        StatusPageLifecycle, StatusPagePublicationState,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// The nearest a promised next update can be.
const MIN_NEXT_UPDATE_MICROS: i64 = 60 * 1_000_000;

/// The furthest a promised next update can be.
const MAX_NEXT_UPDATE_MICROS: i64 = 7 * 24 * 60 * 60 * 1_000_000;

impl StatusPageService {
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
        validate_next_update(incident.kind, input.status, input.next_update_at, now)?;

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
                    next_update_at: input.next_update_at,
                    notify_subscribers: input.notify_subscribers,
                },
                incident.status,
                input.status.is_terminal_for(incident.kind).then_some(now),
                now,
            )
            .await
    }
}

/// A next update can be promised while an event is going on: not once it is
/// over, and not for maintenance that is only announced, whose planned start
/// already says when more will be known.
pub(super) fn validate_next_update(
    kind: StatusPageIncidentKind,
    status: PublicIncidentStatus,
    next_update_at: Option<TimestampMicros>,
    now: TimestampMicros,
) -> Result<()> {
    let Some(next_update_at) = next_update_at else {
        return Ok(());
    };

    if status.is_terminal_for(kind) || status == PublicIncidentStatus::Scheduled {
        return Err(Error::invalid(
            "a next update can only be promised while the event is going on",
        ));
    }

    let ahead = next_update_at.0.saturating_sub(now.0);

    if ahead < MIN_NEXT_UPDATE_MICROS {
        return Err(Error::invalid(
            "the next update must be at least one minute ahead",
        ));
    }

    if ahead > MAX_NEXT_UPDATE_MICROS {
        return Err(Error::invalid(
            "the next update can be at most seven days ahead",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60 * 1_000_000;

    #[test]
    fn a_next_update_is_promised_only_while_the_event_goes_on() {
        let now = TimestampMicros(1_000 * MINUTE);
        let soon = Some(TimestampMicros(now.0 + 30 * MINUTE));

        assert!(
            validate_next_update(
                StatusPageIncidentKind::Incident,
                PublicIncidentStatus::Identified,
                soon,
                now
            )
            .is_ok()
        );
        assert!(
            validate_next_update(
                StatusPageIncidentKind::Incident,
                PublicIncidentStatus::Resolved,
                soon,
                now
            )
            .is_err()
        );
        assert!(
            validate_next_update(
                StatusPageIncidentKind::Maintenance,
                PublicIncidentStatus::Scheduled,
                soon,
                now
            )
            .is_err()
        );
        assert!(
            validate_next_update(
                StatusPageIncidentKind::Incident,
                PublicIncidentStatus::Resolved,
                None,
                now
            )
            .is_ok()
        );
    }

    #[test]
    fn a_next_update_is_between_a_minute_and_a_week_ahead() {
        let now = TimestampMicros(1_000 * MINUTE);
        let at = |minutes: i64| Some(TimestampMicros(now.0 + minutes * MINUTE));
        let check = |next| {
            validate_next_update(
                StatusPageIncidentKind::Incident,
                PublicIncidentStatus::Monitoring,
                next,
                now,
            )
        };

        assert!(check(at(0)).is_err());
        assert!(check(at(1)).is_ok());
        assert!(check(at(7 * 24 * 60)).is_ok());
        assert!(check(at(7 * 24 * 60 + 1)).is_err());
    }
}
