// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A change as a pipeline, a webhook or a cluster reports it, made into the change kept: the
//! service and the environment named as the organization's naming rules, environment mapping
//! and aliases read them, a repository address kept with its password masked, now when no time
//! is said.

use std::sync::Arc;

use domain::apm::{
    ApmCatalogRepository, ChangeKind, DeploymentEvent, DeploymentStatus, masked_repository_address,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::Deserialize;

use crate::app::apm::OrgNames;

/// A deployment, or another change, as it is reported. A deployment needs the service and the
/// version, another change only the service; it went well and happened now unless said otherwise.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportedChange {
    #[serde(default)]
    pub kind: ChangeKind,
    pub service: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub status: Option<DeploymentStatus>,
    pub commit: Option<String>,
    pub repository: Option<String>,
    pub pull_request: Option<String>,
    pub author: Option<String>,
    pub ci_url: Option<String>,
    pub description: Option<String>,
    /// When it happened, in microseconds; now when absent.
    pub deployed_at: Option<i64>,
}

/// The change kept for `change`, checked.
pub fn change_event(
    names: &OrgNames,
    change: ReportedChange,
    created_by: Id,
    now: TimestampMicros,
) -> Result<DeploymentEvent> {
    let present = |value: Option<String>| {
        value
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let environment = change.environment.trim();

    let event = DeploymentEvent {
        id: Id::new(),
        kind: change.kind,
        namespace: change.namespace.trim().to_owned(),
        service: names.service(change.service.trim()),
        environment: if environment.is_empty() {
            String::new()
        } else {
            names.environment(environment).to_owned()
        },
        version: change.version.trim().to_owned(),
        status: change.status.unwrap_or(DeploymentStatus::Succeeded),
        commit: present(change.commit),
        // A password in the address never reaches the table.
        repository_url: present(change.repository)
            .map(|address| {
                masked_repository_address(&address)
                    .ok_or_else(|| Error::invalid("repository is not a repository address"))
            })
            .transpose()?,
        pull_request_url: present(change.pull_request),
        author: present(change.author),
        ci_url: present(change.ci_url),
        description: present(change.description),
        deployed_at: change.deployed_at.map_or(now, TimestampMicros),
        created_by,
        created_at: now,
    };

    event.validate()?;
    Ok(event)
}

/// Keeps reported changes.
#[derive(Clone)]
pub struct ChangeRecorder {
    catalog: Arc<dyn ApmCatalogRepository>,
}

impl ChangeRecorder {
    pub fn new(catalog: Arc<dyn ApmCatalogRepository>) -> Self {
        Self { catalog }
    }

    /// Keeps the changes reported for an organization, each checked before any is kept.
    pub async fn record(
        &self,
        org_id: &Id,
        changes: Vec<ReportedChange>,
        created_by: &Id,
    ) -> Result<Vec<DeploymentEvent>> {
        let settings = self.catalog.get_settings(org_id).await?.unwrap_or_default();
        let names = OrgNames::of(&settings);
        let now = TimestampMicros::now();

        let events = changes
            .into_iter()
            .map(|change| change_event(&names, change, created_by.clone(), now))
            .collect::<Result<Vec<_>>>()?;

        for event in &events {
            self.catalog.save_deployment(org_id, event).await?;
        }

        Ok(events)
    }
}
