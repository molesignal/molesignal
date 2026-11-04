// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Putting a built-in dashboard in an organization, and bringing it up to the version this build
//! ships.

use serde_json::Value;

use super::rum::{AUTHOR, RumDashboard, VERSION, installed_version};
use crate::{
    app::dashboard::{DashboardService, apply_server_model_fields},
    domain::{dashboard::Dashboard, rum::RumApplicationRepository},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// What installing a dashboard did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    Created,
    Updated,
    /// The organization had it at this version already.
    Current,
    /// A dashboard somebody else made has the uid; it is left as it is.
    Taken,
}

/// Whether a stored model is the built-in dashboard, of an older version, and not one a person made
/// and gave the same uid.
fn is_builtin(model: &Value, dashboard: RumDashboard) -> bool {
    model
        .pointer("/extensions/builtin/key")
        .and_then(Value::as_str)
        == Some(dashboard.uid())
}

impl DashboardService {
    /// Makes sure the organization has the dashboard, at the version this build ships: it is
    /// created when it is missing and written again when the organization has an older one. What
    /// a person changed in it is not kept: a built-in dashboard is read-only, and copied to be
    /// changed. A dashboard that a person made and gave the uid is never written over.
    pub async fn install_rum(&self, org_id: &Id, dashboard: RumDashboard) -> Result<Installed> {
        let author = Id::from_string(AUTHOR);
        let now = TimestampMicros::now();

        match self.dashboards.get_by_uid(org_id, dashboard.uid()).await {
            Ok(stored) if installed_version(&stored.model) >= VERSION => Ok(Installed::Current),
            Ok(stored) if !is_builtin(&stored.model, dashboard) => {
                tracing::warn!(
                    org_id = %org_id.0,
                    uid = dashboard.uid(),
                    "a dashboard that is not the built-in one has the uid; it is left as it is"
                );

                Ok(Installed::Taken)
            }
            Ok(stored) => {
                let version = stored.version.saturating_add(1);
                let mut model = dashboard.model(&stored.id);

                apply_server_model_fields(
                    &mut model,
                    &stored.id,
                    dashboard.uid(),
                    version,
                    stored.folder_id.as_ref(),
                )?;

                self.dashboards
                    .update(Dashboard {
                        title: dashboard.title().to_string(),
                        tags: dashboard.tags().map(str::to_string).to_vec(),
                        model,
                        version,
                        updated_at: now,
                        updated_by: author,
                        ..stored
                    })
                    .await?;

                Ok(Installed::Updated)
            }
            Err(Error::NotFound(_)) => {
                let id = dashboard.id(org_id);
                let mut model = dashboard.model(&id);

                apply_server_model_fields(&mut model, &id, dashboard.uid(), 1, None)?;

                self.dashboards
                    .create(Dashboard {
                        id,
                        org_id: org_id.clone(),
                        folder_id: None,
                        uid: dashboard.uid().to_string(),
                        title: dashboard.title().to_string(),
                        tags: dashboard.tags().map(str::to_string).to_vec(),
                        model,
                        version: 1,
                        created_at: now,
                        updated_at: now,
                        created_by: author.clone(),
                        updated_by: author,
                    })
                    .await?;

                Ok(Installed::Created)
            }
            Err(error) => Err(error),
        }
    }

    /// Installs the dashboards of the platforms an organization has applications of. Applications
    /// of a platform nothing is shown for are left out.
    pub async fn install_rum_for_applications(
        &self,
        org_id: &Id,
        applications: &dyn RumApplicationRepository,
    ) -> Result<Vec<(RumDashboard, Installed)>> {
        let mut wanted = Vec::new();

        for application in applications.list(org_id).await? {
            if let Some(dashboard) = RumDashboard::of(application.platform)
                && !wanted.contains(&dashboard)
            {
                wanted.push(dashboard);
            }
        }

        let mut installed = Vec::with_capacity(wanted.len());

        for dashboard in wanted {
            installed.push((dashboard, self.install_rum(org_id, dashboard).await?));
        }

        Ok(installed)
    }
}
