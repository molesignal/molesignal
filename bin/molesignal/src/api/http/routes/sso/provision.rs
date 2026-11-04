// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 外部身份核验之后的本地账号。
//!
//! - 身份已绑定（`sso_identities`）：登录绑定的账号，不看身份源给的邮箱。
//! - 未绑定、邮箱已有账号：不自动关联，交给 [`super::linking`] 等本人确认。
//! - 未绑定、邮箱没有账号：新建无密码账号并绑定。公网身份源确认过的邮箱记为已证明；其余
//!   记为 created，账号只能留在该服务所在的组织（[`super::policy::ensure_membership_allowed`]）。
//!
//! 账号确定后由 [`finish`] 入组、按用户组同步角色，得出要登录的身份；会话在回答浏览器时
//! 写进 Cookie，等待交接期间服务端只存身份，不存 token。

use std::collections::BTreeMap;

use serde::Serialize;

use super::{linking, policy};
use crate::{
    api::{AppState, http::routes::auth::SessionView},
    domain::iam::{IamMembership, IamScope, Organization, SsoProvider, User},
    infra::sso::{ExternalIdentity, LinkedIdentity, SsoIdentity, SsoIdentityProof},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) struct FederatedIdentity {
    pub email: String,
    pub display_name: String,
    pub subject: String,
    pub groups: Vec<String>,
    /// 身份源对邮箱是否已验证的声明；没有声明时为 `None`。
    pub email_verified: Option<bool>,
}

impl From<ExternalIdentity> for FederatedIdentity {
    fn from(identity: ExternalIdentity) -> Self {
        Self {
            display_name: identity.name.unwrap_or_else(|| identity.email.clone()),
            email: identity.email,
            subject: identity.subject,
            groups: identity.groups,
            email_verified: identity.email_verified,
        }
    }
}

impl FederatedIdentity {
    /// 等本人确认期间存下的身份。
    pub(super) fn snapshot(&self) -> LinkedIdentity {
        LinkedIdentity {
            email: self.email.clone(),
            display_name: self.display_name.clone(),
            groups: self.groups.clone(),
            email_verified: self.email_verified,
        }
    }

    pub(super) fn from_snapshot(subject: String, identity: LinkedIdentity) -> Self {
        Self {
            email: identity.email,
            display_name: identity.display_name,
            subject,
            groups: identity.groups,
            email_verified: identity.email_verified,
        }
    }
}

/// 一次单点登录的结果，序列化为 `{"login": …}` 或 `{"link": …}`。
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SignIn {
    /// 账号已确定，回答时登录（写 Cookie）。
    Login(SessionView),
    /// 邮箱已有账号，等本人确认后才关联。
    Link(linking::LinkRequired),
}

/// 取回调要用的服务：已删除时按凭据失效处理，停用时拒绝。
pub(super) async fn enabled_provider(state: &AppState, provider_id: &str) -> Result<SsoProvider> {
    let provider = state
        .iam
        .sso_providers
        .get(&Id::from_string(provider_id.to_owned()))
        .await
        .map_err(|_| Error::unauthorized("sso provider not found"))?;

    if !provider.enabled {
        return Err(Error::forbidden(format!(
            "provider {} is disabled",
            provider.name
        )));
    }

    Ok(provider)
}

/// 身份对应的本地账号。登录据此登录、关联或新建，测试登录只说明它。
pub(super) enum AccountMatch {
    /// 身份已绑定账号。
    Bound { binding: Id, user: User },
    /// 身份未绑定，邮箱已有账号：本人确认后才关联。
    Existing(User),
    /// 身份未绑定，邮箱没有账号：会新建。`email` 已规范化。
    New { email: String },
}

/// 按绑定找账号，没有绑定时看邮箱。只读。
pub(super) async fn match_account(
    state: &AppState,
    provider: &SsoProvider,
    identity: &FederatedIdentity,
) -> Result<AccountMatch> {
    let bound = state
        .iam
        .sso_identities
        .find(&provider.id, &identity.subject)
        .await?;

    if let Some(bound) = bound {
        let user = state.iam.service.users.get(&bound.user_id).await?;

        return Ok(AccountMatch::Bound {
            binding: bound.id,
            user,
        });
    }

    let email = identity.email.trim().to_ascii_lowercase();

    if email.is_empty() || !email.contains('@') {
        return Err(Error::unauthorized(
            "external identity does not contain a valid email address",
        ));
    }

    match state.iam.service.users.get_by_email(&email).await {
        Ok(user) => Ok(AccountMatch::Existing(user)),
        Err(Error::NotFound(_)) => Ok(AccountMatch::New { email }),
        Err(error) => Err(error),
    }
}

pub(super) async fn sign_in(
    state: &AppState,
    provider: &SsoProvider,
    identity: FederatedIdentity,
) -> Result<SignIn> {
    policy::ensure_identity_allowed(state, provider, &identity).await?;

    match match_account(state, provider, &identity).await? {
        AccountMatch::Bound { binding, user } => {
            let login = finish(state, provider, user, &identity).await?;

            state
                .iam
                .sso_identities
                .touch(&binding, TimestampMicros::now())
                .await?;

            Ok(SignIn::Login(login))
        }
        AccountMatch::Existing(user) => Ok(SignIn::Link(
            linking::request(state, provider, &user, &identity).await?,
        )),
        AccountMatch::New { email } => {
            super::super::email_domains::ensure_email_allowed(state, &provider.org_id, &email)
                .await?;

            let display_name = match identity.display_name.trim() {
                "" => email.clone(),
                name => name.to_owned(),
            };

            let user = state
                .iam
                .service
                .create_external_user(email, display_name)
                .await?;

            // 公网身份源只放行它确认过的邮箱（见 `policy`），这类邮箱算已证明。
            let proof = if policy::accepts_public_accounts(provider) {
                SsoIdentityProof::VerifiedEmail
            } else {
                SsoIdentityProof::Created
            };

            state
                .iam
                .sso_identities
                .create(SsoIdentity::new(
                    provider.id.clone(),
                    identity.subject.clone(),
                    user.id.clone(),
                    proof,
                ))
                .await?;

            Ok(SignIn::Login(
                finish(state, provider, user, &identity).await?,
            ))
        }
    }
}

/// 账号能不能进这个组织、会得到哪些角色。
pub(super) struct Admitted {
    pub organization: Organization,
    pub membership: Option<IamMembership>,
    /// 要同步成的角色；空的时候不动。
    pub role_ids: Vec<Id>,
}

/// 账号已确定：检查能否登录，得出要同步的角色。只读。
pub(super) async fn admit(
    state: &AppState,
    provider: &SsoProvider,
    user: &User,
    identity: &FederatedIdentity,
) -> Result<Admitted> {
    let organization = state.iam.service.orgs.get(&provider.org_id).await?;
    organization.ensure_enabled()?;
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
        super::super::email_domains::ensure_email_allowed(state, &provider.org_id, &user.email)
            .await?;

        policy::ensure_membership_allowed(state, &user.id, &provider.org_id).await?;
    }

    let role_ids = synced_roles(state, provider, identity, membership.is_some()).await?;

    Ok(Admitted {
        organization,
        membership,
        role_ids,
    })
}

/// 这个身份要同步成的角色：用户组映射到的，否则默认角色。这两样都没有时，新成员得到自助注册的
/// 角色，老成员不动。
pub(super) async fn synced_roles(
    state: &AppState,
    provider: &SsoProvider,
    identity: &FederatedIdentity,
    member: bool,
) -> Result<Vec<Id>> {
    let mut role_ids = map_role_ids(
        &identity.groups,
        provider.config.group_role_mapping(),
        provider.config.default_role_id(),
    );

    if !member && role_ids.is_empty() {
        role_ids.push(
            state
                .iam
                .service
                .iam_memberships
                .role_id_for_purpose(&provider.org_id, "self_service_signup")
                .await?,
        );
    }

    Ok(role_ids)
}

/// 账号已确定：检查能否登录，入组，按用户组同步角色，得出要登录的身份。
pub(super) async fn finish(
    state: &AppState,
    provider: &SsoProvider,
    user: User,
    identity: &FederatedIdentity,
) -> Result<SessionView> {
    let Admitted {
        organization,
        membership,
        role_ids,
    } = admit(state, provider, &user, identity).await?;

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

    let roles = state
        .iam
        .service
        .iam_memberships
        .assigned_roles(&user.id, &provider.org_id)
        .await?;

    Ok(SessionView::new(
        user,
        organization,
        roles,
        IamScope::Organization,
    ))
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{FederatedIdentity, map_role_ids};

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

    #[test]
    fn a_waiting_identity_comes_back_unchanged() {
        let identity = FederatedIdentity {
            email: "alice@example.com".into(),
            display_name: "Alice".into(),
            subject: "u-1".into(),
            groups: vec!["sre".into()],
            email_verified: Some(true),
        };

        let back = FederatedIdentity::from_snapshot("u-1".into(), identity.snapshot());
        assert_eq!(back.email, "alice@example.com");
        assert_eq!(back.groups, ["sre"]);
        assert_eq!(back.email_verified, Some(true));
    }
}
