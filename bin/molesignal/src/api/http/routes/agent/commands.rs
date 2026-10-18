// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /api/v1/agent/commands?chat_id=&team_id=`：对话里能用的斜杠命令。
//!
//! 内置命令之外，还有这个会话能用的 Skill 与 MCP prompt：带 `chat_id` 时按那个会话算，新会话
//! 按选的 `team_id`（不填是个人会话）算。个人会话里另外列出本人所在团队、还没挂进来的 Skill，
//! 写成 `名称@团队`，调用时把团队挂进会话；团队服务器的 prompt 同样在调用时挂进团队。说明与
//! 参数提示按读者的界面语言取自 `i18n_language`（`agent.command.*`）。用不了的命令也列出来，
//! 带上原因。

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::viewer::{chat_scope, readable_chat, viewer};
use crate::{
    agent::{
        commands::{
            Argument, BUILTIN_COMMANDS, BuiltinCommand, CommandKind, CommandSource, Suggest,
        },
        scope::{ChatScope, Scope, Viewer},
        skills::SkillSource,
    },
    api::{AppState, http::interface_language::interface_language_pack},
    app::{
        iam::IamContext,
        mcp::SessionPrompt,
        skills::{BUNDLED_SKILL_KEY, SessionSkill, bundled_text},
    },
    domain::{i18n_language::LanguagePack, iam::permission},
    shared::{Error, Result, ids::Id},
};

const COMMAND_KEY: &str = "agent.command";

pub fn routes() -> Router<AppState> {
    Router::new().route("/agent/commands", get(list_commands))
}

#[derive(Debug, Serialize)]
struct CommandResp {
    name: String,
    kind: CommandKind,
    source: CommandSource,
    description: String,
    /// 参数怎么写：几个可选的词用 `|` 连起来，其余是一段提示。
    argument_hint: Option<String>,
    argument_required: bool,
    choices: Vec<String>,
    suggest: Option<Suggest>,
    available: bool,
    unavailable_reason: Option<String>,
    /// 团队 Skill 的团队写法（`@` 后的部分），菜单按它分组。
    team: Option<String>,
    /// MCP prompt 所在的服务器名称，菜单按它分组。
    server: Option<String>,
}

fn describe(command: &BuiltinCommand, ctx: &IamContext, texts: &LanguagePack) -> CommandResp {
    let text = |part: &str| {
        texts
            .get(&format!("{COMMAND_KEY}.{}.{part}", command.name))
            .map(str::to_string)
    };

    let argument_hint = match command.argument {
        Argument::None => None,
        Argument::Required if !command.choices.is_empty() => Some(command.choices.join(" | ")),
        Argument::Required => text("argument"),
    };

    let missing = command
        .permission
        .filter(|permission| !ctx.has_permission(permission));

    let unavailable_reason = missing.map(|permission| {
        texts
            .render(
                &format!("{COMMAND_KEY}.unavailable.permission"),
                &[("permission", permission)],
            )
            .unwrap_or_else(|_| format!("Needs the {permission} permission"))
    });

    CommandResp {
        name: command.name.to_string(),
        kind: command.kind,
        source: CommandSource::Builtin,
        description: text("description").unwrap_or_default(),
        argument_hint,
        argument_required: command.argument == Argument::Required,
        choices: command
            .choices
            .iter()
            .map(|choice| choice.to_string())
            .collect(),
        suggest: command.suggest,
        available: missing.is_none(),
        unavailable_reason,
        team: None,
        server: None,
    }
}

/// 一个 Skill 作为命令：`/名称 参数`，参数不是必填。内置 Skill 的说明与参数提示按读者语言写
/// （`skill_texts`，见 [`bundled_text`]）。
fn skill_command(
    skill: &SessionSkill,
    texts: &LanguagePack,
    skill_texts: &LanguagePack,
) -> CommandResp {
    let unavailable_reason = skill.manifest.requires_runner.then(|| {
        texts
            .get(&format!("{COMMAND_KEY}.unavailable.runner"))
            .map(str::to_string)
            .unwrap_or_else(|| "Needs a Mole Runner to run its scripts".into())
    });

    let bundled = |part: &str| {
        (skill.source == SkillSource::Builtin)
            .then(|| bundled_text(skill_texts, &skill.name, part))
            .flatten()
            .map(str::to_string)
    };

    CommandResp {
        name: skill.invocation.clone(),
        kind: CommandKind::Task,
        source: CommandSource::Skill,
        description: bundled("description").unwrap_or_else(|| skill.description.clone()),
        argument_hint: bundled("argument_hint").or_else(|| skill.manifest.argument_hint.clone()),
        argument_required: false,
        choices: Vec::new(),
        suggest: None,
        available: unavailable_reason.is_none(),
        unavailable_reason,
        team: skill.team.as_ref().map(|(_, handle)| handle.clone()),
        server: None,
    }
}

/// 一个 MCP prompt 作为命令：`/<handle>:<prompt> 参数`。要成员凭据而本人还没存时不可用。
fn prompt_command(prompt: &SessionPrompt, texts: &LanguagePack) -> CommandResp {
    let unavailable_reason = prompt.needs_credential.then(|| {
        texts
            .render(
                &format!("{COMMAND_KEY}.unavailable.mcp_credential"),
                &[("server", prompt.server.name.as_str())],
            )
            .unwrap_or_else(|_| format!("Add your own credential for {} first", prompt.server.name))
    });

    let description = match (&prompt.prompt.title, prompt.prompt.description.trim()) {
        (_, description) if !description.is_empty() => description.to_string(),
        (Some(title), _) => title.clone(),
        (None, _) => prompt.prompt.name.clone(),
    };

    CommandResp {
        name: prompt.command.clone(),
        kind: CommandKind::Task,
        source: CommandSource::Mcp,
        description,
        argument_hint: prompt.prompt.argument_hint(),
        argument_required: prompt
            .prompt
            .arguments
            .iter()
            .any(|argument| argument.required),
        choices: Vec::new(),
        suggest: None,
        available: unavailable_reason.is_none(),
        unavailable_reason,
        team: None,
        server: Some(prompt.server.name.clone()),
    }
}

#[derive(Debug, Default, Deserialize)]
struct CommandsQuery {
    #[serde(default)]
    chat_id: Option<String>,
    /// 还没开始的会话选的团队。
    #[serde(default)]
    team_id: Option<String>,
}

/// 命令要按哪个会话算。
async fn session_scope(
    state: &AppState,
    ctx: &IamContext,
    viewer: &Viewer,
    query: &CommandsQuery,
) -> Result<ChatScope> {
    if let Some(chat_id) = query.chat_id.as_deref().filter(|id| !id.trim().is_empty()) {
        let (chat, _) = readable_chat(state, ctx, &Id::from_string(chat_id)).await?;
        return Ok(chat_scope(&chat));
    }

    if let Some(team_id) = query.team_id.as_deref().filter(|id| !id.trim().is_empty()) {
        let team_id = Id::from_string(team_id);

        if !viewer.can_edit(&Scope::Team {
            team_id: team_id.clone(),
        }) {
            return Err(Error::forbidden(
                "only the team's members can start a chat for the team",
            ));
        }

        return Ok(ChatScope::Team { team_id });
    }

    Ok(ChatScope::Personal {
        owner_id: ctx.user_id.clone(),
        mounted_teams: Vec::new(),
    })
}

#[permission("agent.use")]
async fn list_commands(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<CommandsQuery>,
) -> Result<Json<Value>> {
    let texts = interface_language_pack(&state, &ctx, COMMAND_KEY).await;
    let skill_texts = interface_language_pack(&state, &ctx, BUNDLED_SKILL_KEY).await;

    let mut commands = BUILTIN_COMMANDS
        .iter()
        .map(|command| describe(command, &ctx, &texts))
        .collect::<Vec<_>>();

    let viewer = viewer(&state, &ctx).await?;
    let scope = session_scope(&state, &ctx, &viewer, &query).await?;
    let skills = state.agent.skills.session(&ctx.org_id, &scope).await?;

    commands.extend(
        skills
            .skills
            .iter()
            .map(|skill| skill_command(skill, &texts, &skill_texts)),
    );

    commands.extend(
        state
            .agent
            .skills
            .mountable(&ctx.org_id, &viewer, &scope)
            .await?
            .iter()
            .map(|skill| skill_command(skill, &texts, &skill_texts)),
    );

    commands.extend(
        state
            .agent
            .mcp
            .session_prompts(&ctx.org_id, &viewer, &scope)
            .await?
            .iter()
            .map(|prompt| prompt_command(prompt, &texts)),
    );

    Ok(Json(json!({ "commands": commands })))
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use super::*;
    use crate::{
        agent::{
            commands::find_builtin,
            skills::{SKILL_FILE, SkillBundle, SkillFile},
        },
        app::skills::{SkillLocation, instance::InstanceSkill},
        domain::iam::IamScope,
        shared::ids::Id,
    };

    fn texts() -> LanguagePack {
        LanguagePack::new(HashMap::from([
            (
                "agent.command.query.description".to_string(),
                "只生成查询，不执行".to_string(),
            ),
            (
                "agent.command.query.argument".to_string(),
                "<问题>".to_string(),
            ),
            (
                "agent.command.unavailable.permission".to_string(),
                "需要 {permission} 权限".to_string(),
            ),
        ]))
    }

    fn member(permissions: &[&str]) -> IamContext {
        IamContext {
            user_id: Id::from_string("user-1"),
            org_id: Id::from_string("org-1"),
            display_role: "Member".into(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: permissions.iter().map(|key| key.to_string()).collect(),
            policy_version: 1,
        }
    }

    #[test]
    fn a_command_is_described_in_the_readers_language() {
        let ctx = member(&["agent.use"]);
        let query = describe(find_builtin("query").unwrap(), &ctx, &texts());

        assert_eq!(query.description, "只生成查询，不执行");
        assert_eq!(query.argument_hint.as_deref(), Some("<问题>"));
        assert!(query.argument_required);

        let mode = describe(find_builtin("mode").unwrap(), &ctx, &texts());
        assert_eq!(
            mode.argument_hint.as_deref(),
            Some("auto | quick | deep | query")
        );

        let help = describe(find_builtin("help").unwrap(), &ctx, &texts());
        assert_eq!(help.argument_hint, None);
        assert!(!help.argument_required);
    }

    fn skill(name: &str, source: SkillSource) -> SessionSkill {
        let bundle = SkillBundle::from_files(vec![SkillFile {
            path: SKILL_FILE.into(),
            bytes: format!(
                "---\nname: {name}\ndescription: Triage an incident\nargument-hint: <incident id or service>\n---\n"
            )
            .into_bytes(),
        }])
        .unwrap();

        SessionSkill {
            id: name.into(),
            name: name.into(),
            invocation: name.into(),
            description: bundle.manifest.description.clone(),
            version: None,
            hash: bundle.hash.clone(),
            source,
            team: None,
            manifest: bundle.manifest.clone(),
            location: SkillLocation::Instance(Arc::new(InstanceSkill {
                key: format!("builtin:{name}"),
                source,
                dir: None,
                bundle,
            })),
        }
    }

    #[test]
    fn a_bundled_skill_is_offered_in_the_readers_language() {
        let skill_texts = LanguagePack::new(HashMap::from([
            (
                "agent.skill.bundled.incident_triage.description".to_string(),
                "确认坏了什么、影响多大".to_string(),
            ),
            (
                "agent.skill.bundled.incident_triage.argument_hint".to_string(),
                "<事件 ID 或服务>".to_string(),
            ),
        ]));

        let bundled = skill_command(
            &skill("incident-triage", SkillSource::Builtin),
            &texts(),
            &skill_texts,
        );
        assert_eq!(bundled.description, "确认坏了什么、影响多大");
        assert_eq!(bundled.argument_hint.as_deref(), Some("<事件 ID 或服务>"));

        // 组织自己装的同名 Skill 用它自己写的文字。
        let uploaded = skill_command(
            &skill("incident-triage", SkillSource::Upload),
            &texts(),
            &skill_texts,
        );
        assert_eq!(uploaded.description, "Triage an incident");
        assert_eq!(
            uploaded.argument_hint.as_deref(),
            Some("<incident id or service>")
        );
    }

    #[test]
    fn a_command_needing_a_permission_says_which() {
        let rca = find_builtin("rca").unwrap();

        let without = describe(rca, &member(&["agent.use"]), &texts());
        assert!(!without.available);
        assert_eq!(
            without.unavailable_reason.as_deref(),
            Some("需要 alerts.read 权限")
        );

        let with = describe(rca, &member(&["agent.use", "alerts.read"]), &texts());
        assert!(with.available);
        assert_eq!(with.unavailable_reason, None);
    }
}
