// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the organization endpoints answer with: the organization, the caller's roles in it, whether
//! the caller can enter it, and the organization's own logo and description.

use domain::iam::{IamAssignedRole, Organization};
use kernel::{Result, ids::Id};
use postgres::repositories::org_profiles::OrgProfile;
use serde::Serialize;

use crate::api::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OrgView {
    id: String,
    name: String,
    slug: String,
    display_role: Option<String>,
    roles: Vec<IamAssignedRole>,
    system: bool,
    disabled: bool,
    /// Whether the caller can enter the organization. From the platform organization an owner
    /// administers organizations they do not belong to: those are listed, and cannot be entered.
    accessible: bool,
    /// The organization's own logo, a `data:` image. Absent when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    logo: Option<String>,
    /// What the organization is for, in a line or two. Absent when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

impl OrgView {
    pub(super) fn from_org(
        organization: Organization,
        roles: Vec<IamAssignedRole>,
        accessible: bool,
    ) -> Self {
        let display_role = roles
            .iter()
            .map(|role| role.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        Self {
            id: organization.id.0,
            name: organization.name,
            slug: organization.slug,
            display_role: (!display_role.is_empty()).then_some(display_role),
            roles,
            system: organization.system,
            disabled: organization.disabled,
            accessible,
            logo: None,
            description: None,
        }
    }

    fn with_profile(mut self, profile: OrgProfile) -> Self {
        self.logo = profile.logo;
        self.description = (!profile.description.is_empty()).then_some(profile.description);
        self
    }
}

/// The organizations with their own logos and descriptions, read in one query.
pub(super) async fn with_profiles(state: &AppState, views: Vec<OrgView>) -> Result<Vec<OrgView>> {
    let ids = views
        .iter()
        .map(|view| Id(view.id.clone()))
        .collect::<Vec<_>>();
    let mut profiles = state.iam.org_profiles.among(&ids).await?;

    Ok(views
        .into_iter()
        .map(|view| match profiles.remove(&Id(view.id.clone())) {
            Some(profile) => view.with_profile(profile),
            None => view,
        })
        .collect())
}

/// One organization with its own logo and description.
pub(super) async fn with_profile(state: &AppState, view: OrgView) -> Result<OrgView> {
    let profile = state.iam.org_profiles.get(&Id(view.id.clone())).await?;

    Ok(view.with_profile(profile))
}
