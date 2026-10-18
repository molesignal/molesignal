// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `list_knowledge`、`read_knowledge`、`search_knowledge`、`edit_knowledge`。

use serde::Deserialize;
use serde_json::{Value, json};

use super::{WorkspaceCall, parse};
use crate::{
    agent::{
        knowledge::{ENTRY_FILE, check_content, normalize_path},
        model::RiskLevel,
        scope::{ChatScope, Scope},
        tools::{BuiltinToolKind, ToolResult},
    },
    api::http::routes::{activity_audit, agent::tool_dispatcher::session},
    shared::{Error, Result},
};

/// 读一个文件时最多交给模型这么多字节。
const READ_BYTES: usize = 48 * 1024;
/// 搜索默认与最多给几行。
const SEARCH_DEFAULT: usize = 20;
const SEARCH_MAX: usize = 50;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    delete: bool,
    #[serde(default)]
    summary: Option<String>,
}

pub(super) async fn execute(
    call: &WorkspaceCall<'_>,
    kind: BuiltinToolKind,
    arguments: Value,
) -> Result<ToolResult> {
    if kind == BuiltinToolKind::EditKnowledge {
        return edit(call, parse(arguments)?).await;
    }

    let args: Args = parse(arguments)?;

    if kind == BuiltinToolKind::SearchKnowledge && args.scope.is_none() && args.team.is_none() {
        return search(call, loaded(call), &args).await;
    }

    let scope = match requested(call, &args) {
        Ok(scope) => scope,
        Err(error) => return Ok(ToolResult::error(error.to_string())),
    };

    if let Err(refusal) = admit(call, &scope, kind != BuiltinToolKind::SearchKnowledge).await {
        return Ok(refusal);
    }

    match kind {
        BuiltinToolKind::ListKnowledge => list(call, &scope).await,
        BuiltinToolKind::ReadKnowledge => {
            read(call, &scope, args.path.as_deref().unwrap_or_default()).await
        }
        _ => search(call, vec![scope], &args).await,
    }
}

/// 参数指的那份知识；没指定时是团队会话的团队，否则组织。
fn requested(call: &WorkspaceCall<'_>, args: &Args) -> Result<Scope> {
    let chat_team = call.workspace.scope.team_id().cloned();

    match (args.scope.as_deref(), args.team.as_deref()) {
        (Some("org"), _) => Ok(Scope::Org),
        (Some("team") | None, Some(team)) => Ok(Scope::Team {
            team_id: call.workspace.team(team)?,
        }),
        (Some("team"), None) => chat_team
            .map(|team_id| Scope::Team { team_id })
            .ok_or_else(|| Error::invalid("name the team whose knowledge to read")),
        (None, None) => Ok(chat_team.map_or(Scope::Org, |team_id| Scope::Team { team_id })),
        (Some(other), _) => Err(Error::invalid(format!(
            "knowledge belongs to `org` or a `team`, not `{other}`"
        ))),
    }
}

/// 会话已经加载的知识：组织的，团队会话的团队或个人会话挂进来的团队。
fn loaded(call: &WorkspaceCall<'_>) -> Vec<Scope> {
    let mut scopes = vec![Scope::Org];

    match &call.workspace.scope {
        ChatScope::Team { team_id } => scopes.push(Scope::Team {
            team_id: team_id.clone(),
        }),
        ChatScope::Personal { mounted_teams, .. } => {
            scopes.extend(mounted_teams.iter().map(|team_id| Scope::Team {
                team_id: team_id.clone(),
            }));
        }
    }

    scopes
}

/// 这个会话能不能读这份知识。个人会话读本人所在团队的知识时，`mount` 为真就把团队（连同它的
/// 知识、Skill 与 MCP）挂进会话，并在对话里留一条提醒。
async fn admit(
    call: &WorkspaceCall<'_>,
    scope: &Scope,
    mount: bool,
) -> std::result::Result<(), ToolResult> {
    let workspace = call.workspace;

    if workspace.scope.loads(scope) {
        return Ok(());
    }

    match (&workspace.scope, scope) {
        (ChatScope::Personal { .. }, Scope::Team { team_id })
            if workspace.viewer.is_member(team_id) =>
        {
            if !mount || !workspace.mounted.lock().insert(team_id.clone()) {
                return Ok(());
            }

            match call
                .state
                .agent
                .chats
                .mount_team(&workspace.chat_id, &team_id.0)
                .await
            {
                Ok(()) => {
                    if let Some(interaction) = call.interaction {
                        interaction
                            .notice(json!({
                                "kind": "team_mounted",
                                "team_id": team_id.0,
                                "team": workspace.team_name(team_id),
                            }))
                            .await;
                    }
                }
                Err(error) => tracing::warn!(%error, "a team could not be mounted into a chat"),
            }

            Ok(())
        }
        (ChatScope::Personal { .. }, _) => Err(ToolResult::error(
            "a personal chat reads the knowledge of the teams its owner belongs to",
        )),
        _ => Err(ToolResult::error(
            "a team chat reads only the organization's knowledge and its own team's",
        )),
    }
}

fn describe(call: &WorkspaceCall<'_>, scope: &Scope) -> Value {
    match scope {
        Scope::Team { team_id } => {
            json!({ "scope": "team", "team": call.workspace.team_name(team_id) })
        }
        _ => json!({ "scope": "org" }),
    }
}

async fn list(call: &WorkspaceCall<'_>, scope: &Scope) -> Result<ToolResult> {
    let files = call
        .state
        .agent
        .knowledge
        .list(&call.ctx.iam_context().org_id, scope)
        .await?;

    let mut result = describe(call, scope);
    result["entry_file"] = json!(files.iter().any(|file| file.path == ENTRY_FILE));

    result["files"] = files
        .iter()
        .map(|file| json!({ "path": file.path, "size_bytes": file.size_bytes, "updated_at_micros": file.updated_at.0 }))
        .collect();

    Ok(ToolResult::json(result))
}

async fn read(call: &WorkspaceCall<'_>, scope: &Scope, path: &str) -> Result<ToolResult> {
    let path = normalize_path(path.trim_start_matches('@'))?;

    let file = call
        .state
        .agent
        .knowledge
        .get(&call.ctx.iam_context().org_id, scope, &path)
        .await?;

    let Some(file) = file else {
        return Ok(ToolResult::error(format!(
            "there is no knowledge file `{path}` here; list_knowledge lists the files"
        )));
    };

    let mut end = file.content.len().min(READ_BYTES);

    while !file.content.is_char_boundary(end) {
        end -= 1;
    }

    let mut result = describe(call, scope);
    result["path"] = json!(file.path);
    result["content"] = json!(&file.content[..end]);
    result["truncated"] = json!(end < file.content.len());
    result["size_bytes"] = json!(file.size_bytes);
    result["updated_at_micros"] = json!(file.updated_at.0);

    Ok(ToolResult::json(result))
}

async fn search(call: &WorkspaceCall<'_>, scopes: Vec<Scope>, args: &Args) -> Result<ToolResult> {
    let query = args.query.as_deref().unwrap_or_default().trim();

    if query.is_empty() {
        return Ok(ToolResult::error("say what to search for"));
    }

    let limit = args.limit.unwrap_or(SEARCH_DEFAULT).clamp(1, SEARCH_MAX);

    let hits = call
        .state
        .agent
        .knowledge
        .search(&call.ctx.iam_context().org_id, &scopes, query, limit)
        .await?;

    let hits = hits
        .iter()
        .map(|hit| {
            let mut entry = describe(call, &hit.scope);
            entry["path"] = json!(hit.path);
            entry["line"] = json!(hit.line);
            entry["text"] = json!(hit.text);
            entry
        })
        .collect::<Vec<_>>();

    Ok(ToolResult::json(json!({ "query": query, "hits": hits })))
}

/// 会话能写的那份知识：团队会话写本队的，个人会话写组织的。
async fn edit(call: &WorkspaceCall<'_>, args: EditArgs) -> Result<ToolResult> {
    let workspace = call.workspace;

    let scope = workspace
        .scope
        .team_id()
        .map_or(Scope::Org, |team_id| Scope::Team {
            team_id: team_id.clone(),
        });

    // 「只给建议」与「只读」的执行策略下不改任何东西。
    if !call.ctx.execution_policy().allows_approval_request() {
        return Ok(ToolResult::error(
            "this chat's execution policy does not allow changes; switch it with /policy approval or /policy auto",
        ));
    }

    if !workspace.viewer.can_edit(&scope) {
        return Ok(ToolResult::error(
            "a personal chat writes the organization's knowledge, which needs agent.manage; open a team chat to write a team's knowledge",
        ));
    }

    let path = match normalize_path(args.path.trim_start_matches('@')) {
        Ok(path) => path,
        Err(error) => return Ok(ToolResult::error(error.to_string())),
    };

    let content = match (&args.content, args.delete) {
        (_, true) => None,
        (Some(content), false) => {
            if let Err(error) = check_content(content) {
                return Ok(ToolResult::error(error.to_string()));
            }

            Some(content.clone())
        }
        (None, false) => return Ok(ToolResult::error("give the whole new content, or delete")),
    };

    // 确认卡上说明改哪个文件、怎么改。
    let mut shown = describe(call, &scope);
    shown["path"] = json!(path);
    shown["delete"] = json!(content.is_none());
    shown["summary"] = json!(args.summary.as_deref().unwrap_or_default());
    shown["size_bytes"] = json!(content.as_ref().map_or(0, String::len));
    shown["preview"] = json!(
        content
            .as_deref()
            .map(|text| text.chars().take(600).collect::<String>())
    );

    // 「自动」不问改写（知识有历史版本），删除照样问；bypass permissions 都不问。
    let skip = call
        .ctx
        .execution_policy()
        .skips_confirmation(content.is_none());

    if !skip
        && let Err(result) =
            session::confirmation(call.interaction, "edit_knowledge", &shown, RiskLevel::L1).await
    {
        return Ok(result);
    }

    let org_id = &call.ctx.iam_context().org_id;
    let actor = &call.ctx.iam_context().user_id;
    let store = &call.state.agent.knowledge;

    let outcome = match &content {
        Some(content) => store
            .put(org_id, &scope, &[(path.clone(), content.clone())], actor)
            .await
            .map(|()| "written"),
        None => store.delete(org_id, &scope, &path).await.map(|_| "deleted"),
    };

    let action = match outcome {
        Ok(action) => action,
        Err(error) => return Ok(ToolResult::error(error.to_string())),
    };

    activity_audit::record(
        call.state,
        call.ctx.iam_context(),
        "agent.knowledge.changed",
        "agent_knowledge",
        &scope.key(),
        json!({ "path": path, "action": action, "chat_id": workspace.chat_id.0 }),
    )
    .await;

    let mut result = describe(call, &scope);
    result["path"] = json!(path);
    result[action] = json!(true);

    Ok(ToolResult::json(result))
}
