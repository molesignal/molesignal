// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The entity sweep: every five minutes the primary alert node reads, organization by
//! organization, what services run on from APM's instances and from the resource fields of
//! metrics and log streams, and forgets what has not been seen for a week.

use std::{sync::Arc, time::Duration};

use domain::{entity::ENTITY_KEPT_MICROS, iam::OrganizationRepository};
use kernel::time::TimestampMicros;
use tokio::task::JoinHandle;

use crate::app::{cluster::WorkOwnership, entities::EntitySources};

const INTERVAL: Duration = Duration::from_secs(300);

pub struct EntitySweeper {
    orgs: Arc<dyn OrganizationRepository>,
    sources: EntitySources,
    ownership: Arc<WorkOwnership>,
}

impl EntitySweeper {
    pub fn new(
        orgs: Arc<dyn OrganizationRepository>,
        sources: EntitySources,
        ownership: Arc<WorkOwnership>,
    ) -> Self {
        Self {
            orgs,
            sources,
            ownership,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(INTERVAL);
            tick.tick().await;

            loop {
                tick.tick().await;

                if self.ownership.is_primary().await {
                    self.sweep_once().await;
                }
            }
        })
    }

    #[tracing::instrument(
        name = "worker.entities",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "entities")
    )]
    async fn sweep_once(&self) {
        let now = TimestampMicros::now();

        let orgs = match self.orgs.list().await {
            Ok(orgs) => orgs,
            Err(error) => {
                tracing::warn!(%error, "the entity sweep could not list organizations");
                return;
            }
        };

        // One organization that cannot be read leaves the others to be swept.
        for org in orgs {
            if let Err(error) = self.sources.sweep_org(&org.id, now).await {
                tracing::warn!(%error, org_id = %org.id.0, "an organization's entities were not swept");
            }
        }

        let cutoff = TimestampMicros(now.0 - ENTITY_KEPT_MICROS);

        if let Err(error) = self.sources.repository.forget_before(cutoff).await {
            tracing::warn!(%error, "entities not seen for a week were not forgotten");
        }
    }
}
