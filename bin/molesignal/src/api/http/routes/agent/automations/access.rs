// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 谁能看到、谁能改一条自动化。
//!
//! - 看到：`agent.use` 加作用域。组织的人人可见；团队的给成员与 `agent.manage` 持有人；个人的
//!   给本人与 `agent.manage` 持有人。
//! - 改（保存、启用、手动运行、令牌）：`agent.automations.manage` 加作用域。组织的有这项权限就
//!   能改；团队的要是成员或 `agent.manage` 持有人；个人的只有本人。
//! - 停用、删除：能改的人，以及 `agent.manage` 持有人。

use crate::{
    agent::{
        automations::Automation,
        scope::{Scope, Viewer},
    },
    api::{AppState, http::routes::agent::viewer::viewer},
    app::iam::IamContext,
    domain::iam::IamScope,
    shared::{Error, Result, ids::Id},
};

pub(super) const MANAGE: &str = "agent.automations.manage";

/// 这个人能在这一作用域里建、改规则。
pub(super) fn can_edit(ctx: &IamContext, viewer: &Viewer, scope: &Scope) -> bool {
    ctx.has_permission(MANAGE)
        && match scope {
            Scope::Org => true,
            Scope::Team { team_id } => viewer.manager || viewer.is_member(team_id),
            Scope::Personal { owner_id } => *owner_id == viewer.user_id,
        }
}

/// 能停用、删除：能改的人，以及管理员。
pub(super) fn can_moderate(ctx: &IamContext, viewer: &Viewer, scope: &Scope) -> bool {
    viewer.manager || can_edit(ctx, viewer, scope)
}

/// 一条看得到的规则，连同读的人；看不到的与不存在一样回 404。
pub(super) async fn visible(
    state: &AppState,
    ctx: &IamContext,
    id: &Id,
) -> Result<(Automation, Viewer)> {
    let viewer = viewer(state, ctx).await?;
    let automation = state.agent.automations.get(&ctx.org_id, id).await?;

    if !viewer.can_see(&automation.scope) {
        return Err(Error::not_found(format!("automation `{}` not found", id.0)));
    }

    Ok((automation, viewer))
}

/// 一条能改的规则。
pub(super) async fn editable(
    state: &AppState,
    ctx: &IamContext,
    id: &Id,
) -> Result<(Automation, Viewer)> {
    let (automation, viewer) = visible(state, ctx, id).await?;

    if !can_edit(ctx, &viewer, &automation.scope) {
        return Err(Error::forbidden(
            "only the people who manage automations in this scope can change it",
        ));
    }

    Ok((automation, viewer))
}

/// 保存规则的人成为它的执行身份。API Token 只代表它绑定的角色，以签发者本人的身份运行会越过
/// 这个角色，所以用 API Token 保存的请求没有可用身份。
pub(super) fn run_as(ctx: &IamContext) -> Result<Id> {
    if ctx.scope != IamScope::Organization {
        return Err(Error::forbidden(
            "an automation runs as the organization member who saves it: save it while signed in \
             to the organization, not with an API token",
        ));
    }

    Ok(ctx.user_id.clone())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::agent::runs::RunPrincipal;

    fn ctx(user: &str, permissions: &[&str]) -> IamContext {
        let mut context = RunPrincipal {
            user_id: Id::from_string(user),
            scope: IamScope::Organization,
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
        }
        .unresolved_context(&Id::from_string("org-1"));

        context.permissions = permissions.iter().map(|p| p.to_string()).collect();
        context
    }

    fn viewer(user: &str, manager: bool, teams: &[&str]) -> Viewer {
        Viewer {
            user_id: Id::from_string(user),
            manager,
            teams: teams
                .iter()
                .map(|team| Id::from_string(*team))
                .collect::<HashSet<_>>(),
        }
    }

    #[test]
    fn editing_needs_the_permission_and_the_scope() {
        let team = Scope::Team {
            team_id: Id::from_string("sre"),
        };
        let mine = Scope::Personal {
            owner_id: Id::from_string("ann"),
        };

        let editor = ctx("ann", &[MANAGE]);
        assert!(can_edit(&editor, &viewer("ann", false, &[]), &Scope::Org));
        assert!(can_edit(&editor, &viewer("ann", false, &["sre"]), &team));
        assert!(!can_edit(&editor, &viewer("ann", false, &[]), &team));
        assert!(can_edit(&editor, &viewer("ann", false, &[]), &mine));

        let viewer_only = ctx("ann", &["agent.use"]);
        assert!(!can_edit(
            &viewer_only,
            &viewer("ann", false, &["sre"]),
            &team
        ));

        // 管理员能停用别人的个人规则，但不能改它。
        let admin = ctx("bob", &[MANAGE, "agent.manage"]);
        assert!(!can_edit(&admin, &viewer("bob", true, &[]), &mine));
        assert!(can_moderate(&admin, &viewer("bob", true, &[]), &mine));
    }
}
