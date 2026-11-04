// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 身份核验之后，这次登录会落到哪个账号、得到哪些角色。
//!
//! 判断和 [`provision::sign_in`] 是同一套（`policy`、`match_account`、`admit`），这里只读不写：不开户、
//! 不绑定、不入组、不改角色。真登录时会拒绝的，这里同样报错，由调用方算作没通过。

use std::collections::{BTreeMap, HashMap};

use super::{
    super::{
        linking::mask_email,
        policy,
        provision::{self, AccountMatch, FederatedIdentity},
    },
    failure::{DIRECTORY_BLOCKED, ROLE_MISSING},
};
use crate::{
    api::{AppState, http::routes::mfa},
    app::iam::{
        SignInMethod,
        signin_policy::{self, Admission},
    },
    domain::iam::{SsoProvider, SsoProviderKind, User},
    infra::sso::{TestedAccount, TestedMapping, TestedRole, TestedRoleSource},
    shared::{Error, Result, ids::Id},
};

/// 通过了也值得看一眼的事（`sso_providers.test.notes.<code>`）。
mod note {
    /// 配了角色映射，身份源却没给用户组。
    pub const NO_GROUPS: &str = "no_groups";
    /// 配了角色映射，身份源给的用户组一个都没命中。
    pub const GROUPS_UNMAPPED: &str = "groups_unmapped";
    /// 落到的不是发起测试的人的账号。
    pub const ANOTHER_ACCOUNT: &str = "another_account";
    /// 这个成员的角色会被换掉。
    pub const ROLES_CHANGE: &str = "roles_change";
    /// 目录登录之后还要第二步。
    pub const SECOND_FACTOR: &str = "second_factor";
}

pub(super) struct Preview {
    pub mapping: TestedMapping,
    pub notes: Vec<String>,
}

/// 账号和要同步的角色：已有账号时是 `admit` 的结论，新账号时按新成员算。
struct Destination {
    account: TestedAccount,
    user: Option<User>,
    email: String,
    member: bool,
    role_ids: Vec<Id>,
}

pub(super) async fn preview(
    state: &AppState,
    provider: &SsoProvider,
    identity: &FederatedIdentity,
    tester: &Id,
) -> Result<Preview> {
    policy::ensure_identity_allowed(state, provider, identity).await?;

    let destination = destination_of(state, provider, identity).await?;

    let second_factor = if provider.kind == SsoProviderKind::Ldap {
        directory_second_step(state, &provider.org_id, destination.user.as_ref()).await?
    } else {
        false
    };

    let mapping = mapping_of(state, provider, identity, tester, destination).await?;
    let notes = notes_of(provider, identity, &mapping, second_factor);

    Ok(Preview { mapping, notes })
}

async fn destination_of(
    state: &AppState,
    provider: &SsoProvider,
    identity: &FederatedIdentity,
) -> Result<Destination> {
    let (account, user) = match provision::match_account(state, provider, identity).await? {
        AccountMatch::Bound { user, .. } => (TestedAccount::Bound, user),
        AccountMatch::Existing(user) => (TestedAccount::Link, user),
        AccountMatch::New { email } => {
            super::super::super::email_domains::ensure_email_allowed(
                state,
                &provider.org_id,
                &email,
            )
            .await?;

            state
                .iam
                .service
                .orgs
                .get(&provider.org_id)
                .await?
                .ensure_enabled()?;

            return Ok(Destination {
                account: TestedAccount::Create,
                user: None,
                email,
                member: false,
                role_ids: provision::synced_roles(state, provider, identity, false).await?,
            });
        }
    };

    let admitted = provision::admit(state, provider, &user, identity).await?;

    Ok(Destination {
        account,
        email: user.email.clone(),
        member: admitted.membership.is_some(),
        role_ids: admitted.role_ids,
        user: Some(user),
    })
}

/// 目录登录只核对了密码：组织只允许单点登录时进不来；账号开了多因素认证或组织要求时还要第二步。
async fn directory_second_step(state: &AppState, org_id: &Id, user: Option<&User>) -> Result<bool> {
    let (admission, mfa_on) = match user {
        Some(user) => {
            let mfa_on = mfa::enabled(state, &user.id).await?;
            let method = mfa::after_first_step(mfa_on);

            (
                mfa::admission(state, org_id, &user.id, &method).await?,
                mfa_on,
            )
        }
        None => {
            let policy = state.iam.signin_policies.get(org_id).await?;
            let method = SignInMethod::single_factor();

            (
                signin_policy::admission(&policy, &method, org_id, false),
                false,
            )
        }
    };

    if admission == Admission::NeedsSso {
        return Err(Error::forbidden(DIRECTORY_BLOCKED));
    }

    Ok(mfa_on || admission == Admission::NeedsMfa)
}

async fn mapping_of(
    state: &AppState,
    provider: &SsoProvider,
    identity: &FederatedIdentity,
    tester: &Id,
    destination: Destination,
) -> Result<TestedMapping> {
    let Destination {
        account,
        user,
        email,
        member,
        role_ids,
    } = destination;

    let current_roles = match (&user, member) {
        (Some(user), true) => state
            .iam
            .service
            .iam_memberships
            .assigned_roles(&user.id, &provider.org_id)
            .await?
            .into_iter()
            .map(|role| TestedRole {
                id: role.id.0,
                name: role.name,
            })
            .collect(),
        _ => Vec::new(),
    };

    let roles = if role_ids.is_empty() {
        current_roles.clone()
    } else {
        named_roles(state, &provider.org_id, &role_ids).await?
    };

    // 不是本组织成员的账号，这个管理员本来看不到它是谁。
    let account_email = if member || user.is_none() {
        email
    } else {
        mask_email(&email)
    };

    let account_name = user
        .as_ref()
        .filter(|_| member)
        .map(|user| user.display_name.clone());

    let mapping = provider.config.group_role_mapping();

    Ok(TestedMapping {
        account,
        account_email,
        account_name,
        is_tester: user.as_ref().is_some_and(|user| user.id == *tester),
        member,
        current_roles,
        roles,
        role_source: role_source(
            &identity.groups,
            mapping,
            provider.config.default_role_id(),
            member,
        ),
        matched_groups: matched_groups(&identity.groups, mapping),
    })
}

/// 角色的名字。绑定里有组织已经没有的角色时，真登录写角色那一步会失败，这里同样算失败。
async fn named_roles(state: &AppState, org_id: &Id, role_ids: &[Id]) -> Result<Vec<TestedRole>> {
    state.iam.roles.ensure_builtin_roles(org_id).await?;

    let names = state
        .iam
        .roles
        .list(org_id)
        .await?
        .into_iter()
        .map(|role| (role.id.0, role.name))
        .collect::<HashMap<_, _>>();

    role_ids
        .iter()
        .map(|id| {
            let name = names
                .get(&id.0)
                .ok_or_else(|| Error::invalid(format!("{ROLE_MISSING}: {}", id.0)))?;

            Ok(TestedRole {
                id: id.0.clone(),
                name: name.clone(),
            })
        })
        .collect()
}

fn role_source(
    groups: &[String],
    mapping: &BTreeMap<String, String>,
    default_role: Option<&str>,
    member: bool,
) -> TestedRoleSource {
    if groups.iter().any(|group| mapping.contains_key(group)) {
        TestedRoleSource::GroupMapping
    } else if default_role.is_some() {
        TestedRoleSource::DefaultRole
    } else if member {
        TestedRoleSource::Unchanged
    } else {
        TestedRoleSource::Signup
    }
}

fn matched_groups(groups: &[String], mapping: &BTreeMap<String, String>) -> Vec<String> {
    groups
        .iter()
        .filter(|group| mapping.contains_key(*group))
        .cloned()
        .collect()
}

fn notes_of(
    provider: &SsoProvider,
    identity: &FederatedIdentity,
    mapping: &TestedMapping,
    second_factor: bool,
) -> Vec<String> {
    let mut notes = Vec::new();

    if !provider.config.group_role_mapping().is_empty() {
        if identity.groups.is_empty() {
            notes.push(note::NO_GROUPS.to_owned());
        } else if mapping.matched_groups.is_empty() {
            notes.push(note::GROUPS_UNMAPPED.to_owned());
        }
    }

    if !mapping.is_tester {
        notes.push(note::ANOTHER_ACCOUNT.to_owned());
    }

    if mapping.member && roles_differ(&mapping.current_roles, &mapping.roles) {
        notes.push(note::ROLES_CHANGE.to_owned());
    }

    if second_factor {
        notes.push(note::SECOND_FACTOR.to_owned());
    }

    notes
}

fn roles_differ(left: &[TestedRole], right: &[TestedRole]) -> bool {
    let ids = |roles: &[TestedRole]| {
        let mut ids = roles.iter().map(|role| role.id.clone()).collect::<Vec<_>>();
        ids.sort();
        ids
    };

    ids(left) != ids(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::iam::{SsoOidcConfig, SsoPlatform, SsoProviderConfig},
        shared::time::TimestampMicros,
    };

    fn provider_with(mapping: &[(&str, &str)]) -> SsoProvider {
        SsoProvider {
            id: Id::from_string("p-1"),
            org_id: Id::from_string("org-1"),
            name: "Okta".into(),
            kind: SsoProviderKind::Oidc,
            platform: SsoPlatform::Okta,
            enabled: true,
            config: SsoProviderConfig::Oidc(SsoOidcConfig {
                group_role_mapping: mapping
                    .iter()
                    .map(|(group, role)| ((*group).to_owned(), (*role).to_owned()))
                    .collect(),
                ..SsoOidcConfig::default()
            }),
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    fn identity(groups: &[&str]) -> FederatedIdentity {
        FederatedIdentity {
            email: "alice@example.com".into(),
            display_name: "Alice".into(),
            subject: "u-1".into(),
            groups: groups.iter().map(|group| (*group).to_owned()).collect(),
            email_verified: Some(true),
        }
    }

    fn role(id: &str) -> TestedRole {
        TestedRole {
            id: id.into(),
            name: id.into(),
        }
    }

    fn mapping(matched: &[&str], current: &[&str], after: &[&str]) -> TestedMapping {
        TestedMapping {
            account: TestedAccount::Bound,
            account_email: "alice@example.com".into(),
            account_name: Some("Alice".into()),
            is_tester: true,
            member: true,
            current_roles: current.iter().map(|id| role(id)).collect(),
            roles: after.iter().map(|id| role(id)).collect(),
            role_source: TestedRoleSource::GroupMapping,
            matched_groups: matched.iter().map(|group| (*group).to_owned()).collect(),
        }
    }

    #[test]
    fn roles_come_from_the_mapping_then_the_default_then_stay_or_sign_up() {
        let mapping = BTreeMap::from([("sre".to_owned(), "role-admin".to_owned())]);
        let sre = ["sre".to_owned()];
        let dev = ["dev".to_owned()];

        assert_eq!(
            role_source(&sre, &mapping, Some("role-viewer"), true),
            TestedRoleSource::GroupMapping
        );
        assert_eq!(
            role_source(&dev, &mapping, Some("role-viewer"), true),
            TestedRoleSource::DefaultRole
        );
        assert_eq!(
            role_source(&dev, &mapping, None, true),
            TestedRoleSource::Unchanged
        );
        assert_eq!(
            role_source(&dev, &mapping, None, false),
            TestedRoleSource::Signup
        );
    }

    #[test]
    fn only_groups_in_the_mapping_are_named() {
        let mapping = BTreeMap::from([("sre".to_owned(), "role-admin".to_owned())]);
        let groups = ["dev".to_owned(), "sre".to_owned()];

        assert_eq!(matched_groups(&groups, &mapping), ["sre"]);
    }

    #[test]
    fn a_clean_sign_in_has_nothing_to_say() {
        let provider = provider_with(&[("sre", "role-admin")]);
        let notes = notes_of(
            &provider,
            &identity(&["sre"]),
            &mapping(&["sre"], &["role-admin"], &["role-admin"]),
            false,
        );

        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn a_mapping_without_groups_to_read_is_pointed_out() {
        let provider = provider_with(&[("sre", "role-admin")]);

        let notes = notes_of(
            &provider,
            &identity(&[]),
            &mapping(&[], &["role-admin"], &["role-admin"]),
            false,
        );
        assert_eq!(notes, [note::NO_GROUPS]);

        let notes = notes_of(
            &provider,
            &identity(&["dev"]),
            &mapping(&[], &["role-admin"], &["role-admin"]),
            false,
        );
        assert_eq!(notes, [note::GROUPS_UNMAPPED]);

        // 没配映射时，没有用户组不算事。
        let notes = notes_of(
            &provider_with(&[]),
            &identity(&[]),
            &mapping(&[], &["role-admin"], &["role-admin"]),
            false,
        );
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn someone_elses_account_and_changing_roles_are_pointed_out() {
        let provider = provider_with(&[("sre", "role-viewer")]);
        let mut other = mapping(&["sre"], &["role-owner"], &["role-viewer"]);
        other.is_tester = false;

        let notes = notes_of(&provider, &identity(&["sre"]), &other, true);

        assert_eq!(
            notes,
            [
                note::ANOTHER_ACCOUNT,
                note::ROLES_CHANGE,
                note::SECOND_FACTOR
            ]
        );
    }

    #[test]
    fn a_new_member_has_no_roles_to_lose() {
        let provider = provider_with(&[("sre", "role-viewer")]);
        let mut new = mapping(&["sre"], &[], &["role-viewer"]);
        new.member = false;

        let notes = notes_of(&provider, &identity(&["sre"]), &new, false);

        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn the_order_of_roles_does_not_count_as_a_change() {
        assert!(!roles_differ(
            &[role("a"), role("b")],
            &[role("b"), role("a")]
        ));
        assert!(roles_differ(&[role("a")], &[role("a"), role("b")]));
    }
}
