// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Organization, team, and personal scopes determine who can edit an Agent resource, who can see it
//! in lists, and which resources a session can use. Skills, MCP servers, and future knowledge,
//! memory, and automation all use these checks; the rules are defined only here.
//!
//! - Organization resources are managed by `agent.manage` holders and available to everyone.
//! - Team resources are managed by team members and `agent.manage` holders (administrators do not
//!   need to join the team first).
//! - Personal resources can only be edited by their owner; administrators can see, disable, and
//!   delete them, but cannot see their credentials.
//! - Team sessions load only organization resources and their own team's resources; no one's
//!   personal resources enter team sessions. Personal sessions load organization resources, the
//!   owner's personal resources, and resources from teams already attached to the session.
//!   Organization sessions (organization-scoped automation) load only organization resources.

use std::collections::HashSet;

use kernel::{Error, Result, ids::Id};
use serde::{Deserialize, Serialize};

/// Who owns an Agent resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "scope",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
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

    /// The key used for names unique within a scope: `org`, `team:<id>`, `personal:<id>`.
    pub fn key(&self) -> String {
        match self {
            Self::Org => "org".into(),
            Self::Team { team_id } => format!("team:{}", team_id.0),
            Self::Personal { owner_id } => format!("personal:{}", owner_id.0),
        }
    }

    /// Restore a scope from the three stored columns.
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

/// The requester: their teams and whether they hold `agent.manage`.
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

    /// Can create, edit, and move resources into this scope.
    pub fn can_edit(&self, scope: &Scope) -> bool {
        match scope {
            Scope::Org => self.manager,
            Scope::Team { team_id } => self.manager || self.is_member(team_id),
            Scope::Personal { owner_id } => *owner_id == self.user_id,
        }
    }

    /// Can see it in lists.
    pub fn can_see(&self, scope: &Scope) -> bool {
        match scope {
            Scope::Org => true,
            Scope::Team { team_id } => self.manager || self.is_member(team_id),
            Scope::Personal { owner_id } => self.manager || *owner_id == self.user_id,
        }
    }

    /// Can disable or delete: anyone who can edit, and administrators.
    pub fn can_moderate(&self, scope: &Scope) -> bool {
        self.manager || self.can_edit(scope)
    }
}

/// Who owns a session.
/// Chosen before the first message in a new session and never changed afterward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatScope {
    /// A personal session: the initiator and teams already attached to it.
    Personal {
        owner_id: Id,
        mounted_teams: Vec<Id>,
    },
    /// A team session: all team members can read and continue the conversation.
    Team { team_id: Id },
    /// An organization session, used for each organization-scoped automation run, loads only
    /// organization resources and has no memory.
    Org,
}

impl ChatScope {
    /// Resources in this scope are available in the session.
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
            (Self::Org, _) => false,
        }
    }

    pub fn team_id(&self) -> Option<&Id> {
        match self {
            Self::Team { team_id } => Some(team_id),
            Self::Personal { .. } | Self::Org => None,
        }
    }

    /// Where session memory is written: to the team for team sessions, or to the initiator for
    /// personal sessions; organization sessions have no memory.
    pub fn memory_scope(&self) -> Option<Scope> {
        match self {
            Self::Team { team_id } => Some(Scope::Team {
                team_id: team_id.clone(),
            }),
            Self::Personal { owner_id, .. } => Some(Scope::Personal {
                owner_id: owner_id.clone(),
            }),
            Self::Org => None,
        }
    }
}

/// The level of access a person has to a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAccess {
    None,
    /// Can only view metadata such as the title and usage, and can delete: administrators accessing
    /// other people's personal sessions.
    Metadata,
    /// Can read and continue the conversation.
    Full,
}

impl ChatAccess {
    pub fn can_read(self) -> bool {
        self == Self::Full
    }
}

/// `owner_id` is the person who initiates the session.
pub fn chat_access(viewer: &Viewer, chat: &ChatScope, owner_id: &Id) -> ChatAccess {
    match chat {
        ChatScope::Personal { .. } if *owner_id == viewer.user_id => ChatAccess::Full,
        ChatScope::Personal { .. } if viewer.manager => ChatAccess::Metadata,
        ChatScope::Personal { .. } => ChatAccess::None,
        ChatScope::Team { team_id } if viewer.manager || viewer.is_member(team_id) => {
            ChatAccess::Full
        }
        ChatScope::Team { .. } => ChatAccess::None,
        // Organization automation runs: organization resources and their results are open to
        // everyone.
        ChatScope::Org => ChatAccess::Full,
    }
}

/// Who can delete a session: the owner of a personal session, the initiator of a team session,
/// and administrators.
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

        // Administrators do not need to join the team first.
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
    fn an_org_chat_loads_only_the_organizations_resources_and_keeps_no_memory() {
        let chat = ChatScope::Org;

        assert!(chat.loads(&Scope::Org));
        assert!(!chat.loads(&team("sre")));
        assert!(!chat.loads(&personal("ann")));
        assert_eq!(chat.memory_scope(), None);
        assert_eq!(
            ChatScope::Team { team_id: id("sre") }.memory_scope(),
            Some(team("sre"))
        );
        assert_eq!(
            chat_access(&viewer("bob", false, &[]), &chat, &id("ann")),
            ChatAccess::Full
        );
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

        // Team sessions can be deleted by the initiator or an administrator; other members can only
        // read and continue the conversation.
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
