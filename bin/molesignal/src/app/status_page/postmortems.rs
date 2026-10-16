// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The public postmortem of a resolved incident: drafted while the incident
//! goes on or after it, published once it is resolved, and told to the
//! subscribers who hear about the incident.

use super::StatusPageService;
use crate::{
    domain::status_page::{
        PostmortemWrite, PublicIncidentStatus, StatusPageIncident, StatusPageIncidentKind,
        StatusPageLifecycle, StatusPagePublicationState,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const MAX_POSTMORTEM_CHARS: usize = 20_000;

impl StatusPageService {
    pub async fn save_postmortem(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
        body: &str,
        write: PostmortemWrite,
        actor_id: &Id,
    ) -> Result<StatusPageIncident> {
        let incident = self
            .postmortem_incident(org_id, page_id, incident_id)
            .await?;

        if matches!(write, PostmortemWrite::Publish { .. })
            && incident.status != PublicIncidentStatus::Resolved
        {
            return Err(Error::conflict(
                "a postmortem is published once the incident is resolved",
            ));
        }

        let body = validate_body(body)?;

        self.repository
            .save_postmortem(
                org_id,
                page_id,
                incident_id,
                body,
                write,
                Some(actor_id),
                TimestampMicros::now(),
            )
            .await
    }

    pub async fn delete_postmortem_draft(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
    ) -> Result<StatusPageIncident> {
        self.postmortem_incident(org_id, page_id, incident_id)
            .await?;

        self.repository
            .delete_postmortem_draft(org_id, page_id, incident_id)
            .await
    }

    /// The incident a postmortem is written for: a published incident on a
    /// page that is not archived.
    async fn postmortem_incident(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
    ) -> Result<StatusPageIncident> {
        let page = self.repository.get_page(org_id, page_id).await?;

        if page.lifecycle == StatusPageLifecycle::Archived {
            return Err(Error::conflict(
                "postmortems cannot be changed on an archived status page",
            ));
        }

        let incident = self
            .repository
            .get_status_incident(org_id, page_id, incident_id)
            .await?;

        if incident.kind != StatusPageIncidentKind::Incident {
            return Err(Error::invalid("only an incident can have a postmortem"));
        }

        if incident.publication_state != StatusPagePublicationState::Published {
            return Err(Error::conflict(
                "publish the incident before writing its postmortem",
            ));
        }

        Ok(incident)
    }
}

fn validate_body(body: &str) -> Result<&str> {
    let body = body.trim();

    if body.is_empty() || body.chars().count() > MAX_POSTMORTEM_CHARS {
        return Err(Error::invalid(
            "a postmortem must contain 1 to 20000 characters",
        ));
    }

    if body.contains('\0') {
        return Err(Error::invalid("a postmortem cannot contain NUL characters"));
    }

    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_postmortem_body_is_trimmed_and_bounded() {
        assert_eq!(
            validate_body("  ## Summary\n\nIt broke.  ").unwrap(),
            "## Summary\n\nIt broke."
        );
        assert!(validate_body("   ").is_err());
        assert!(validate_body(&"x".repeat(MAX_POSTMORTEM_CHARS)).is_ok());
        assert!(validate_body(&"x".repeat(MAX_POSTMORTEM_CHARS + 1)).is_err());
        assert!(validate_body("a\0b").is_err());
    }
}
