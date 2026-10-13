// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 巡检以谁的身份执行：最后一次保存它的登录用户。
//!
//! 谁最后决定了「查什么、通知谁」，巡检就用谁的权限读日志。每次执行都重新核对这位用户：
//! 账号还能登录、还在组织里、还能使用 Mole Agent 并查询数据流。任何一条不成立，这次执行
//! 就失败并写明原因；有 `inspections.manage` 的人重新保存即可接管。

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    app::iam::{IamAccessService, IamContext, IamService},
    domain::iam::IamScope,
    shared::{Error, Result, ids::Id},
};

/// 执行一次巡检需要的权限：用 Mole Agent，查询数据流。
pub const RUN_PERMISSIONS: [&str; 2] = ["agent.use", "streams.query"];

/// 保存巡检的人能不能成为它的执行身份。
///
/// API Token 只代表它绑定的那个角色，以签发者本人的身份去读日志会越过这个角色，所以用
/// API Token 保存的请求没有可用身份，直接拒绝。
pub fn run_as_user(saver: &IamContext) -> Result<Id> {
    if saver.scope != IamScope::Organization {
        return Err(Error::forbidden(
            "a scheduled log inspection runs as the organization member who saves it: save it \
             while signed in to the organization, not with an API token",
        ));
    }

    Ok(saver.user_id.clone())
}

/// 保存者手里缺的那项执行权限。
pub fn missing_run_permission(context: &IamContext) -> Option<&'static str> {
    RUN_PERMISSIONS
        .into_iter()
        .find(|permission| !context.has_permission(permission))
}

/// 一位用户此刻在组织里的身份。
#[async_trait]
pub trait RunIdentity: Send + Sync {
    /// 角色和权限都已解析的身份。这位用户用不了时返回 [`Error::Forbidden`]，文本说明是哪一种；
    /// 其它错误是 IAM 自己出了故障，与这位用户无关。
    async fn resolve(&self, org_id: &Id, user_id: &Id) -> Result<IamContext>;
}

/// 生产实现：读 IAM 的用户、成员关系和授权。
pub struct IamRunIdentity {
    iam: Arc<IamService>,
    access: Arc<IamAccessService>,
}

impl IamRunIdentity {
    pub fn new(iam: Arc<IamService>, access: Arc<IamAccessService>) -> Self {
        Self { iam, access }
    }
}

#[async_trait]
impl RunIdentity for IamRunIdentity {
    async fn resolve(&self, org_id: &Id, user_id: &Id) -> Result<IamContext> {
        let user = match self.iam.current_user(user_id).await {
            Ok(user) => user,
            Err(Error::NotFound(_)) => {
                return Err(unusable(&format!("user {}", user_id.0), "no longer exists"));
            }
            Err(error) => return Err(error),
        };

        // 与请求进来时的鉴权同一套核对：账号状态、组织状态，再解析成员关系和权限。
        let admitted = async {
            self.iam.ensure_user_access(user_id).await?;
            self.iam.ensure_organization_access(org_id).await
        };

        if let Err(error) = admitted.await {
            return Err(match error {
                Error::Unauthorized(reason) | Error::Forbidden(reason) => unusable(
                    &user.email,
                    &format!("can no longer sign in to this organization ({reason})"),
                ),
                other => other,
            });
        }

        let mut context = IamContext {
            user_id: user_id.clone(),
            org_id: org_id.clone(),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: Default::default(),
            policy_version: 0,
        };

        if let Err(error) = self.access.enrich_context(&mut context).await {
            return Err(match error {
                Error::Unauthorized(reason) | Error::Forbidden(reason) => unusable(
                    &user.email,
                    &format!("is no longer a member of this organization ({reason})"),
                ),
                other => other,
            });
        }

        if let Some(permission) = missing_run_permission(&context) {
            return Err(unusable(
                &user.email,
                &format!("no longer holds {permission}"),
            ));
        }

        Ok(context)
    }
}

/// 巡检记着的那位用户这次用不了。
fn unusable(who: &str, why: &str) -> Error {
    Error::forbidden(format!(
        "the inspection runs as {who}, who {why}. Save the inspection again to run it as another \
         user"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(scope: IamScope, permissions: &[&str]) -> IamContext {
        IamContext {
            user_id: Id::from_string("user-1"),
            org_id: Id::from_string("org-1"),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope,
            permissions: permissions.iter().map(|key| (*key).to_string()).collect(),
            policy_version: 0,
        }
    }

    #[test]
    fn a_signed_in_user_becomes_the_identity_of_what_they_save() {
        let saver = context(IamScope::Organization, &RUN_PERMISSIONS);

        assert_eq!(run_as_user(&saver).unwrap(), Id::from_string("user-1"));
    }

    #[test]
    fn an_api_token_cannot_save_an_inspection() {
        let token = context(IamScope::ApiToken, &RUN_PERMISSIONS);

        assert!(matches!(run_as_user(&token), Err(Error::Forbidden(_))));
    }

    #[test]
    fn the_first_missing_run_permission_is_named() {
        assert_eq!(
            missing_run_permission(&context(IamScope::Organization, &["agent.use"])),
            Some("streams.query")
        );
        assert_eq!(
            missing_run_permission(&context(IamScope::Organization, &[])),
            Some("agent.use")
        );
        assert_eq!(
            missing_run_permission(&context(IamScope::Organization, &RUN_PERMISSIONS)),
            None
        );
    }
}
