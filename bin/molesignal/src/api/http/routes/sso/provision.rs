// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Shared local-account provisioning after an external identity is verified.

use std::collections::BTreeMap;

use base64::Engine as _;
use serde::Serialize;

use crate::{
    api::AppState,
    domain::iam::{IamAssignedRole, IamMembership, SsoProvider, SsoProviderKind},
    infra::sso::SsoSession,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(in crate::api::http::routes) struct FederatedIdentity {
    pub email: String,
    pub display_name: String,
    pub subject: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct FederatedLoginResponse {
    pub token: String,
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub org_id: String,
    pub org_name: String,
    pub display_role: String,
    pub roles: Vec<IamAssignedRole>,
}

pub(in crate::api::http::routes) async fn complete_login(
    state: &AppState,
    provider: &SsoProvider,
    identity: FederatedIdentity,
    group_role_mapping: &BTreeMap<String, String>,
    default_role_id: Option<&str>,
) -> Result<FederatedLoginResponse> {
    let email = identity.email.trim().to_ascii_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(Error::unauthorized(
            "external identity does not contain a valid email address",
        ));
    }
    let org = state.iam.service.orgs.get(&provider.org_id).await?;
    org.ensure_enabled()?;
    let display_name = match identity.display_name.trim() {
        "" => email.clone(),
        name => name.to_owned(),
    };

    let user = match state.iam.service.users.get_by_email(&email).await {
        Ok(user) => user,
        Err(Error::NotFound(_)) => {
            super::super::email_domains::ensure_email_allowed(state, &provider.org_id, &email)
                .await?;
            state
                .iam
                .service
                .create_user(email.clone(), display_name, &random_password())
                .await?
        }
        Err(error) => return Err(error),
    };
    state.iam.service.ensure_user_access(&user.id).await?;

    let memberships = state
        .iam
        .service
        .iam_memberships
        .list_for_user(&user.id)
        .await?;
    let membership = memberships
        .iter()
        .find(|membership| membership.org_id == provider.org_id)
        .cloned();
    if membership.is_none() {
        super::super::email_domains::ensure_email_allowed(state, &provider.org_id, &email).await?;
    }
    let mut role_ids = map_role_ids(&identity.groups, group_role_mapping, default_role_id);
    if membership.is_none() && role_ids.is_empty() {
        role_ids.push(
            state
                .iam
                .service
                .iam_memberships
                .role_id_for_purpose(&provider.org_id, "self_service_signup")
                .await?,
        );
    }
    if membership.is_none() || !role_ids.is_empty() {
        state
            .iam
            .service
            .iam_memberships
            .upsert(
                IamMembership {
                    user_id: user.id.clone(),
                    org_id: provider.org_id.clone(),
                    joined_at: membership
                        .map(|membership| membership.joined_at)
                        .unwrap_or_else(TimestampMicros::now),
                },
                &role_ids,
                &user.id,
            )
            .await?;
    }

    let now = TimestampMicros::now();
    state
        .iam
        .sso_sessions
        .upsert(SsoSession {
            id: Id::new(),
            user_id: user.id.clone(),
            provider: provider_kind_name(provider.kind).into(),
            idp_subject: identity.subject,
            issued_at: now,
            last_login_at: now,
        })
        .await?;

    let roles = state
        .iam
        .service
        .iam_memberships
        .assigned_roles(&user.id, &provider.org_id)
        .await?;
    let display_role = roles
        .iter()
        .map(|role| role.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let token = state.iam.service.issue_token(&user.id, &provider.org_id)?;

    Ok(FederatedLoginResponse {
        token,
        user_id: user.id.0,
        email: user.email,
        display_name: user.display_name,
        org_id: provider.org_id.0.clone(),
        org_name: org.name,
        display_role,
        roles,
    })
}

fn map_role_ids(
    groups: &[String],
    mapping: &BTreeMap<String, String>,
    fallback: Option<&str>,
) -> Vec<Id> {
    let mut role_ids = groups
        .iter()
        .filter_map(|group| mapping.get(group))
        .map(|role_id| Id::from_string(role_id.clone()))
        .collect::<Vec<_>>();
    if role_ids.is_empty()
        && let Some(role_id) = fallback
    {
        role_ids.push(Id::from_string(role_id));
    }
    role_ids.sort_by(|left, right| left.0.cmp(&right.0));
    role_ids.dedup();
    role_ids
}

fn provider_kind_name(kind: SsoProviderKind) -> &'static str {
    match kind {
        SsoProviderKind::Oidc => "oidc",
        SsoProviderKind::Saml => "saml",
        SsoProviderKind::Ldap => "ldap",
    }
}

fn random_password() -> String {
    let mut bytes = [0_u8; 32];
    use rand::TryRng as _;
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("operating-system RNG");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::map_role_ids;

    #[test]
    fn role_mapping_is_deduplicated_and_sorted() {
        let mapping = BTreeMap::from([
            ("operators".into(), "role-z".into()),
            ("admins".into(), "role-a".into()),
        ]);
        let roles = map_role_ids(
            &["operators".into(), "admins".into(), "operators".into()],
            &mapping,
            None,
        );
        assert_eq!(
            roles.into_iter().map(|id| id.0).collect::<Vec<_>>(),
            ["role-a", "role-z"]
        );
    }
}
