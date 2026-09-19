// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use crate::{
    app::status_page::StatusPageService,
    domain::status_page::{AutomationCandidate, StatusPageIncident},
    shared::{Result, time::TimestampMicros},
};

impl StatusPageService {
    pub(super) fn automation_incident_update_is_duplicate(
        &self,
        candidate: &AutomationCandidate,
        incident: &StatusPageIncident,
    ) -> bool {
        let metadata_changed = candidate.automatic
            && (incident.title != candidate.title
                || incident.impact != candidate.impact
                || incident.component_ids != candidate.component_ids);
        !metadata_changed
            && incident
                .updates
                .last()
                .is_some_and(|update| update.message == candidate.message)
    }

    pub(super) async fn sync_automation_incident_metadata(
        &self,
        candidate: &AutomationCandidate,
        mut incident: StatusPageIncident,
        updated_at: TimestampMicros,
    ) -> Result<StatusPageIncident> {
        let changed = incident.title != candidate.title
            || incident.impact != candidate.impact
            || incident.component_ids != candidate.component_ids;
        if !candidate.automatic || !changed {
            return Ok(incident);
        }
        incident.title.clone_from(&candidate.title);
        incident.impact = candidate.impact;
        incident.component_ids.clone_from(&candidate.component_ids);
        incident.updated_at = updated_at;
        self.repository
            .update_status_incident_metadata(incident)
            .await
    }
}
