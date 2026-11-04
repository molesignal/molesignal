// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An organization's sharing policy, kept in the `resource_sharing` section of
//! `org_settings`. Without one, sharing stays closed ([`ResourceSharePolicy::secure_default`]).
//! The expiry range is checked by the settings endpoint.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use super::super::org_settings::{self, sections::RESOURCE_SHARING};
use crate::shared::{Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSharePolicy {
    pub organization_id: Id,
    pub allow_public_links: bool,
    pub allow_public_dashboards: bool,
    pub max_public_expiry_secs: i64,
    pub require_public_report_password: bool,
    pub deny_production_public_shares: bool,
    pub allow_public_csv_download: bool,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}

impl ResourceSharePolicy {
    pub fn secure_default(organization_id: Id) -> Self {
        Self {
            organization_id,
            allow_public_links: false,
            allow_public_dashboards: false,
            max_public_expiry_secs: 7 * 24 * 60 * 60,
            require_public_report_password: true,
            deny_production_public_shares: true,
            allow_public_csv_download: false,
            updated_by: Id::from_string(String::new()),
            updated_at: TimestampMicros(0),
        }
    }
}

/// The value of the `resource_sharing` section.
#[derive(Serialize, Deserialize)]
struct StoredPolicy {
    allow_public_links: bool,
    allow_public_dashboards: bool,
    max_public_expiry_secs: i64,
    require_public_report_password: bool,
    deny_production_public_shares: bool,
    allow_public_csv_download: bool,
}

pub(super) async fn get(pool: &PgPool, organization_id: &Id) -> Result<ResourceSharePolicy> {
    let section =
        org_settings::read::<StoredPolicy>(pool, &organization_id.0, RESOURCE_SHARING).await?;

    let Some(section) = section else {
        return Ok(ResourceSharePolicy::secure_default(organization_id.clone()));
    };

    let stored = section.value;

    Ok(ResourceSharePolicy {
        organization_id: organization_id.clone(),
        allow_public_links: stored.allow_public_links,
        allow_public_dashboards: stored.allow_public_dashboards,
        max_public_expiry_secs: stored.max_public_expiry_secs,
        require_public_report_password: stored.require_public_report_password,
        deny_production_public_shares: stored.deny_production_public_shares,
        allow_public_csv_download: stored.allow_public_csv_download,
        updated_by: Id::from_string(section.updated_by.unwrap_or_default()),
        updated_at: section.updated_at,
    })
}

pub(super) async fn put(pool: &PgPool, policy: ResourceSharePolicy) -> Result<ResourceSharePolicy> {
    let stored = StoredPolicy {
        allow_public_links: policy.allow_public_links,
        allow_public_dashboards: policy.allow_public_dashboards,
        max_public_expiry_secs: policy.max_public_expiry_secs,
        require_public_report_password: policy.require_public_report_password,
        deny_production_public_shares: policy.deny_production_public_shares,
        allow_public_csv_download: policy.allow_public_csv_download,
    };

    org_settings::write(
        pool,
        &policy.organization_id.0,
        RESOURCE_SHARING,
        &stored,
        Some(&policy.updated_by.0),
        policy.updated_at,
    )
    .await?;

    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_defaults_are_closed() {
        let policy = ResourceSharePolicy::secure_default(Id::from_string("org"));
        assert!(!policy.allow_public_links);
        assert!(!policy.allow_public_dashboards);
        assert!(policy.require_public_report_password);
        assert!(policy.deny_production_public_shares);
        assert_eq!(policy.max_public_expiry_secs, 604_800);
    }
}
