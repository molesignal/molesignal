// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Brings the built-in RUM dashboards of every organization that has applications up to the
//! version this build ships. A dashboard is installed when the first application of a platform is
//! created; this covers the organizations that had applications before the dashboards existed, and
//! the ones whose dashboards an upgrade has changed.

use std::{sync::Arc, time::Duration};

use tokio::task::JoinHandle;

use crate::{
    app::dashboard::{DashboardService, builtin::Installed},
    domain::{iam::OrganizationRepository, rum::RumApplicationRepository},
    shared::drain::DrainController,
};

/// Long enough for the node to be serving, so the sweep does not compete with the start.
const STARTUP_DELAY: Duration = Duration::from_secs(30);

pub fn spawn(
    organizations: Arc<dyn OrganizationRepository>,
    dashboards: Arc<DashboardService>,
    applications: Arc<dyn RumApplicationRepository>,
    drain: Arc<DrainController>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;

        if drain.is_draining() {
            return;
        }

        let installed = install_all(&organizations, &dashboards, applications.as_ref()).await;

        if installed > 0 {
            tracing::info!(dashboards = installed, "installed built-in RUM dashboards");
        }
    })
}

#[tracing::instrument(
    name = "worker.rum_dashboards",
    parent = None,
    skip_all,
    fields(otel.kind = "internal", molesignal.worker.name = "rum_dashboards")
)]
async fn install_all(
    organizations: &Arc<dyn OrganizationRepository>,
    dashboards: &DashboardService,
    applications: &dyn RumApplicationRepository,
) -> usize {
    let organizations = match organizations.list().await {
        Ok(organizations) => organizations,
        Err(error) => {
            tracing::warn!(%error, "organizations unreadable; RUM dashboards not installed");
            return 0;
        }
    };

    let mut installed = 0;

    for organization in organizations {
        match dashboards
            .install_rum_for_applications(&organization.id, applications)
            .await
        {
            Ok(done) => {
                installed += done
                    .iter()
                    .filter(|(_, outcome)| {
                        matches!(outcome, Installed::Created | Installed::Updated)
                    })
                    .count();
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    org_id = %organization.id.0,
                    "RUM dashboards could not be installed"
                );
            }
        }
    }

    installed
}
