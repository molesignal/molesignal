// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `remember`、`forget`、`read_memory`：会话作用域里的记忆。个人会话是发起人的，团队会话是
//! 本队的。对话里把记住与忘掉显示出来，可以撤销，所以不再另外确认。

use serde::Deserialize;
use serde_json::{Value, json};

use super::{WorkspaceCall, parse};
use crate::{
    agent::{
        memory::{MemoryDraft, MemoryKind},
        tools::{BuiltinToolKind, ToolResult},
    },
    shared::Result,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RememberArgs {
    kind: MemoryKind,
    name: String,
    description: String,
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NameArgs {
    name: String,
}

pub(super) async fn execute(
    call: &WorkspaceCall<'_>,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    let scope = call.workspace.memory_scope();

    // 团队的记忆由团队成员（与管理员）写。
    if kind != BuiltinToolKind::ReadMemory && !call.workspace.viewer.can_edit(&scope) {
        return Ok(ToolResult::error(
            "only the team's members can change its memory",
        ));
    }

    let org_id = &call.ctx.iam_context().org_id;
    let actor = &call.ctx.iam_context().user_id;
    let store = &call.state.agent.memory;

    match kind {
        BuiltinToolKind::Remember => {
            let args: RememberArgs = parse(arguments)?;

            let draft = MemoryDraft {
                kind: args.kind,
                name: args.name,
                description: args.description,
                body: args.body,
            };

            match store
                .remember(org_id, &scope, &draft, Some(&call.workspace.chat_id), actor)
                .await
            {
                Ok(memory) => Ok(ToolResult::json(json!({
                    "remembered": {
                        "id": memory.id.0,
                        "kind": memory.kind,
                        "name": memory.name,
                        "scope": memory.scope.kind(),
                    }
                }))),
                Err(error) => Ok(ToolResult::error(error.to_string())),
            }
        }
        BuiltinToolKind::Forget => {
            let args: NameArgs = parse(arguments)?;

            let Some(memory) = store.find(org_id, &scope, &args.name).await? else {
                return Ok(ToolResult::error(format!(
                    "there is no memory called `{}` here",
                    args.name.trim()
                )));
            };

            let memory = store.forget(org_id, &memory.id, actor).await?;

            Ok(ToolResult::json(json!({
                "forgotten": { "id": memory.id.0, "name": memory.name, "scope": memory.scope.kind() }
            })))
        }
        _ => {
            let args: NameArgs = parse(arguments)?;

            Ok(match store.find(org_id, &scope, &args.name).await? {
                Some(memory) => ToolResult::json(json!({
                    "name": memory.name,
                    "kind": memory.kind,
                    "description": memory.description,
                    "body": memory.body,
                    "updated_at_micros": memory.updated_at.0,
                })),
                None => ToolResult::error(format!(
                    "there is no memory called `{}` here",
                    args.name.trim()
                )),
            })
        }
    }
}
