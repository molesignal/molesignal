// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 发消息时的斜杠命令：解析、校验，把 task 命令展开成这一轮的模式、问题与上下文。不是内置
//! 命令的名称按会话里能用的 Skill 解析（[`super::skill_invocation`]），带 `:` 的是 MCP 服务器的
//! prompt（[`super::mcp_prompt`]）。
//!
//! 出错时只给一个固定的错误码和命令名，说明文字由前端按读者语言写。control 命令由前端
//! 执行，发到这里说明客户端没认出它，同样拒绝。

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

use super::{
    PostMessageReq, TimeRangeReq, mcp_prompt::invoke_mcp_prompt, skill_invocation::invoke_skill,
};
use crate::{
    agent::{
        commands::{
            CommandKind, CommandRecord, CommandSource, Input, Invocation, find_builtin, parse,
        },
        scope::Viewer,
    },
    api::{
        AppState,
        http::{interface_language::interface_language_pack, middleware::authorize_resource},
        rca::incident_facts,
    },
    app::iam::IamContext,
    domain::alerting::incident::Incident,
    infra::persistence::repositories::agent::chats::Chat,
    shared::{Error, ids::Id, time::TimestampMicros},
};

pub(super) const UNKNOWN_COMMAND: &str = "unknown_command";
pub(super) const CLIENT_COMMAND: &str = "client_command";
pub(super) const COMMAND_ARGUMENT_MISSING: &str = "command_argument_missing";
pub(super) const COMMAND_UNAVAILABLE: &str = "command_unavailable";
pub(super) const COMMAND_TARGET_NOT_FOUND: &str = "command_target_not_found";
pub(super) const COMMAND_AMBIGUOUS: &str = "command_ambiguous";
pub(super) const COMMAND_NEEDS_RUNNER: &str = "command_needs_runner";
pub(super) const COMMAND_NEEDS_CREDENTIAL: &str = "command_needs_credential";
pub(super) const COMMAND_FAILED: &str = "command_failed";

/// `/rca` 从事件开始前多久看起。
const RCA_LEAD_IN_MICROS: i64 = 30 * 60 * 1_000_000;

/// 展开后的这一轮。
#[derive(Debug, Default)]
pub(super) struct Turn {
    /// 交给模型的问题。
    pub(super) model_text: String,
    /// 记在用户消息上的命令。
    pub(super) record: Option<CommandRecord>,
    /// 命令带来的 system 指令（例如事件的上下文）。
    pub(super) system_texts: Vec<String>,
    /// 任务 prompt 里的 `alert_name`。
    pub(super) alert_name: Option<String>,
    /// 这一轮的分析模式只用这一次，不记成会话的模式（`/query`）。
    pub(super) one_shot_mode: bool,
    /// 这一轮直接交给模型的工具（Skill 的 `allowed-tools`），仍受工具策略约束。
    pub(super) pinned_tools: Vec<String>,
    /// 显式调用的 Skill，记进用量。
    pub(super) skill_id: Option<String>,
}

impl Turn {
    fn command(invocation: &Invocation, model_text: String) -> Self {
        Self {
            record: Some(CommandRecord {
                name: invocation.name.clone(),
                args: invocation.args.clone(),
                source: CommandSource::Builtin,
                model_text: model_text.clone(),
                ..CommandRecord::default()
            }),
            model_text,
            ..Self::default()
        }
    }
}

pub(super) fn refuse(status: StatusCode, code: &'static str, command: &str) -> Response {
    refuse_with(status, code, command, Vec::new())
}

/// 带上可选写法（有歧义时）。
pub(super) fn refuse_with(
    status: StatusCode,
    code: &'static str,
    command: &str,
    options: Vec<String>,
) -> Response {
    (
        status,
        Json(json!({
            "error": code,
            "code": code,
            "command": command,
            "options": options,
            "message": format!("/{command}: {code}"),
        })),
    )
        .into_response()
}

/// 解析这条消息；是命令时校验并改写这一轮的请求。拒绝时返回给客户端的应答。
pub(super) async fn apply(
    state: &AppState,
    ctx: &IamContext,
    viewer: &Viewer,
    chat: &Chat,
    req: &mut PostMessageReq,
) -> std::result::Result<Turn, Response> {
    let invocation = match parse(&req.content) {
        Input::Text(text) => {
            return Ok(Turn {
                model_text: text,
                ..Turn::default()
            });
        }
        Input::Command(invocation) => invocation,
    };

    let name = invocation.name.as_str();

    if name.contains(':') {
        return invoke_mcp_prompt(state, ctx, viewer, chat, &invocation).await;
    }

    let command = match find_builtin(name) {
        Some(command) if invocation.team.is_none() => command,
        _ => return invoke_skill(state, ctx, viewer, chat, &invocation).await,
    };

    if command.kind == CommandKind::Control {
        return Err(refuse(StatusCode::BAD_REQUEST, CLIENT_COMMAND, name));
    }

    if command
        .permission
        .is_some_and(|permission| !ctx.has_permission(permission))
    {
        return Err(refuse(StatusCode::FORBIDDEN, COMMAND_UNAVAILABLE, name));
    }

    if invocation.args.is_empty() {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            COMMAND_ARGUMENT_MISSING,
            name,
        ));
    }

    match name {
        "query" => {
            req.analysis_mode = Some("query_generation".into());
            req.capability = None;

            Ok(Turn {
                one_shot_mode: true,
                ..Turn::command(&invocation, invocation.args.clone())
            })
        }
        "dashboard" => {
            req.capability = Some("dashboard_authoring".into());

            Ok(Turn::command(&invocation, invocation.args.clone()))
        }
        "rca" => rca(state, ctx, req, &invocation).await,
        _ => Err(refuse(StatusCode::BAD_REQUEST, UNKNOWN_COMMAND, name)),
    }
}

/// `/rca <事件> [补充]`：在事件前后的时间里做根因调查。
async fn rca(
    state: &AppState,
    ctx: &IamContext,
    req: &mut PostMessageReq,
    invocation: &Invocation,
) -> std::result::Result<Turn, Response> {
    let (incident_id, extra) = invocation
        .args
        .split_once(char::is_whitespace)
        .map(|(id, extra)| (id, extra.trim()))
        .unwrap_or((invocation.args.as_str(), ""));

    let incident: Incident = match authorize_resource::<Incident>(
        state,
        ctx,
        Id::from_string(incident_id),
        "alerts.read",
    )
    .await
    {
        Ok(incident) => incident,
        Err(Error::Forbidden(_)) => {
            return Err(refuse(StatusCode::FORBIDDEN, COMMAND_UNAVAILABLE, "rca"));
        }
        Err(Error::NotFound(_)) => {
            return Err(refuse(
                StatusCode::NOT_FOUND,
                COMMAND_TARGET_NOT_FOUND,
                "rca",
            ));
        }
        Err(error) => return Err(error.into_response()),
    };

    let start = incident.created_at.0 - RCA_LEAD_IN_MICROS;
    let end = incident.resolved_at.unwrap_or_else(TimestampMicros::now).0;

    req.time_range = Some(TimeRangeReq {
        start_micros: start,
        end_micros: end.max(start),
    });

    req.analysis_mode = Some("root_cause".into());
    req.capability = None;

    let question = interface_language_pack(state, ctx, "agent.command.rca")
        .await
        .render(
            "agent.command.rca.question",
            &[("summary", incident.summary.as_str())],
        )
        .unwrap_or_else(|_| {
            format!(
                "Find the root cause of the incident \"{}\".",
                incident.summary
            )
        });

    let model_text = if extra.is_empty() {
        question
    } else {
        format!("{question}\n\n{extra}")
    };

    let mut turn = Turn::command(invocation, model_text);

    if let Some(record) = turn.record.as_mut() {
        record.incident_id = Some(incident.id.0.clone());
    }

    turn.system_texts.push(format!(
        "The user asked for a root cause investigation of the incident below. Work over the incident's time range: confirm what changed and which services were affected with your tools before concluding, and say which evidence supports the cause.\n\n{}",
        incident_facts(&incident)
    ));

    turn.alert_name = Some(incident.summary);
    Ok(turn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_the_command_and_a_stable_code() {
        let response = refuse(StatusCode::BAD_REQUEST, UNKNOWN_COMMAND, "nope");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn a_command_turn_keeps_what_the_user_typed_and_what_the_model_reads() {
        let turn = Turn::command(
            &Invocation {
                name: "query".into(),
                team: None,
                args: "errors by service".into(),
            },
            "errors by service".into(),
        );

        let record = turn.record.unwrap();
        assert_eq!(record.name, "query");
        assert_eq!(record.args, "errors by service");
        assert_eq!(record.model_text, "errors by service");
        assert_eq!(record.source, CommandSource::Builtin);
    }
}
