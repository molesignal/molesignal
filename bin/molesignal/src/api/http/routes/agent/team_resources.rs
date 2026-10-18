// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一个团队名下的 Mole Agent 资源。删除团队前要先移交或删除它们，免得团队私有的内容被并入
//! 组织或无人能管。

use serde::Serialize;

use crate::{
    agent::scope::Scope,
    api::AppState,
    shared::{Result, ids::Id},
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TeamResource {
    /// `skill`、`skill_repository`、`mcp_server`。
    pub kind: &'static str,
    pub id: String,
    pub name: String,
}

pub(crate) async fn team_resources(
    state: &AppState,
    org_id: &Id,
    team_id: &Id,
) -> Result<Vec<TeamResource>> {
    let in_team = |scope: &Scope| scope.team_id() == Some(team_id);
    let store = state.agent.skills.store_handle();
    let mut resources = Vec::new();

    for repository in store.list_repositories(org_id).await? {
        if in_team(&repository.scope) {
            resources.push(TeamResource {
                kind: "skill_repository",
                id: repository.id.0,
                name: repository.url,
            });
        }
    }

    for skill in store.list(org_id).await? {
        // 仓库来源的 Skill 跟着仓库走，只列仓库。
        if in_team(&skill.scope) && skill.repository_id.is_none() {
            resources.push(TeamResource {
                kind: "skill",
                id: skill.id.0,
                name: skill.name,
            });
        }
    }

    for server in state.agent.tool_control.list_mcp_servers(org_id).await? {
        if in_team(&server.scope) {
            resources.push(TeamResource {
                kind: "mcp_server",
                id: server.id.0,
                name: server.name,
            });
        }
    }

    Ok(resources)
}
