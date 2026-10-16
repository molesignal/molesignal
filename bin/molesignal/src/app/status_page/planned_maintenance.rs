// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What planned maintenance does on its own schedule: it starts at its planned
//! time and reminds subscribers before it.

use std::collections::HashMap;

use super::{StatusPageService, notification_copy};
use crate::{
    domain::status_page::{MaintenanceDueToStart, PublicIncidentStatus, StatusPageIncidentUpdate},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// What one sweep over planned maintenance did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlannedMaintenanceSweep {
    pub started: u32,
    pub reminders_queued: u64,
}

impl StatusPageService {
    /// Queues the reminders that are due and starts the maintenance whose
    /// planned start has come.
    ///
    /// Safe to run on several nodes at once. A reminder is claimed once for
    /// each planned start, and a start is refused once the maintenance has left
    /// the scheduled status.
    pub async fn run_planned_maintenance(&self, limit: u32) -> Result<PlannedMaintenanceSweep> {
        let now = TimestampMicros::now();
        let reminders_queued = self.repository.enqueue_maintenance_reminders(now).await?;

        let due = self
            .repository
            .list_maintenance_due_to_start(now, limit.min(500))
            .await?;

        // The update is written in the default language of each page, read once per sweep.
        let mut update_texts: HashMap<Id, String> = HashMap::new();
        let mut started = 0;

        for maintenance in due {
            match self
                .start_maintenance(&maintenance, &mut update_texts)
                .await
            {
                Ok(true) => started += 1,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(
                        status_page_id = %maintenance.status_page_id,
                        incident_id = %maintenance.incident_id,
                        error = %error,
                        "planned maintenance could not be started"
                    );
                }
            }
        }

        Ok(PlannedMaintenanceSweep {
            started,
            reminders_queued,
        })
    }

    /// Publishes the update that moves maintenance to in progress; subscribers
    /// are notified of it like of any other update. `false` when a person or
    /// another node moved the maintenance first.
    async fn start_maintenance(
        &self,
        maintenance: &MaintenanceDueToStart,
        update_texts: &mut HashMap<Id, String>,
    ) -> Result<bool> {
        let message = match update_texts.get(&maintenance.status_page_id) {
            Some(message) => message.clone(),
            None => {
                let page = self
                    .repository
                    .get_page(&maintenance.org_id, &maintenance.status_page_id)
                    .await?;

                let pack = self.language_pack(&page.language).await?;
                let message = notification_copy::maintenance_started_update(&pack)?;
                update_texts.insert(maintenance.status_page_id.clone(), message.clone());
                message
            }
        };

        let now = TimestampMicros::now();

        let started = self
            .repository
            .append_status_update(
                StatusPageIncidentUpdate {
                    id: Id::new(),
                    org_id: maintenance.org_id.clone(),
                    status_page_id: maintenance.status_page_id.clone(),
                    incident_id: maintenance.incident_id.clone(),
                    status: PublicIncidentStatus::InProgress,
                    message,
                    created_at: now,
                    next_update_at: None,
                    notify_subscribers: true,
                },
                PublicIncidentStatus::Scheduled,
                None,
                now,
            )
            .await;

        match started {
            Ok(_) => Ok(true),
            Err(Error::Conflict(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }
}
