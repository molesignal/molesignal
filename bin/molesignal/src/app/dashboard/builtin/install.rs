// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Putting a built-in dashboard in an organization, and bringing it up to the version this build
//! ships.

use domain::{dashboard::Dashboard, rum::RumApplicationRepository};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::Serialize;
use serde_json::Value;

use super::rum::RumDashboard;
use crate::app::dashboard::{DashboardService, apply_server_model_fields};

/// The author the dashboards the product installs are written by.
pub(in crate::app::dashboard) const AUTHOR: &str = "molesignal-system";

/// A dashboard the product installs in an organization and keeps current: read-only, written in
/// code, and found again by its uid.
pub trait Builtin: Copy {
    /// What the dashboard is called to the organization: fixed, so the same one is found again.
    fn uid(self) -> &'static str;

    /// The id the dashboard has in an organization: the uid and the organization, so that the
    /// pages can link to it without asking for it, and two organizations never share one.
    fn id(self, org_id: &Id) -> Id {
        Id::from_string(format!("{}-{}", self.uid(), org_id.0))
    }

    fn title(self) -> &'static str;

    fn tags(self) -> Vec<&'static str>;

    /// The version of the content this build ships: an organization whose dashboard is older gets
    /// the new one.
    fn version(self) -> u32;

    /// The model of the dashboard, with the identity the organization stores it under.
    fn model(self, id: &Id) -> Value;
}

/// The version of the content a stored model of a built-in dashboard holds; zero for a model that
/// is not one.
pub fn installed_version(model: &Value) -> u32 {
    model
        .pointer("/extensions/builtin/version")
        .and_then(Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .unwrap_or(0)
}

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

/// Where an organization's copy of a built-in dashboard stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Standing {
    Missing,
    /// Installed at a version older than this build ships.
    Outdated,
    Current,
    /// A dashboard somebody else made has the uid.
    Taken,
}

/// Whether a stored model is the built-in dashboard, of an older version, and not one a person made
/// and gave the same uid.
fn is_builtin(model: &Value, dashboard: impl Builtin) -> bool {
    model
        .pointer("/extensions/builtin/key")
        .and_then(Value::as_str)
        == Some(dashboard.uid())
}

impl DashboardService {
    /// Where the organization's copy of `dashboard` stands.
    pub async fn standing(&self, org_id: &Id, dashboard: impl Builtin) -> Result<Standing> {
        match self.dashboards.get_by_uid(org_id, dashboard.uid()).await {
            Ok(stored) if !is_builtin(&stored.model, dashboard) => Ok(Standing::Taken),
            Ok(stored) if installed_version(&stored.model) < dashboard.version() => {
                Ok(Standing::Outdated)
            }
            Ok(_) => Ok(Standing::Current),
            Err(Error::NotFound(_)) => Ok(Standing::Missing),
            Err(error) => Err(error),
        }
    }

    /// Makes sure the organization has the dashboard, at the version this build ships: it is
    /// created when it is missing and written again when the organization has an older one. What
    /// a person changed in it is not kept: a built-in dashboard is read-only, and copied to be
    /// changed. A dashboard that a person made and gave the uid is never written over.
    pub async fn install(&self, org_id: &Id, dashboard: impl Builtin) -> Result<Installed> {
        let author = Id::from_string(AUTHOR);
        let now = TimestampMicros::now();

        match self.dashboards.get_by_uid(org_id, dashboard.uid()).await {
            Ok(stored) if installed_version(&stored.model) >= dashboard.version() => {
                Ok(Installed::Current)
            }
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
                        tags: dashboard.tags().into_iter().map(str::to_string).collect(),
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
                        tags: dashboard.tags().into_iter().map(str::to_string).collect(),
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
            installed.push((dashboard, self.install(org_id, dashboard).await?));
        }

        Ok(installed)
    }
}
