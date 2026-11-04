// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `get_agent_insights`：`/insight` 读的数。范围跟着会话：团队会话看本队，个人会话看发起人与
//! 他所在的团队；组织会话（组织作用域的自动化）只看组织的自动化。

use serde::Deserialize;
use serde_json::Value;

use super::{WorkspaceCall, parse};
use crate::{
    agent::{
        insights::{DEFAULT_DAYS, InsightScope, MAX_DAYS},
        scope::ChatScope,
        tools::ToolResult,
    },
    shared::{Result, time::TimestampMicros},
};

const MICROS_PER_DAY: i64 = 24 * 60 * 60 * 1_000_000;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct InsightArgs {
    #[serde(default)]
    days: Option<i64>,
}

pub(super) fn scope_of(call: &WorkspaceCall<'_>) -> InsightScope {
    let org_id = call.ctx.iam_context().org_id.clone();
    let workspace = call.workspace;

    match &workspace.scope {
        ChatScope::Team { team_id } => InsightScope {
            org_id,
            owner_id: None,
            team_ids: vec![team_id.clone()],
        },
        ChatScope::Personal { owner_id, .. } => InsightScope {
            org_id,
            owner_id: Some(owner_id.clone()),
            team_ids: workspace.viewer.teams.iter().cloned().collect(),
        },
        ChatScope::Org => InsightScope {
            org_id,
            owner_id: None,
            team_ids: Vec::new(),
        },
    }
}

pub(super) async fn execute(call: &WorkspaceCall<'_>, arguments: Value) -> Result<ToolResult> {
    let args: InsightArgs = if arguments.is_null() {
        InsightArgs::default()
    } else {
        parse(arguments)?
    };

    let days = args.days.unwrap_or(DEFAULT_DAYS).clamp(1, MAX_DAYS);
    let since = TimestampMicros(TimestampMicros::now().0 - days * MICROS_PER_DAY);

    let insights = call
        .state
        .agent
        .chats
        .insights(&scope_of(call), since)
        .await?;

    Ok(ToolResult::json(serde_json::json!({
        "days": days,
        "insights": insights,
    })))
}
