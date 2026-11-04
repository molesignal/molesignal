// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{bounded, json_result, parse_args},
};
use crate::{
    app::iam::IamContext,
    shared::{Error, Result, ids::Id},
};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentArgs {
    investigation_id: Option<String>,
    approval_id: Option<String>,
    execution_id: Option<String>,
    limit: Option<usize>,
}

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: serde_json::Value,
) -> Result<ToolResult> {
    let args: AgentArgs = parse_args(arguments)?;

    match kind {
        BuiltinToolKind::ListAgentInvestigations => json_result(&bounded(
            runtime
                .agent
                .repository
                .list_investigations(&auth.org_id)
                .await?,
            args.limit,
            500,
        )),
        BuiltinToolKind::GetAgentInvestigation => {
            let id = args
                .investigation_id
                .filter(|value| !value.trim().is_empty())
                .map(Id)
                .ok_or_else(|| Error::invalid("investigationId is required"))?;

            json_result(
                &runtime
                    .agent
                    .repository
                    .get_investigation(&auth.org_id, &id)
                    .await?,
            )
        }
        BuiltinToolKind::ListAgentApprovals => json_result(&bounded(
            runtime
                .agent
                .repository
                .list_approvals(&auth.org_id)
                .await?,
            args.limit,
            500,
        )),
        BuiltinToolKind::GetAgentApproval => {
            let id = required_id(args.approval_id, "approvalId")?;

            json_result(
                &runtime
                    .agent
                    .repository
                    .get_approval(&auth.org_id, &id)
                    .await?,
            )
        }
        BuiltinToolKind::ListAgentExecutions => json_result(&bounded(
            runtime
                .agent
                .repository
                .list_executions(&auth.org_id)
                .await?,
            args.limit,
            500,
        )),
        BuiltinToolKind::GetAgentExecution => {
            let id = required_id(args.execution_id, "executionId")?;

            let execution = runtime
                .agent
                .repository
                .get_execution(&auth.org_id, &id)
                .await?;

            json_result(&execution)
        }
        BuiltinToolKind::ListAgentAutomations => {
            // 只列调用的人看得到的：组织的、他所在团队的、他自己的。
            let teams = runtime
                .administration
                .teams
                .list_for_member(&auth.org_id, &auth.user_id)
                .await?
                .into_iter()
                .map(|team| team.id)
                .collect::<std::collections::HashSet<_>>();

            let visible = runtime
                .agent
                .automations
                .list(&auth.org_id)
                .await?
                .into_iter()
                .filter(|automation| match &automation.scope {
                    crate::agent::scope::Scope::Org => true,
                    crate::agent::scope::Scope::Team { team_id } => teams.contains(team_id),
                    crate::agent::scope::Scope::Personal { owner_id } => *owner_id == auth.user_id,
                })
                .collect::<Vec<_>>();

            json_result(&bounded(visible, args.limit, 500))
        }
        BuiltinToolKind::ExecuteAgentApproval => {
            unreachable!("approved execution is handled by the entry operation coordinator")
        }
        _ => unreachable!("agent-control executor received unrelated tool"),
    }
}

fn required_id(value: Option<String>, field: &str) -> Result<Id> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(Id)
        .ok_or_else(|| Error::invalid(format!("{field} is required")))
}
