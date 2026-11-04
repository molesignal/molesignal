// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织管理员能对哪些账号动手。
//!
//! 账号是整个实例共用的，一个账号可以同时是几个组织的成员。管理员只能看到、处理当前组织的
//! 成员；别的组织的账号一律按不存在回答，不透露它在不在。改名、换头像、停用、删除和重置
//! 多因素认证会影响账号所在的每个组织，所以只对只属于当前组织的账号开放；也在别的组织的
//! 账号，管理员只能把它从当前组织移除。平台管理员不受这一条限制。

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::IamMembership,
    shared::{Error, Result, ids::Id},
};

/// 一个账号和当前组织的关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// 不是当前组织的成员。
    Outside,
    /// 只属于当前组织。
    OnlyHere,
    /// 是当前组织的成员，也在别的组织。
    AlsoElsewhere,
}

fn reach(memberships: &[IamMembership], org_id: &Id) -> Reach {
    if !memberships
        .iter()
        .any(|membership| membership.org_id == *org_id)
    {
        Reach::Outside
    } else if memberships
        .iter()
        .all(|membership| membership.org_id == *org_id)
    {
        Reach::OnlyHere
    } else {
        Reach::AlsoElsewhere
    }
}

async fn reach_of(state: &AppState, ctx: &IamContext, target: &Id) -> Result<Reach> {
    let memberships = state
        .iam
        .service
        .iam_memberships
        .list_for_user(target)
        .await?;

    Ok(reach(&memberships, &ctx.org_id))
}

/// 目标是本人，或者是当前组织的成员。
pub(crate) async fn member_of_current_org(
    state: &AppState,
    ctx: &IamContext,
    target: &Id,
) -> Result<()> {
    if *target == ctx.user_id {
        return Ok(());
    }

    match reach_of(state, ctx, target).await? {
        Reach::Outside => Err(Error::not_found("organization user")),
        Reach::OnlyHere | Reach::AlsoElsewhere => Ok(()),
    }
}

/// 目标是本人，或者是只属于当前组织的成员；平台管理员可以处理当前组织的任何成员。
pub(crate) async fn account_of_current_org(
    state: &AppState,
    ctx: &IamContext,
    target: &Id,
) -> Result<()> {
    if *target == ctx.user_id {
        return Ok(());
    }

    match reach_of(state, ctx, target).await? {
        Reach::Outside => Err(Error::not_found("organization user")),
        Reach::OnlyHere => Ok(()),
        Reach::AlsoElsewhere => {
            if state
                .iam
                .platform_administrators
                .is_active(&ctx.user_id)
                .await?
            {
                Ok(())
            } else {
                Err(Error::forbidden(
                    "this account also belongs to other organizations; remove it from this organization instead",
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::time::TimestampMicros;

    fn membership(org_id: &str) -> IamMembership {
        IamMembership {
            user_id: Id::from_string("user-1"),
            org_id: Id::from_string(org_id),
            joined_at: TimestampMicros(0),
        }
    }

    #[test]
    fn an_account_without_a_membership_here_is_outside() {
        let here = Id::from_string("org-a");

        assert_eq!(reach(&[], &here), Reach::Outside);
        assert_eq!(reach(&[membership("org-b")], &here), Reach::Outside);
    }

    #[test]
    fn an_account_only_in_this_organization_belongs_here() {
        let here = Id::from_string("org-a");

        assert_eq!(reach(&[membership("org-a")], &here), Reach::OnlyHere);
    }

    #[test]
    fn an_account_shared_with_another_organization_is_also_elsewhere() {
        let here = Id::from_string("org-a");
        let memberships = [membership("org-a"), membership("org-b")];

        assert_eq!(reach(&memberships, &here), Reach::AlsoElsewhere);
    }
}
