// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织里的人彼此看得到名字。
//!
//! 事件的负责人、值班的人和升级步骤要选的人都按名字显示，处理它们的角色却不一定有
//! `org.members.read`：editor 能管告警和排班，这个权限只有 owner 和 admin 有。所以登录的成员读
//! `GET /users` 至少拿到每个成员的名字和头像；邮箱、角色、团队、登录方式和活跃时间仍然要这个权限。
//!
//! 只有登录会话有这个待遇。API token 没有权限时照旧拒绝：公开的 RUM token 不能列出组织里的人。

use serde::Serialize;

use crate::{
    app::iam::IamContext,
    domain::iam::{IamScope, User, UserStatus},
};

/// 组织里每个人都能看到的一个成员。
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MemberName {
    id: String,
    display_name: String,
    avatar_url: Option<String>,
}

/// 这个调用者读成员列表时只拿名字：登录在组织里，又没有 `org.members.read`。
pub(super) fn sees_names_only(ctx: &IamContext) -> bool {
    ctx.scope == IamScope::Organization && !ctx.has_permission("org.members.read")
}

/// 按名字排好的成员。还没通过审批或被拒绝的账号不算：只有管理员该知道它们。停用的账号留着，
/// 它经手过的事件还要显示它的名字。
pub(super) fn member_names(users: Vec<User>) -> Vec<MemberName> {
    let mut names = users
        .into_iter()
        .filter(|user| user.status == UserStatus::Active)
        .map(|user| MemberName {
            display_name: name_of(&user),
            id: user.id.0,
            avatar_url: user.avatar_url,
        })
        .collect::<Vec<_>>();

    names.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });

    names
}

/// 没填名字的账号用邮箱 `@` 前面的部分，不把整个地址给没有权限的人看。
fn name_of(user: &User) -> String {
    let name = user.display_name.trim();

    if name.is_empty() {
        user.email.split('@').next().unwrap_or_default().to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::{ids::Id, time::TimestampMicros};

    fn context(scope: IamScope, permissions: &[&str]) -> IamContext {
        IamContext {
            user_id: Id::from_string("user-1"),
            org_id: Id::from_string("org"),
            display_role: "Editor".into(),
            roles: Vec::new(),
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope,
            permissions: permissions
                .iter()
                .map(|permission| (*permission).to_string())
                .collect(),
            policy_version: 1,
        }
    }

    fn user(id: &str, display_name: &str, status: UserStatus) -> User {
        User {
            id: Id::from_string(id),
            email: format!("{id}@example.com"),
            display_name: display_name.into(),
            avatar_url: None,
            bio: String::new(),
            password_hash: "hash".into(),
            disabled: false,
            status,
            created_at: TimestampMicros(0),
            sessions_valid_after: TimestampMicros(0),
        }
    }

    #[test]
    fn a_signed_in_member_without_the_permission_sees_names_only() {
        assert!(sees_names_only(&context(
            IamScope::Organization,
            &["alerts.rules.manage"]
        )));
    }

    #[test]
    fn whoever_holds_the_permission_sees_everything() {
        assert!(!sees_names_only(&context(
            IamScope::Organization,
            &["org.members.read"]
        )));
    }

    #[test]
    fn neither_a_token_nor_the_platform_organization_gets_names_for_free() {
        assert!(!sees_names_only(&context(IamScope::ApiToken, &[])));
        assert!(!sees_names_only(&context(IamScope::System, &[])));
    }

    #[test]
    fn names_are_sorted_and_leave_out_who_is_not_approved_yet() {
        let names = member_names(vec![
            user("zoe", "zoe", UserStatus::Active),
            user("applicant", "Applicant", UserStatus::Pending),
            user("refused", "Refused", UserStatus::Rejected),
            user("amara", "Amara", UserStatus::Active),
        ]);

        let ids = names
            .iter()
            .map(|name| name.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, ["amara", "zoe"]);
    }

    #[test]
    fn a_disabled_member_keeps_a_name() {
        let mut gone = user("gone", "Gone", UserStatus::Active);
        gone.disabled = true;

        assert_eq!(member_names(vec![gone]).len(), 1);
    }

    #[test]
    fn someone_without_a_name_is_not_shown_by_their_whole_address() {
        let names = member_names(vec![user("dana", "  ", UserStatus::Active)]);

        assert_eq!(names[0].display_name, "dana");
    }

    #[test]
    fn nothing_but_the_name_and_the_picture_leaves() {
        let names = member_names(vec![user("dana", "Dana Reyes", UserStatus::Active)]);

        assert_eq!(
            serde_json::to_value(&names[0]).expect("serialize a member name"),
            json!({ "id": "dana", "displayName": "Dana Reyes", "avatarUrl": null })
        );
    }
}
