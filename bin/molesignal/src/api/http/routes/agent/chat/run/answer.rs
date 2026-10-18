// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮回答：解析模型服务、prompt、Skill 与工具，交给 AgentLoop 流式执行，最后落回答。
//!
//! 工具调用的身份恒取自重新解析的 `IamContext`，忽略模型传的身份字段；默认 Agent Profile 与
//! 组织工具集只能进一步收窄编译期内置白名单。每次模型调用都记进组织的每日用量。

use std::{sync::Arc, time::Duration};

use serde_json::{Value, json};

use super::{
    super::{
        prompts::{PromptMeta, resolve_prompts},
        provider::resolve_provider,
        tool_schema::tools_schema_for,
    },
    Ending, RunControl,
    history::{self, Conversation},
    interaction::Interaction,
    request::AnswerRequest,
    sink::{Record, RunSink},
    title, workspace,
};
use crate::{
    agent::{
        capabilities::dashboard_authoring,
        chat::{AgentLoop, RunConclusion, ToolChoice},
        runs::{RunEventKind, RunStatus, TEXT_FLUSH_MILLIS, language_instruction},
        skills::render_catalog,
        tools::{AgentExecutionPolicy, ToolAuthContext, ToolDispatcher, builtin_tools},
        usage::UsageOrigin,
    },
    api::{
        AppState,
        http::routes::agent::{
            budget, tool_dispatcher::RealToolDispatcher, toolsets, viewer::chat_scope,
        },
    },
    app::{
        agent_usage::{BudgetGate, UsageRecorder, UsageScope},
        iam::IamContext,
        skills::SessionSkills,
    },
    infra::persistence::repositories::agent::chats::{Chat, ChatMessage},
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn answer(
    control: &Arc<RunControl>,
    ctx: &IamContext,
    mut chat: Chat,
    mut request: AnswerRequest,
) -> Ending {
    let state = &control.state;

    // bypass permissions 只在生效的 Agent Profile 允许时有效：排队期间被关掉时按「执行（需
    // 审批）」处理，改动都先问提问人，并说一声。
    if request.message.execution_policy == Some(AgentExecutionPolicy::BypassPermissions) {
        let profile_id = request
            .message
            .agent_profile_id
            .as_deref()
            .map(Id::from_string);
        let allowed = toolsets::bypass_permissions_allowed(state, &ctx.org_id, profile_id.as_ref())
            .await
            .unwrap_or(false);

        if !allowed {
            request.message.execution_policy = Some(AgentExecutionPolicy::Policy);

            control
                .emit(
                    RunEventKind::Notice,
                    json!({ "kind": "bypass_unavailable" }),
                )
                .await;
        }
    }

    let req = &request.message;
    let turn = &request.turn;

    // 组织今天的预算：排队这段时间里可能已经用完。
    let gate = state.agent.usage.gate(&ctx.org_id, true).await;

    if let Some(notice) = budget::notice(gate.status()) {
        control.emit(RunEventKind::Notice, notice).await;
    }

    if let BudgetGate::Refuse(_) = gate {
        return Ending::refused(budget::BUDGET_EXHAUSTED);
    }

    // 模型服务用不了时落一条说明。
    let resolved = match resolve_provider(
        state,
        ctx,
        &chat,
        req.provider_id.as_deref(),
        req.model.as_deref(),
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(error) => {
            let asked_at = request.asked_at_micros.map(TimestampMicros);
            return failure_message(control, ctx, &chat, asked_at, error.to_string()).await;
        }
    };

    chat.provider = resolved.provider.as_str().to_string();
    chat.model = resolved.model.clone();
    chat.provider_id = resolved.provider_id.clone();

    if request.dashboard.is_some() {
        chat.analysis_mode = Some("dashboard".into());
    }

    let _ = state
        .agent
        .chats
        .set_chat_provider(
            &chat.id,
            &chat.provider,
            &chat.model,
            chat.provider_id.as_deref(),
        )
        .await;

    let (mut system_texts, prompt_meta) = resolve_prompts(
        state,
        ctx,
        &chat,
        req,
        turn.alert_name.as_deref(),
        request.dashboard.is_some(),
    )
    .await;

    system_texts.extend(language_instruction(&chat.reply_language));

    // 会话里能用的 Skill：清单交给模型，需要时用 load_skill 读正文。只生成查询时不调工具。
    let query_only = req.analysis_mode.as_deref() == Some("query_generation");

    let skills = if query_only {
        SessionSkills::default()
    } else {
        state
            .agent
            .skills
            .session(&ctx.org_id, &chat_scope(&chat))
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "the skills of a chat could not be read");
                SessionSkills::default()
            })
    };

    system_texts.extend(render_catalog(&skills.catalog()));

    // 知识入口文件与记忆目录，然后是命令与引用带来的指令。
    let loaded = workspace::load(state, ctx, &chat).await;
    system_texts.extend(loaded.system_texts);
    system_texts.extend(turn.system_texts.iter().cloned());

    let requested_profile_id = req.agent_profile_id.as_deref().map(|id| Id(id.to_string()));

    let resolution = match toolsets::resolve_toolsets_for_profile(
        state,
        &ctx.org_id,
        requested_profile_id.as_ref(),
        Some(&chat_scope(&chat)),
    )
    .await
    {
        Ok(resolution) => resolution,
        Err(error) => return Ending::failed(error.to_string()),
    };

    let initial_tool_choice = match dashboard_choice(
        state,
        &resolution,
        req,
        request.dashboard,
        &mut system_texts,
    )
    .await
    {
        Ok(choice) => choice,
        Err(error) => return Ending::failed(error.to_string()),
    };

    let rows = match state.agent.chats.list_messages(&chat.id).await {
        Ok(rows) => rows,
        Err(error) => return Ending::failed(error.to_string()),
    };

    // 每次模型调用都记进组织的每日用量与 `_agent_model_traces`；合计成本写到回答上。
    let recorder = state.agent.usage.recorder(UsageScope {
        user_id: Some(ctx.user_id.clone()),
        provider_id: resolved.provider_id.clone().map(Id),
        prices: resolved.prices,
        chat_id: Some(chat.id.clone()),
        profile_id: resolution.active_profile_id.clone(),
        prompt_hash: prompt_meta.hash.clone(),
        skill_id: turn.skill_id.clone(),
        ..UsageScope::new(ctx.org_id.clone(), UsageOrigin::Chat)
    });

    let window = resolution
        .limits
        .context_tokens
        .unwrap_or(control.context_window);

    let conversation = history::for_answer(
        control,
        &chat,
        &resolved,
        &recorder,
        rows,
        &system_texts,
        window,
    )
    .await;

    let mut visible_tools = turn.pinned_tools.clone();
    visible_tools.extend(loaded.tools);

    if !skills.is_empty() {
        visible_tools.extend(["list_skills", "load_skill", "read_skill_file"].map(String::from));
    }

    let dispatcher: Arc<dyn ToolDispatcher> = Arc::new(
        RealToolDispatcher::new(state.clone())
            .with_toolsets(resolution.clone())
            .with_skills(Arc::new(skills))
            .with_workspace(loaded.workspace)
            .with_interaction(Arc::new(Interaction::new(control.clone()))),
    );

    let agent_loop = AgentLoop::new(resolved.adapter.clone(), dispatcher)
        .with_limits(resolution.limits)
        .with_observer(recorder.clone());

    let tool_ctx = ToolAuthContext::from_iam(ctx)
        .with_chat(Some(chat.id.0.clone()))
        .with_investigation(
            chat.investigation_id
                .clone()
                .or(req.investigation_id.clone()),
        )
        .with_execution_policy(req.execution_policy.unwrap_or_default())
        .query_generation_only(query_only);

    let tools_schema = tools_schema_for(resolved.provider, &resolution, &visible_tools);
    let message_id = Id::new();
    let sink = RunSink::new(
        control.clone(),
        ctx.clone(),
        chat.id.clone(),
        message_id.clone(),
    );
    let Conversation { messages, asked_at } = conversation;

    // 标题还是临时的，就在回答的同时给对话起名。
    let naming = title::start(control, ctx, &chat, &resolved, &req.content);

    let conclusion = {
        let run = agent_loop.run_with_sink(
            &sink,
            &tool_ctx,
            &resolved.model,
            messages,
            Some(tools_schema),
            resolved.max_tokens,
            initial_tool_choice,
        );

        tokio::pin!(run);

        // 模型停顿时攒着的文字也按时写掉。
        let mut ticker = tokio::time::interval(Duration::from_millis(TEXT_FLUSH_MILLIS / 2));

        loop {
            tokio::select! {
                conclusion = &mut run => break conclusion,
                _ = ticker.tick() => sink.flush_if_due().await,
            }
        }
    };

    sink.flush().await;

    let landing = Landing {
        chat: &chat,
        ctx,
        message_id,
        asked_at,
        prompt_meta: &prompt_meta,
        recorder: &recorder,
    };

    let ending = land(control, landing, conclusion, sink.into_record()).await;
    title::settle(control, naming).await;
    ending
}

/// 写看板时这一轮怎么开始，以及给模型的限制说明。
async fn dashboard_choice(
    state: &AppState,
    resolution: &toolsets::ToolsetResolution,
    req: &super::super::PostMessageReq,
    dashboard: Option<bool>,
    system_texts: &mut Vec<String>,
) -> Result<ToolChoice> {
    let Some(input_complete) = dashboard else {
        return Ok(ToolChoice::Auto);
    };

    let enabled_tools = builtin_tools()
        .into_iter()
        .filter(|tool| tool.exposure.mole_agent && resolution.builtin_enabled(&tool.name))
        .map(|tool| tool.name)
        .collect::<Vec<_>>();

    let compatibility = dashboard_authoring::validate_compatibility(
        enabled_tools,
        &state
            .agent
            .dashboard_authoring
            .capabilities()
            .await?
            .authoring_versions,
    )?;

    if compatibility.preview_only
        || !req
            .execution_policy
            .unwrap_or_default()
            .allows_approval_request()
    {
        system_texts.push(
            "Dashboard creation proposals are unavailable in this chat. You may prepare and present a preview, but must stop before proposing creation and clearly explain that limitation."
                .into(),
        );
    }

    Ok(if input_complete {
        ToolChoice::Specific("prepare_dashboard".into())
    } else {
        ToolChoice::Auto
    })
}

/// 落回答要用的东西。
struct Landing<'a> {
    chat: &'a Chat,
    ctx: &'a IamContext,
    message_id: Id,
    asked_at: Option<TimestampMicros>,
    prompt_meta: &'a PromptMeta,
    recorder: &'a Arc<UsageRecorder>,
}

/// 回答排在它的问题后面：同一会话里排队的问题在它之后才提的，也排在它之后。
fn answer_time(asked_at: Option<TimestampMicros>) -> TimestampMicros {
    asked_at
        .map(|at| TimestampMicros(at.0 + 1))
        .unwrap_or_else(TimestampMicros::now)
}

async fn land(
    control: &RunControl,
    landing: Landing<'_>,
    conclusion: RunConclusion,
    record: Record,
) -> Ending {
    let state = &control.state;
    let totals = landing.recorder.totals();
    let cost = landing.recorder.cost();

    let (status, content, finish_reason, error) = match conclusion {
        RunConclusion::Answered {
            content,
            finish_reason,
            ..
        } => (RunStatus::Succeeded, content, Some(finish_reason), None),
        RunConclusion::Failed(error) => (
            RunStatus::Failed,
            format!("[error: {error}]"),
            None,
            Some(error),
        ),
        RunConclusion::Cancelled => (RunStatus::Cancelled, record.pending_text, None, None),
    };

    // 停下时什么都还没写、也没调用工具，就不留一条空回答。
    let keep =
        status != RunStatus::Cancelled || !content.trim().is_empty() || !record.steps.is_empty();

    let mut message_id = None;

    if keep {
        let message = ChatMessage {
            prompt_tokens: Some(totals.prompt_tokens),
            completion_tokens: Some(totals.completion_tokens),
            cost,
            prompt_template_id: landing.prompt_meta.id.clone(),
            prompt_builtin_key: landing.prompt_meta.builtin_key.clone(),
            prompt_version: landing.prompt_meta.version,
            prompt_hash: landing.prompt_meta.hash.clone(),
            evidence_json: (!record.evidence.is_empty()).then(|| json!(record.evidence)),
            run_id: Some(control.run.id.clone()),
            tool_transcript: (!record.steps.is_empty())
                .then(|| serde_json::to_value(&record.steps).unwrap_or(Value::Null)),
            ..ChatMessage::minimal(
                landing.message_id.clone(),
                landing.chat.id.clone(),
                landing.ctx.org_id.clone(),
                "assistant",
                content.clone(),
                answer_time(landing.asked_at),
            )
        };

        match state.agent.chats.append_message(message).await {
            Ok(saved) => message_id = Some(saved.id),
            Err(error) => {
                tracing::warn!(chat_id = %landing.chat.id.0, %error, "an answer could not be saved");
            }
        }
    }

    let _ = state
        .agent
        .chats
        .touch_chat(&landing.chat.id, TimestampMicros::now())
        .await;

    Ending {
        status,
        message_id,
        content: Some(content).filter(|_| status == RunStatus::Succeeded),
        prompt_tokens: totals.prompt_tokens,
        completion_tokens: totals.completion_tokens,
        cost,
        finish_reason,
        error,
        code: None,
    }
}

/// 还没开始调用模型就失败：落一条说明，和以前一样。
pub(super) async fn failure_message(
    control: &RunControl,
    ctx: &IamContext,
    chat: &Chat,
    asked_at: Option<TimestampMicros>,
    error: String,
) -> Ending {
    let message = ChatMessage {
        run_id: Some(control.run.id.clone()),
        ..ChatMessage::minimal(
            Id::new(),
            chat.id.clone(),
            ctx.org_id.clone(),
            "assistant",
            format!("[error: {error}]"),
            answer_time(asked_at),
        )
    };

    let message_id = control
        .state
        .agent
        .chats
        .append_message(message)
        .await
        .ok()
        .map(|message| message.id);

    Ending {
        message_id,
        ..Ending::failed(error)
    }
}
