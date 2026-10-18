// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织 / 团队 / 个人三级作用域：谁能改一项 Agent 资源，谁能在列表里看到它，一个会话里能
//! 用到哪些资源。Skill、MCP 服务器以及之后的知识、记忆、自动化都用这一套判定，规则只写在
//! 这里。
//!
//! - 组织资源由 `agent.manage` 持有人管理，所有人可用。
//! - 团队资源由团队成员和 `agent.manage` 持有人管理（管理员不必先入队）。
//! - 个人资源只有本人能改；管理员能看到并停用、删除，看不到其中的凭据。
//! - 团队会话只加载组织资源与本队资源，任何人的个人资源都不进团队会话；个人会话加载组织资源、
//!   本人的个人资源，以及已经挂进来的团队的资源。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result, ids::Id};

/// 一项 Agent 资源属于谁。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum Scope {
    Org,
    Team { team_id: Id },
    Personal { owner_id: Id },
}

impl Scope {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Org => "org",
            Self::Team { .. } => "team",
            Self::Personal { .. } => "personal",
        }
    }

    pub fn team_id(&self) -> Option<&Id> {
        match self {
            Self::Team { team_id } => Some(team_id),
            _ => None,
        }
    }

    pub fn owner_id(&self) -> Option<&Id> {
        match self {
            Self::Personal { owner_id } => Some(owner_id),
            _ => None,
        }
    }

    /// 同一作用域里名称唯一时用的键：`org`、`team:<id>`、`personal:<id>`。
    pub fn key(&self) -> String {
        match self {
            Self::Org => "org".into(),
            Self::Team { team_id } => format!("team:{}", team_id.0),
            Self::Personal { owner_id } => format!("personal:{}", owner_id.0),
        }
    }

    /// 存储里的三列还原成作用域。
    pub fn from_parts(kind: &str, team_id: Option<&str>, owner_id: Option<&str>) -> Result<Self> {
        fn present(value: Option<&str>) -> Option<&str> {
            value.map(str::trim).filter(|value| !value.is_empty())
        }

        match kind {
            "org" => Ok(Self::Org),
            "team" => present(team_id)
                .map(|id| Self::Team {
                    team_id: Id::from_string(id),
                })
                .ok_or_else(|| Error::invalid("a team scope needs team_id")),
            "personal" => present(owner_id)
                .map(|id| Self::Personal {
                    owner_id: Id::from_string(id),
                })
                .ok_or_else(|| Error::invalid("a personal scope needs its owner")),
            other => Err(Error::invalid(format!("unknown scope `{other}`"))),
        }
    }
}

/// 发起请求的人：所在的团队，是否持有 `agent.manage`。
#[derive(Debug, Clone, Default)]
pub struct Viewer {
    pub user_id: Id,
    pub manager: bool,
    pub teams: HashSet<Id>,
}

impl Viewer {
    pub fn is_member(&self, team_id: &Id) -> bool {
        self.teams.contains(team_id)
    }

    /// 能在这一作用域新建、修改、移入资源。
    pub fn can_edit(&self, scope: &Scope) -> bool {
        match scope {
            Scope::Org => self.manager,
            Scope::Team { team_id } => self.manager || self.is_member(team_id),
            Scope::Personal { owner_id } => *owner_id == self.user_id,
        }
    }

    /// 能在列表里看到。
    pub fn can_see(&self, scope: &Scope) -> bool {
        match scope {
            Scope::Org => true,
            Scope::Team { team_id } => self.manager || self.is_member(team_id),
            Scope::Personal { owner_id } => self.manager || *owner_id == self.user_id,
        }
    }

    /// 能停用或删除：能改的人，以及管理员。
    pub fn can_moderate(&self, scope: &Scope) -> bool {
        self.manager || self.can_edit(scope)
    }
}

/// 一个会话属于谁。新会话在第一条消息前选定，之后不再改变。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatScope {
    /// 个人会话：发起人，以及已经挂进来的团队。
    Personal {
        owner_id: Id,
        mounted_teams: Vec<Id>,
    },
    /// 团队会话：团队成员都能读、能续聊。
    Team { team_id: Id },
}

impl ChatScope {
    /// 这一作用域的资源在会话里能用。
    pub fn loads(&self, scope: &Scope) -> bool {
        match (self, scope) {
            (_, Scope::Org) => true,
            (Self::Team { team_id }, Scope::Team { team_id: owner }) => team_id == owner,
            (Self::Team { .. }, Scope::Personal { .. }) => false,
            (Self::Personal { mounted_teams, .. }, Scope::Team { team_id }) => {
                mounted_teams.contains(team_id)
            }
            (Self::Personal { owner_id, .. }, Scope::Personal { owner_id: owner }) => {
                owner_id == owner
            }
        }
    }

    pub fn team_id(&self) -> Option<&Id> {
        match self {
            Self::Team { team_id } => Some(team_id),
            Self::Personal { .. } => None,
        }
    }
}

/// 一个会话对一个人开放到什么程度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAccess {
    None,
    /// 只看标题、用量等元数据，可以删除：管理员对别人的个人会话。
    Metadata,
    /// 能读、能续聊。
    Full,
}

impl ChatAccess {
    pub fn can_read(self) -> bool {
        self == Self::Full
    }
}

/// `owner_id` 是发起会话的人。
pub fn chat_access(viewer: &Viewer, chat: &ChatScope, owner_id: &Id) -> ChatAccess {
    match chat {
        ChatScope::Personal { .. } if *owner_id == viewer.user_id => ChatAccess::Full,
        ChatScope::Personal { .. } if viewer.manager => ChatAccess::Metadata,
        ChatScope::Personal { .. } => ChatAccess::None,
        ChatScope::Team { team_id } if viewer.manager || viewer.is_member(team_id) => {
            ChatAccess::Full
        }
        ChatScope::Team { .. } => ChatAccess::None,
    }
}

/// 能删除会话：个人会话的本人，团队会话的发起人，以及管理员。
pub fn can_delete_chat(viewer: &Viewer, chat: &ChatScope, owner_id: &Id) -> bool {
    viewer.manager
        || (*owner_id == viewer.user_id && chat_access(viewer, chat, owner_id) == ChatAccess::Full)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> Id {
        Id::from_string(value)
    }

    fn viewer(user: &str, manager: bool, teams: &[&str]) -> Viewer {
        Viewer {
            user_id: id(user),
            manager,
            teams: teams.iter().map(|team| id(team)).collect(),
        }
    }

    fn team(value: &str) -> Scope {
        Scope::Team { team_id: id(value) }
    }

    fn personal(value: &str) -> Scope {
        Scope::Personal {
            owner_id: id(value),
        }
    }

    #[test]
    fn who_may_change_a_resource() {
        let admin = viewer("admin", true, &[]);
        let member = viewer("ann", false, &["sre"]);
        let outsider = viewer("bob", false, &[]);

        assert!(admin.can_edit(&Scope::Org));
        assert!(!member.can_edit(&Scope::Org));

        // 管理员不必先入队。
        assert!(admin.can_edit(&team("sre")));
        assert!(member.can_edit(&team("sre")));
        assert!(!outsider.can_edit(&team("sre")));

        assert!(member.can_edit(&personal("ann")));
        assert!(!admin.can_edit(&personal("ann")));
        assert!(admin.can_moderate(&personal("ann")));
        assert!(!outsider.can_moderate(&personal("ann")));
    }

    #[test]
    fn who_sees_a_resource_in_lists() {
        let outsider = viewer("bob", false, &[]);

        assert!(outsider.can_see(&Scope::Org));
        assert!(!outsider.can_see(&team("sre")));
        assert!(!outsider.can_see(&personal("ann")));
        assert!(viewer("admin", true, &[]).can_see(&personal("ann")));
    }

    #[test]
    fn a_team_chat_never_loads_anyones_personal_resources() {
        let chat = ChatScope::Team { team_id: id("sre") };

        assert!(chat.loads(&Scope::Org));
        assert!(chat.loads(&team("sre")));
        assert!(!chat.loads(&team("dba")));
        assert!(!chat.loads(&personal("ann")));
    }

    #[test]
    fn a_personal_chat_loads_its_owner_and_the_teams_mounted_into_it() {
        let chat = ChatScope::Personal {
            owner_id: id("ann"),
            mounted_teams: vec![id("sre")],
        };

        assert!(chat.loads(&Scope::Org));
        assert!(chat.loads(&personal("ann")));
        assert!(!chat.loads(&personal("bob")));
        assert!(chat.loads(&team("sre")));
        assert!(!chat.loads(&team("dba")));
    }

    #[test]
    fn chats_open_to_their_people() {
        let personal_chat = ChatScope::Personal {
            owner_id: id("ann"),
            mounted_teams: Vec::new(),
        };
        let team_chat = ChatScope::Team { team_id: id("sre") };
        let ann = id("ann");

        assert_eq!(
            chat_access(&viewer("ann", false, &[]), &personal_chat, &ann),
            ChatAccess::Full
        );
        assert_eq!(
            chat_access(&viewer("admin", true, &[]), &personal_chat, &ann),
            ChatAccess::Metadata
        );
        assert_eq!(
            chat_access(&viewer("bob", false, &["sre"]), &personal_chat, &ann),
            ChatAccess::None
        );

        assert_eq!(
            chat_access(&viewer("bob", false, &["sre"]), &team_chat, &ann),
            ChatAccess::Full
        );
        assert_eq!(
            chat_access(&viewer("carl", false, &[]), &team_chat, &ann),
            ChatAccess::None
        );

        // 团队会话由发起人或管理员删除，其他成员只能读和续聊。
        assert!(can_delete_chat(
            &viewer("ann", false, &["sre"]),
            &team_chat,
            &ann
        ));
        assert!(!can_delete_chat(
            &viewer("bob", false, &["sre"]),
            &team_chat,
            &ann
        ));
        assert!(can_delete_chat(
            &viewer("admin", true, &[]),
            &personal_chat,
            &ann
        ));
    }

    #[test]
    fn a_scope_is_stored_as_three_columns() {
        assert_eq!(Scope::from_parts("org", None, None).unwrap(), Scope::Org);
        assert_eq!(
            Scope::from_parts("team", Some("sre"), None).unwrap(),
            team("sre")
        );
        assert_eq!(
            Scope::from_parts("personal", None, Some("ann")).unwrap(),
            personal("ann")
        );
        assert!(Scope::from_parts("team", Some(" "), None).is_err());
        assert!(Scope::from_parts("everyone", None, None).is_err());
        assert_eq!(team("sre").key(), "team:sre");
    }
}
