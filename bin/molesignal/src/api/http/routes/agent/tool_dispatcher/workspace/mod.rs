// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跟着会话走的工具：知识（[`knowledge`]）、记忆（[`memory`]）与调查记录（[`investigation`]）。
//!
//! 读哪份知识、写谁的记忆、记哪个调查都由会话决定（[`ChatWorkspace`]），不听模型传的身份：
//! - 团队会话读组织与本队的知识，写本队的知识与记忆；
//! - 个人会话读组织的知识，也能读本人所在团队的知识（读到即把团队挂进会话），写组织的知识
//!   （要 `agent.manage`）与本人的记忆。
//!
//! 写知识前都请提问人在对话里确认。

use std::{collections::HashSet, sync::Arc};

use parking_lot::Mutex;
use serde_json::Value;

use super::session::RunInteraction;
use crate::{
    agent::{
        scope::{ChatScope, Scope, Viewer},
        tools::{BuiltinToolKind, ToolAuthContext, ToolResult},
    },
    api::AppState,
    shared::{Error, Result, ids::Id},
};

mod investigation;
mod knowledge;
mod memory;

/// 一个会话在知识与记忆上的位置。
#[derive(Debug)]
pub(crate) struct ChatWorkspace {
    pub(crate) chat_id: Id,
    pub(crate) scope: ChatScope,
    /// 发起这一轮的人。
    pub(crate) viewer: Viewer,
    /// 组织里的团队（ID, 名称）：模型按名字指团队。
    pub(crate) teams: Vec<(Id, String)>,
    /// 这一轮里挂进会话的团队：每个团队只挂一次、只提醒一次。
    pub(crate) mounted: Mutex<HashSet<Id>>,
}

impl ChatWorkspace {
    fn team_name(&self, team_id: &Id) -> String {
        self.teams
            .iter()
            .find(|(id, _)| id == team_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| team_id.0.clone())
    }

    /// 按名字或 ID 找团队。
    fn team(&self, raw: &str) -> Result<Id> {
        let raw = raw.trim().trim_start_matches('@');

        self.teams
            .iter()
            .find(|(id, name)| id.0 == raw || name.eq_ignore_ascii_case(raw))
            .map(|(id, _)| id.clone())
            .ok_or_else(|| Error::invalid(format!("there is no team called `{raw}`")))
    }

    /// 这个会话的记忆写在哪里：团队会话写本队的，个人会话写发起人的。
    fn memory_scope(&self) -> Scope {
        match &self.scope {
            ChatScope::Team { team_id } => Scope::Team {
                team_id: team_id.clone(),
            },
            ChatScope::Personal { owner_id, .. } => Scope::Personal {
                owner_id: owner_id.clone(),
            },
        }
    }
}

/// 交给各个工具的东西。
pub(super) struct WorkspaceCall<'a> {
    pub(super) state: &'a AppState,
    pub(super) ctx: &'a ToolAuthContext,
    pub(super) workspace: &'a ChatWorkspace,
    pub(super) interaction: Option<&'a dyn RunInteraction>,
}

pub(super) async fn execute(
    state: &AppState,
    ctx: &ToolAuthContext,
    workspace: Option<&Arc<ChatWorkspace>>,
    interaction: Option<&dyn RunInteraction>,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    let Some(workspace) = workspace else {
        return Ok(ToolResult::error(format!(
            "`{}` works in a Mole Agent chat only",
            kind.name()
        )));
    };

    let call = WorkspaceCall {
        state,
        ctx,
        workspace,
        interaction,
    };

    match kind {
        BuiltinToolKind::ListKnowledge
        | BuiltinToolKind::ReadKnowledge
        | BuiltinToolKind::SearchKnowledge
        | BuiltinToolKind::EditKnowledge => knowledge::execute(&call, kind, arguments).await,
        BuiltinToolKind::Remember | BuiltinToolKind::Forget | BuiltinToolKind::ReadMemory => {
            memory::execute(&call, kind, arguments).await
        }
        BuiltinToolKind::UpdateInvestigation => investigation::execute(&call, arguments).await,
        _ => Ok(ToolResult::error(format!("unknown tool: {}", kind.name()))),
    }
}

fn parse<T: serde::de::DeserializeOwned>(arguments: Value) -> Result<T> {
    serde_json::from_value(arguments)
        .map_err(|error| Error::invalid(format!("invalid tool arguments: {error}")))
}
