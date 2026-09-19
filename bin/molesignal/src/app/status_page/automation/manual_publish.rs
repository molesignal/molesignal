// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use crate::{
    app::status_page::StatusPageService,
    domain::status_page::AutomationCandidateState,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

impl StatusPageService {
    pub async fn prepare_automation_draft_publish(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
    ) -> Result<bool> {
        let candidate = match self
            .repository
            .get_automation_candidate(org_id, incident_id)
            .await
        {
            Ok(candidate) => candidate,
            Err(Error::NotFound(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        if &candidate.status_page_id != page_id {
            return Ok(false);
        }
        if !matches!(
            candidate.state,
            AutomationCandidateState::PendingApproval | AutomationCandidateState::Approved
        ) {
            return Err(Error::conflict(
                "automation draft is no longer eligible for publication",
            ));
        }
        Ok(true)
    }

    pub async fn complete_automation_draft_publish(
        &self,
        org_id: &Id,
        page_id: &Id,
        incident_id: &Id,
        actor_id: &Id,
    ) -> Result<()> {
        let candidate = self
            .repository
            .mark_automation_candidate(
                org_id,
                incident_id,
                &[
                    AutomationCandidateState::PendingApproval,
                    AutomationCandidateState::Approved,
                    AutomationCandidateState::Cancelled,
                    AutomationCandidateState::Rejected,
                ],
                AutomationCandidateState::Published,
                Some(incident_id),
                Some(actor_id),
                Some("automation draft edited and published manually"),
                TimestampMicros::now(),
            )
            .await?;
        if &candidate.status_page_id != page_id {
            return Err(Error::not_found("Status Page automation Candidate"));
        }
        let detail = self
            .repository
            .get_automation_candidate_detail(org_id, &candidate.id)
            .await?;
        if !detail
            .sources
            .iter()
            .any(|source| source.active && !source.muted)
        {
            self.enqueue_candidate_work(&candidate, "resolve", TimestampMicros::now())
                .await?;
        }
        Ok(())
    }
}
