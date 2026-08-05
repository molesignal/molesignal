// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence chat HTTP 路由。
//!
//! - `GET    /api/v1/intelligence/chat`               列出当前用户活跃会话
//! - `POST   /api/v1/intelligence/chat`               新建会话
//! - `GET    /api/v1/intelligence/chat/{id}/messages` 拉历史
//! - `POST   /api/v1/intelligence/chat/{id}/messages` 发用户消息 + SSE 流回 Mole Agent
//! - `POST   /api/v1/intelligence/chat/{id}/archive`  归档 transcript 到对象存储
//! - `DELETE /api/v1/intelligence/chat/{id}`          归档后软删会话
//!
//! Mole Agent 分析能力：
//! - provider 从 PG `intelligence_model_providers` 行 + 加密 key 构造 adapter（dev 缺省 env fallback）。
//! - prompt 按 purpose 解析（user → org → builtin 默认），渲染白名单变量，持久化 id/version/key/hash。
//! - tool 调用走 `RealToolDispatcher`，org/user 身份恒取自 `IamContext`（忽略 model 传的 org_id）。
//! - tool 证据落 `intelligence_messages.evidence_json`，大原始结果 spill 到对象存储。
//! - transcript 归档：对象存储 JSON + PG 元数据（object_key/sha256/bytes/status）+ 审计事件。
//!
//! `license.has_feature("intelligence")`；OSS 运行时拒绝。

use std::{collections::HashMap, sync::Arc};

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{Path, State},
    http::header::CONTENT_TYPE,
    response::Response,
    routing::get,
};
use futures::stream;
use object_store::{ObjectStoreExt, PutPayload, path::Path as ObjPath};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{tool_dispatcher::RealToolDispatcher, toolsets};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::{
        iam::permission,
        ingestion::{IngestBatch, RawEvent},
        stream::StreamType,
    },
    infra::persistence::repositories::intelligence::{
        chats::{Chat, ChatMessage},
        prompts::{prompt_hash, render_prompt},
    },
    intelligence::{
        FEATURE,
        capabilities::dashboard_authoring,
        chat::{
            AgentLoop, AgentStreamEvent, ChatMessage as ExternalChatMessage, MessageRole, Provider,
            ProviderAdapter, ToolChoice, adapter_from_env, adapter_from_parts,
        },
        tools::{AgentExecutionPolicy, ToolAuthContext, ToolDispatcher, builtin_tools},
    },
    shared::{
        Error, Result, ids::Id, time::TimestampMicros, trace_stream::segmented_result_stream,
    },
};

/// 单条 tool 原始结果内联上限（字节）；超此值 spill 到对象存储，evidence 只留 object_key + 摘要。
const INLINE_TOOL_RESULT_LIMIT: usize = 16 * 1024;
/// evidence 摘要文本上限。
const EVIDENCE_SUMMARY_CAP: usize = 500;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/intelligence/chat", get(list_chats).post(create_chat))
        .route(
            "/intelligence/chat/{id}/messages",
            get(list_messages).post(post_message),
        )
        .route(
            "/intelligence/chat/{id}/archive",
            axum::routing::post(archive_chat_route),
        )
        .route(
            "/intelligence/chat/{id}",
            axum::routing::delete(delete_chat),
        )
}

fn require_license(state: &AppState) -> Result<()> {
    if !state.platform.license.has_feature(FEATURE) {
        return Err(Error::forbidden(format!("{FEATURE} feature not licensed")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Chat CRUD
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateChatReq {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub title: String,
    /// 可选：绑定 PG provider 行 id（区别于 provider 类型串）。
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub analysis_mode: Option<String>,
    #[serde(default)]
    pub capability: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatResp {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub title: String,
    pub provider_id: Option<String>,
    pub analysis_mode: Option<String>,
    pub capability: Option<String>,
    pub time_range_start_micros: Option<i64>,
    pub time_range_end_micros: Option<i64>,
    pub archive_object_key: Option<String>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

#[derive(Debug, Serialize)]
pub struct ChatMessageResp {
    pub id: String,
    pub chat_id: String,
    pub org_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls_json: Option<Value>,
    pub tool_result_for: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
    pub prompt_template_id: Option<String>,
    pub prompt_builtin_key: Option<String>,
    pub prompt_version: Option<i32>,
    pub prompt_hash: Option<String>,
    pub evidence_json: Option<Value>,
    pub created_at_micros: i64,
}

fn to_chat_resp(s: Chat) -> ChatResp {
    let capability = (s.analysis_mode.as_deref() == Some("dashboard"))
        .then(|| "dashboard_authoring".to_string());
    ChatResp {
        id: s.id.0,
        provider: s.provider,
        model: s.model,
        title: s.title,
        provider_id: s.provider_id,
        analysis_mode: s.analysis_mode,
        capability,
        time_range_start_micros: s.time_range_start_micros,
        time_range_end_micros: s.time_range_end_micros,
        archive_object_key: s.archive_object_key,
        created_at_micros: s.created_at.0,
        updated_at_micros: s.updated_at.0,
    }
}

fn to_message_resp(message: ChatMessage) -> ChatMessageResp {
    ChatMessageResp {
        id: message.id.0,
        chat_id: message.chat_id.0,
        org_id: message.org_id.0,
        role: message.role,
        content: message.content,
        tool_calls_json: message.tool_calls_json,
        tool_result_for: message.tool_result_for,
        prompt_tokens: message.prompt_tokens,
        completion_tokens: message.completion_tokens,
        cost_usd: message.cost_usd,
        prompt_template_id: message.prompt_template_id,
        prompt_builtin_key: message.prompt_builtin_key,
        prompt_version: message.prompt_version,
        prompt_hash: message.prompt_hash,
        evidence_json: message.evidence_json,
        created_at_micros: message.created_at.0,
    }
}

#[permission("intelligence.use")]
async fn list_chats(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ChatResp>>> {
    require_license(&state)?;
    Ok(Json(
        state
            .intelligence
            .chats
            .list_chats(&ctx.org_id, &ctx.user_id)
            .await?
            .into_iter()
            .map(to_chat_resp)
            .collect(),
    ))
}

#[permission("intelligence.use")]
async fn create_chat(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateChatReq>,
) -> Result<Json<ChatResp>> {
    require_license(&state)?;
    let now = TimestampMicros::now();
    let mut s = Chat::minimal(
        Id::new(),
        ctx.org_id.clone(),
        ctx.user_id.clone(),
        req.provider,
        req.model,
        req.title,
        now,
    );
    s.provider_id = req.provider_id;
    s.analysis_mode = match req.capability.as_deref() {
        Some("dashboard_authoring") => Some("dashboard".into()),
        Some(other) => {
            return Err(Error::invalid(format!(
                "unknown Mole Agent capability `{other}`"
            )));
        }
        None => req.analysis_mode,
    };
    Ok(Json(to_chat_resp(
        state.intelligence.chats.create_chat(s).await?,
    )))
}

#[permission("intelligence.use")]
async fn delete_chat(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    // 软删前先尽力归档，保证删除的会话仍有 transcript 留档（best-effort）。
    if let Ok(chat) = state
        .intelligence
        .chats
        .get_chat(&ctx.org_id, &Id(id.clone()))
        .await
    {
        let _ = archive_chat(&state, &ctx, &chat).await;
    }
    state
        .intelligence
        .chats
        .delete_chat(&ctx.org_id, &Id(id))
        .await?;
    Ok(Json(json!({ "deleted": true })))
}

#[permission("intelligence.use")]
async fn list_messages(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let _ = state
        .intelligence
        .chats
        .get_chat(&ctx.org_id, &Id(id.clone()))
        .await?;
    let msgs = state.intelligence.chats.list_messages(&Id(id)).await?;
    Ok(Json(json!({
        "messages": msgs.into_iter().map(to_message_resp).collect::<Vec<_>>()
    })))
}

// ---------------------------------------------------------------------------
// Post message (provider/prompt resolution + tool dispatch + evidence + SSE)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TimeRangeReq {
    pub start_micros: i64,
    pub end_micros: i64,
}

#[derive(Debug, Deserialize)]
pub struct PostMessageReq {
    pub content: String,
    #[serde(default)]
    pub regenerate_from_message_id: Option<String>,
    #[serde(default)]
    pub investigation_id: Option<String>,
    #[serde(default)]
    pub time_range: Option<TimeRangeReq>,
    #[serde(default)]
    pub analysis_mode: Option<String>,
    #[serde(default)]
    pub capability: Option<String>,
    #[serde(default)]
    pub execution_policy: Option<AgentExecutionPolicy>,
    #[serde(default)]
    pub stream_hints: Vec<String>,
    #[serde(default)]
    pub agent_profile_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt_template_id: Option<String>,
    /// 客户端传 prompt token 数（legacy cost hint）。
    #[serde(default)]
    pub prompt_tokens: Option<i64>,
    #[serde(default)]
    pub completion_tokens: Option<i64>,
}

/// 解析出的 provider adapter + 运行参数。
struct ResolvedProvider {
    model: String,
    adapter: Arc<dyn ProviderAdapter>,
    max_tokens: Option<i32>,
    provider: Provider,
    provider_id: Option<String>,
}

/// 解析出的活跃 prompt 元数据（持久化到 assistant 消息）。
#[derive(Clone, Default)]
struct PromptMeta {
    id: Option<String>,
    builtin_key: Option<String>,
    version: Option<i32>,
    hash: Option<String>,
}

#[permission("intelligence.use")]
async fn post_message(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(chat_id): Path<String>,
    Json(req): Json<PostMessageReq>,
) -> Result<Response> {
    require_license(&state)?;
    let chat = state
        .intelligence
        .chats
        .get_chat(&ctx.org_id, &Id(chat_id.clone()))
        .await?;
    let dashboard_activation = dashboard_authoring::resolve(
        req.capability.as_deref(),
        req.analysis_mode.as_deref(),
        chat.analysis_mode.as_deref(),
        &req.content,
        req.time_range.is_some(),
    )?;
    let effective_analysis_mode = if dashboard_activation.is_some() {
        Some("dashboard")
    } else {
        req.analysis_mode.as_deref()
    };
    let now = TimestampMicros::now();

    // 1) 普通发送落 user 消息；重新生成则复用指定 user 消息，并截断传给模型的
    // 历史到该问题，避免把上一版回答当作下一轮输入。
    let history_rows = if let Some(message_id) = req.regenerate_from_message_id.as_deref() {
        regeneration_history(
            state.intelligence.chats.list_messages(&chat.id).await?,
            message_id,
            &req.content,
        )?
    } else {
        state
            .intelligence
            .chats
            .append_message(ChatMessage {
                prompt_tokens: req.prompt_tokens,
                ..ChatMessage::minimal(
                    Id::new(),
                    chat.id.clone(),
                    ctx.org_id.clone(),
                    "user",
                    req.content.clone(),
                    now,
                )
            })
            .await?;
        state.intelligence.chats.list_messages(&chat.id).await?
    };
    let _ = state
        .intelligence
        .chats
        .set_chat_context(
            &chat.id,
            effective_analysis_mode,
            req.time_range.as_ref().map(|t| t.start_micros),
            req.time_range.as_ref().map(|t| t.end_micros),
        )
        .await;

    // 2) 解析 provider adapter（PG 行 → env fallback）。失败 → SSE error。
    let resolved = match resolve_provider(
        &state,
        &ctx,
        &chat,
        req.provider_id.as_deref(),
        req.model.as_deref(),
    )
    .await
    {
        Ok(r) => r,
        Err(e) => return error_stream_response(&state, &ctx, &chat, e.to_string()).await,
    };
    let mut effective_chat = chat.clone();
    effective_chat.provider = resolved.provider.as_str().to_string();
    effective_chat.model = resolved.model.clone();
    effective_chat.provider_id = resolved.provider_id.clone();
    if dashboard_activation.is_some() {
        effective_chat.analysis_mode = Some("dashboard".into());
    }
    let _ = state
        .intelligence
        .chats
        .set_chat_provider(
            &effective_chat.id,
            &effective_chat.provider,
            &effective_chat.model,
            effective_chat.provider_id.as_deref(),
        )
        .await;

    // 3) 解析 + 渲染 prompt（system + 任务），收集 system 消息 + 活跃 prompt 元数据。
    let (mut system_texts, prompt_meta) = resolve_prompts(
        &state,
        &ctx,
        &effective_chat,
        &req,
        resolved.provider,
        dashboard_activation.is_some(),
    )
    .await;

    // 5) tool dispatch：org/user 身份恒取自 IamContext（忽略 model 传入的身份字段）。
    //    默认 Agent Profile 与组织级工具集都只能进一步收窄编译期内置白名单。
    let requested_profile_id = req.agent_profile_id.as_deref().map(|id| Id(id.to_string()));
    let resolution =
        toolsets::resolve_toolsets_for_profile(&state, &ctx.org_id, requested_profile_id.as_ref())
            .await?;
    let initial_tool_choice = if let Some(activation) = dashboard_activation {
        let enabled_tools = builtin_tools()
            .into_iter()
            .filter(|tool| resolution.builtin_enabled(&tool.name))
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        let compatibility = dashboard_authoring::validate_compatibility(
            enabled_tools,
            &state
                .intelligence
                .dashboard_authoring
                .capabilities()
                .await?
                .authoring_versions,
        )?;
        if compatibility.preview_only
            || req.execution_policy.unwrap_or_default() != AgentExecutionPolicy::Policy
        {
            system_texts.push(
                "Dashboard creation proposals are unavailable in this chat. You may prepare and present a preview, but must stop before proposing creation and clearly explain that limitation."
                    .into(),
            );
        }
        if activation.input_complete {
            ToolChoice::Specific("prepare_dashboard".into())
        } else {
            ToolChoice::Auto
        }
    } else {
        ToolChoice::Auto
    };

    // 4) 历史 → external；前置 system/capability 消息。
    let mut history: Vec<ExternalChatMessage> =
        system_texts.into_iter().map(system_message).collect();
    history.extend(history_to_external(&history_rows));
    let dispatcher: Arc<dyn ToolDispatcher> =
        Arc::new(RealToolDispatcher::new(state.clone()).with_toolsets(resolution.clone()));
    let agent_loop = Arc::new(AgentLoop::new(resolved.adapter.clone(), dispatcher));
    let tool_ctx = ToolAuthContext {
        user_id: ctx.user_id.0.clone(),
        org_id: ctx.org_id.0.clone(),
        chat_id: Some(chat.id.0.clone()),
        investigation_id: req.investigation_id.clone(),
        execution_policy: req.execution_policy.unwrap_or_default(),
        query_generation_only: req.analysis_mode.as_deref() == Some("query_generation"),
    };
    let tools_schema = tools_schema_for(resolved.provider, &resolution);
    let event_stream = agent_loop.clone().run_stream_with_tool_choice(
        tool_ctx,
        resolved.model.clone(),
        history,
        Some(tools_schema),
        resolved.max_tokens,
        initial_tool_choice,
    );

    // 6) Tee：每个 AgentStreamEvent → SSE 字节给客户端；同时累积 final content / token /
    //    错误 / tool evidence，流结束后落 assistant row + 审计 + intelligence_model_traces。
    let (sse_tx, sse_rx) = tokio::sync::mpsc::channel::<
        std::result::Result<bytes::Bytes, std::convert::Infallible>,
    >(64);
    let persist_state = state.clone();
    let persist_ctx = ctx.clone();
    let persist_chat = effective_chat.clone();
    let assistant_id = Id::new();
    let resolved_provider_id = resolved.provider_id.clone();
    crate::shared::trace_context::spawn_with_current_trace_context(async move {
        use futures::StreamExt;
        let mut events = event_stream;
        let mut final_content = String::new();
        let mut prompt_tokens: i32 = 0;
        let mut completion_tokens: i32 = 0;
        let mut error_msg: Option<String> = None;
        // tool_call_id → (name, arguments)
        let mut tool_args: HashMap<String, (String, Value)> = HashMap::new();
        let mut evidence: Vec<Value> = Vec::new();

        while let Some(evt) = events.next().await {
            let line = match &evt {
                AgentStreamEvent::Chunk(t) => {
                    final_content.push_str(t);
                    format!("event: chunk\ndata: {}\n\n", json!({ "text": t }))
                }
                AgentStreamEvent::ToolStart {
                    id,
                    name,
                    arguments,
                } => {
                    let args: Value =
                        serde_json::from_str(arguments).unwrap_or(Value::String(arguments.clone()));
                    tool_args.insert(id.clone(), (name.clone(), args.clone()));
                    format!(
                        "event: tool_start\ndata: {}\n\n",
                        json!({ "id": id, "name": name, "arguments": arguments })
                    )
                }
                AgentStreamEvent::ToolEnd {
                    id,
                    result,
                    is_error,
                } => {
                    let (name, args) = tool_args
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| ("unknown".into(), Value::Null));
                    let entry = build_evidence(
                        &persist_state,
                        &persist_ctx.org_id,
                        &persist_chat.id,
                        &assistant_id,
                        id,
                        &name,
                        args,
                        result,
                        *is_error,
                    )
                    .await;
                    // 审计只记录摘要，不写原始可观测数据。
                    activity_audit::record(
                        &persist_state,
                        &persist_ctx,
                        "intelligence.tool.called",
                        "intelligence_chat",
                        &persist_chat.id.0,
                        json!({
                            "tool": name,
                            "status": if *is_error { "error" } else { "success" },
                            "row_count": entry.get("row_count").cloned().unwrap_or(Value::Null),
                            "object_key": entry.get("object_key").cloned().unwrap_or(Value::Null),
                        }),
                    )
                    .await;
                    evidence.push(entry);
                    format!(
                        "event: tool_end\ndata: {}\n\n",
                        json!({ "id": id, "result": result, "is_error": is_error })
                    )
                }
                AgentStreamEvent::Done {
                    content,
                    prompt_tokens: pt,
                    completion_tokens: ct,
                    finish_reason,
                } => {
                    if !content.is_empty() {
                        final_content = content.clone();
                    }
                    prompt_tokens = *pt;
                    completion_tokens = *ct;
                    format!(
                        "event: done\ndata: {}\n\n",
                        json!({
                            "prompt_tokens": pt,
                            "completion_tokens": ct,
                            "finish_reason": finish_reason,
                        })
                    )
                }
                AgentStreamEvent::Error(msg) => {
                    error_msg = Some(msg.clone());
                    format!("event: error\ndata: {}\n\n", json!({ "message": msg }))
                }
            };
            if sse_tx
                .send(Ok::<_, std::convert::Infallible>(bytes::Bytes::from(line)))
                .await
                .is_err()
            {
                tracing::debug!("sse client disconnected; aborting chat persistence pipe");
                return;
            }
        }

        // 7) 终结：落 assistant row（prompt 元数据 + evidence）+ cost。
        let price = persist_state
            .platform
            .model_prices
            .get(&persist_chat.provider, &persist_chat.model)
            .await
            .ok()
            .flatten();
        let cost = price.as_ref().map(|p| {
            crate::infra::persistence::repositories::model_prices::compute_cost_usd(
                p,
                prompt_tokens as i64,
                completion_tokens as i64,
            )
        });
        if price.is_none() {
            tracing::warn!(
                provider = %persist_chat.provider,
                model = %persist_chat.model,
                "model_prices missed; leaving cost_usd NULL"
            );
        }
        let assistant_content = match &error_msg {
            Some(err) => format!("[error: {err}]"),
            None => final_content.clone(),
        };
        let evidence_json = if evidence.is_empty() {
            None
        } else {
            Some(json!(evidence))
        };
        let _ = persist_state
            .intelligence
            .chats
            .append_message(ChatMessage {
                prompt_tokens: Some(prompt_tokens as i64),
                completion_tokens: Some(completion_tokens as i64),
                cost_usd: cost,
                prompt_template_id: prompt_meta.id.clone(),
                prompt_builtin_key: prompt_meta.builtin_key.clone(),
                prompt_version: prompt_meta.version,
                prompt_hash: prompt_meta.hash.clone(),
                evidence_json,
                ..ChatMessage::minimal(
                    assistant_id.clone(),
                    persist_chat.id.clone(),
                    persist_ctx.org_id.clone(),
                    "assistant",
                    assistant_content,
                    TimestampMicros::now(),
                )
            })
            .await;
        let _ = persist_state
            .intelligence
            .chats
            .touch_chat(&persist_chat.id, TimestampMicros::now())
            .await;

        // 8) intelligence_model_traces event（best-effort）。
        // NB: 不要手动塞 `_timestamp`——RawEvent.timestamp 已经生成系统 `_timestamp`
        // (timestamp 类型)列；再塞一个 i64 `_timestamp` 会导致列重复 + 类型冲突。
        // 字段名也不能带点（SQL 里点是限定符分隔符），用下划线。
        let trace_event = json!({
            "chat_id": persist_chat.id.0,
            "user_id": persist_ctx.user_id.0,
            "provider": persist_chat.provider,
            "provider_id": resolved_provider_id,
            "model": persist_chat.model,
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "cost_usd": cost,
            "error": error_msg,
            "tool_calls": evidence.len(),
            "gen_ai_system": persist_chat.provider,
            "gen_ai_request_model": persist_chat.model,
        });
        let _ = persist_state
            .ingestion
            .ingest_internal_telemetry(IngestBatch {
                batch_id: Id::new(),
                org_id: persist_ctx.org_id.clone(),
                stream: "_intelligence_model_traces".into(),
                stream_type: StreamType::Logs,
                events: vec![RawEvent {
                    timestamp: TimestampMicros::now(),
                    fields: trace_event.as_object().cloned().unwrap_or_default(),
                }],
                received_at: TimestampMicros::now(),
            })
            .await;
    });

    let body_stream = segmented_result_stream(
        tokio_stream::wrappers::ReceiverStream::new(sse_rx),
        "intelligence.http.sse",
        "sse",
    );
    Ok(Response::builder()
        .status(200)
        .header(CONTENT_TYPE, "text/event-stream")
        .header("cache-control", "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(body_stream))
        .unwrap_or_else(|_| Response::new(Body::empty())))
}

fn regeneration_history(
    mut rows: Vec<ChatMessage>,
    message_id: &str,
    content: &str,
) -> Result<Vec<ChatMessage>> {
    let position = rows
        .iter()
        .position(|message| message.id.0 == message_id && message.role == "user")
        .ok_or_else(|| Error::invalid("regenerate target must be a user message in this chat"))?;
    let original = &rows[position];
    if original.content.trim() != content.trim() {
        return Err(Error::invalid(
            "regenerate content must match the original user message",
        ));
    }
    rows.truncate(position + 1);
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Provider resolution
// ---------------------------------------------------------------------------

/// 解析顺序：显式 provider_id → 会话绑定 provider_id → org 首个 enabled provider →
/// env fallback（仅 dev：env key 配齐时才成功）。
async fn resolve_provider(
    state: &AppState,
    ctx: &IamContext,
    chat: &Chat,
    explicit_provider_id: Option<&str>,
    explicit_model: Option<&str>,
) -> Result<ResolvedProvider> {
    // 1) 找 provider 行。
    let row = if let Some(pid) = explicit_provider_id {
        Some(
            state
                .intelligence
                .model_providers
                .get(&ctx.org_id, &Id(pid.to_string()))
                .await?,
        )
    } else if let Some(pid) = chat.provider_id.as_deref() {
        state
            .intelligence
            .model_providers
            .get(&ctx.org_id, &Id(pid.to_string()))
            .await
            .ok()
    } else {
        state
            .intelligence
            .model_providers
            .list(&ctx.org_id)
            .await?
            .into_iter()
            .find(|p| p.enabled)
    };

    if let Some(p) = row {
        if !p.enabled {
            return Err(Error::invalid(format!(
                "model provider `{}` is disabled",
                p.id.0
            )));
        }
        let provider = Provider::parse(&p.provider)?;
        let key = state
            .intelligence
            .model_providers
            .get_plaintext_key(&ctx.org_id, &p.id)
            .await?
            .ok_or_else(|| Error::invalid(format!("model provider `{}` has no API key", p.id.0)))?;
        let adapter = adapter_from_parts(provider, p.base_url.clone(), key)?;
        let model = explicit_model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if chat.model.is_empty() {
                    p.default_model.clone()
                } else {
                    chat.model.clone()
                }
            });
        return Ok(ResolvedProvider {
            model,
            adapter,
            max_tokens: p.max_tokens.map(|m| m as i32),
            provider,
            provider_id: Some(p.id.0),
        });
    }

    // 2) env fallback（dev）。
    let provider = Provider::parse(&chat.provider)?;
    let adapter = adapter_from_env(provider)?;
    let model = explicit_model
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(chat.model.as_str())
        .to_string();
    Ok(ResolvedProvider {
        model,
        adapter,
        max_tokens: None,
        provider,
        provider_id: None,
    })
}

// ---------------------------------------------------------------------------
// Prompt resolution + rendering
// ---------------------------------------------------------------------------

/// analysis_mode → prompt purpose；自由问答返 None（只用 system prompt）。
fn purpose_for_mode(mode: &str) -> Option<&'static str> {
    match mode {
        "anomaly" | "anomaly_analysis" => Some("anomaly_analysis"),
        "root_cause" | "rootcause" => Some("root_cause"),
        "alert" | "alert_explain" => Some("alert_explain"),
        "query" | "query_generation" => Some("query_generation"),
        "dashboard" | "dashboard_authoring" => Some("dashboard_authoring"),
        _ => None,
    }
}

fn prompt_readable(
    t: &crate::infra::persistence::repositories::intelligence::prompts::AgentPromptTemplate,
    ctx: &IamContext,
) -> bool {
    match t.scope.as_str() {
        "builtin" => true,
        "org" => t.org_id.as_deref() == Some(ctx.org_id.0.as_str()),
        "user" => {
            t.org_id.as_deref() == Some(ctx.org_id.0.as_str())
                && t.user_id.as_deref() == Some(ctx.user_id.0.as_str())
        }
        _ => false,
    }
}

/// 解析 system + 任务 prompt，渲染白名单变量，返回 (system_texts, 活跃 prompt 元数据)。
/// 全程容错：解析/渲染失败不阻断 chat（返回已收集的部分）。
async fn resolve_prompts(
    state: &AppState,
    ctx: &IamContext,
    chat: &Chat,
    req: &PostMessageReq,
    _provider: Provider,
    dashboard_capability_active: bool,
) -> (Vec<String>, PromptMeta) {
    let now = TimestampMicros::now();
    let org_name = state
        .iam
        .service
        .orgs
        .get(&ctx.org_id)
        .await
        .map(|o| o.name)
        .unwrap_or_else(|_| ctx.org_id.0.clone());
    let current_time = now.to_datetime().to_rfc3339();
    let time_range_str = match &req.time_range {
        Some(t) => format!(
            "{} to {}",
            TimestampMicros(t.start_micros).to_datetime().to_rfc3339(),
            TimestampMicros(t.end_micros).to_datetime().to_rfc3339(),
        ),
        None => "unspecified".into(),
    };
    let streams_str = if req.stream_hints.is_empty() {
        "all available streams".to_string()
    } else {
        req.stream_hints.join(", ")
    };

    let mut system_vars = Map::new();
    system_vars.insert("org_name".into(), Value::String(org_name));
    system_vars.insert("current_time".into(), Value::String(current_time));

    let mut task_vars = Map::new();
    task_vars.insert("time_range".into(), Value::String(time_range_str));
    task_vars.insert("streams".into(), Value::String(streams_str));
    task_vars.insert("alert_name".into(), Value::String(String::new()));

    let mut system_texts = Vec::new();
    let mut active = PromptMeta::default();

    // system prompt（always）。
    if let Ok(sys) = state
        .intelligence
        .prompts
        .resolve(&ctx.org_id, &ctx.user_id, "system")
        .await
    {
        let rendered = render_prompt(&sys.body, &system_vars);
        active = PromptMeta {
            id: Some(sys.id.0.clone()),
            builtin_key: sys.builtin_key.clone(),
            version: Some(sys.version),
            hash: Some(prompt_hash(&rendered)),
        };
        system_texts.push(rendered);
    }
    system_texts.push(
        match req.execution_policy.unwrap_or_default() {
            AgentExecutionPolicy::AdviceOnly => {
                "The user selected Advice only. You may use read-only investigation tools, but do not create approval requests. Describe any proposed operation without invoking a write-capable tool."
            }
            AgentExecutionPolicy::ReadOnly => {
                "The user selected Auto-run read-only. Run authorized read-only investigation tools as needed, but do not create approval requests or invoke write-capable tools."
            }
            AgentExecutionPolicy::Policy => {
                "The user selected Policy approval. Read-only tools may run automatically. Any state-changing operation must only be proposed through the approval workflow and must never be executed directly."
            }
        }
        .to_string(),
    );
    if req.analysis_mode.as_deref() == Some("query_generation") {
        system_texts.push(
            "The user selected Generate query only. Generate the query text, but do not call tools or execute the query."
                .to_string(),
        );
    } else {
        system_texts.push(observability_query_instruction().to_string());
    }

    // 任务 prompt：显式 id → analysis_mode purpose → 无。
    let task = if dashboard_capability_active {
        state
            .intelligence
            .prompts
            .resolve(&ctx.org_id, &ctx.user_id, "dashboard_authoring")
            .await
            .ok()
    } else if let Some(pid) = req.prompt_template_id.as_deref() {
        match state.intelligence.prompts.get(&Id(pid.to_string())).await {
            Ok(t) if prompt_readable(&t, ctx) => Some(t),
            _ => None,
        }
    } else if let Some(purpose) = req
        .analysis_mode
        .as_deref()
        .or(chat.analysis_mode.as_deref())
        .and_then(purpose_for_mode)
    {
        state
            .intelligence
            .prompts
            .resolve(&ctx.org_id, &ctx.user_id, purpose)
            .await
            .ok()
    } else {
        None
    };

    if let Some(t) = task {
        let rendered = render_prompt(&t.body, &task_vars);
        active = PromptMeta {
            id: Some(t.id.0.clone()),
            builtin_key: t.builtin_key.clone(),
            version: Some(t.version),
            hash: Some(prompt_hash(&rendered)),
        };
        system_texts.push(rendered);
    } else if dashboard_capability_active {
        let instruction = dashboard_authoring::INSTRUCTIONS.to_string();
        active = PromptMeta {
            id: None,
            builtin_key: Some("dashboard.authoring.v1".into()),
            version: Some(1),
            hash: Some(prompt_hash(&instruction)),
        };
        system_texts.push(instruction);
    }
    system_texts.push(
        answer_presentation_instruction(req.analysis_mode.as_deref() == Some("query_generation"))
            .to_string(),
    );

    (system_texts, active)
}

fn answer_presentation_instruction(query_generation_only: bool) -> &'static str {
    if query_generation_only {
        return "Write only the requested query plus a short explanation of what it returns. Do not expose internal tool names, function names, parameter names, data-stream discovery attempts, hidden reasoning, or implementation failures.";
    }
    r#"Your final response is a product-facing operations result, not a debug trace. Never expose internal tool/function names, raw parameter names, hidden reasoning, data-stream discovery attempts, or implementation failures. Do not narrate "I first tried..." or similar process language. Return one JSON object without a Markdown fence using this shape: {"summary":"concise conclusion","evidence":[{"label":"user-facing fact","kind":"logs|metrics|trace|alert|schedule|other","route":"optional in-app route"}],"likely_causes":["optional cause"],"limitations":["what could not be confirmed"],"suggested_next_steps":["clear next action"],"related_links":[{"label":"action label","route":"optional in-app route"}],"confidence":"high|medium|low"}. Keep empty sections as empty arrays. Tool execution transparency is rendered separately by the product from recorded evidence."#
}

fn observability_query_instruction() -> &'static str {
    r#"When investigating observability data, inspect list_streams before the first log or metric query unless the exact current schema is already present in this chat. SQL must reference only exact fields returned for the selected stream. The query_logs time_range argument already constrains event time, so never invent timestamp, time, or _timestamp fields and never order by one unless that exact field exists in the schema. query_metrics accepts PromQL only; label_values() is a Grafana template helper and is invalid. Do not repeat equivalent empty queries. After at most one corrected retry for a failed query, stop querying that path, explain the limitation in the final answer, and use the evidence already collected to produce a conclusion."#
}

// ---------------------------------------------------------------------------
// Tool evidence
// ---------------------------------------------------------------------------

/// 构造一条 tool evidence；result 过大时 spill 原始 JSON 到对象存储，evidence 只留 object_key。
#[allow(clippy::too_many_arguments)]
async fn build_evidence(
    state: &AppState,
    org_id: &Id,
    chat_id: &Id,
    message_id: &Id,
    tool_call_id: &str,
    name: &str,
    args: Value,
    result_json: &str,
    is_error: bool,
) -> Value {
    let (row_count, scanned_rows, took_ms, summary) = summarize_tool_result(result_json);
    let mut entry = json!({
        "tool_call_id": tool_call_id,
        "tool": name,
        "arguments": args,
        "status": if is_error { "error" } else { "success" },
        "summary": summary,
    });
    if let Some(v) = row_count {
        entry["row_count"] = json!(v);
    }
    if let Some(v) = scanned_rows {
        entry["scanned_rows"] = json!(v);
    }
    if let Some(v) = took_ms {
        entry["took_ms"] = json!(v);
    }

    if result_json.len() > INLINE_TOOL_RESULT_LIMIT {
        let object_key = format!(
            "intelligence/chat/{}/{}/tool-results/{}/{}.json",
            org_id.0, chat_id.0, message_id.0, tool_call_id
        );
        match ObjPath::parse(&object_key) {
            Ok(path) => {
                match state
                    .storage
                    .object_store
                    .put(&path, PutPayload::from(result_json.as_bytes().to_vec()))
                    .await
                {
                    Ok(_) => {
                        entry["object_key"] = json!(object_key);
                        entry["spilled_bytes"] = json!(result_json.len());
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to spill tool result to object store");
                        entry["spill_error"] = json!(e.to_string());
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "invalid tool spill object path");
            }
        }
    }
    entry
}

/// 从 tool 结果 JSON（`Vec<ToolContent>` 序列化）提取 row/scanned/took + 摘要文本。
fn summarize_tool_result(result_json: &str) -> (Option<i64>, Option<i64>, Option<i64>, String) {
    let parsed: Value = serde_json::from_str(result_json).unwrap_or(Value::Null);
    let mut row_count = None;
    let mut scanned = None;
    let mut took = None;
    let mut text_summary = String::new();
    if let Some(arr) = parsed.as_array() {
        for item in arr {
            match item.get("type").and_then(|t| t.as_str()) {
                Some("json") => {
                    let j = item.get("json").cloned().unwrap_or(Value::Null);
                    for key in ["rows", "incidents", "streams", "spans"] {
                        if let Some(a) = j.get(key).and_then(|v| v.as_array()) {
                            row_count = Some(a.len() as i64);
                        }
                    }
                    scanned = j.get("scanned_rows").and_then(|v| v.as_i64()).or(scanned);
                    took = j.get("took_ms").and_then(|v| v.as_i64()).or(took);
                }
                Some("text") => {
                    if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                        text_summary.push_str(t);
                    }
                }
                _ => {}
            }
        }
    }
    let summary = if !text_summary.is_empty() {
        cap_str(&text_summary, EVIDENCE_SUMMARY_CAP)
    } else if let Some(n) = row_count {
        format!("{n} rows")
    } else {
        "ok".to_string()
    };
    (row_count, scanned, took, summary)
}

fn cap_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{truncated}…")
}

// ---------------------------------------------------------------------------
// Transcript archive
// ---------------------------------------------------------------------------

#[permission("intelligence.use")]
async fn archive_chat_route(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    require_license(&state)?;
    let chat = state
        .intelligence
        .chats
        .get_chat(&ctx.org_id, &Id(id))
        .await?;
    let archive = archive_chat(&state, &ctx, &chat).await;
    Ok(Json(json!({
        "status": archive.status,
        "object_key": archive.object_key,
        "sha256": archive.sha256,
        "bytes": archive.bytes,
        "error": archive.error,
    })))
}

/// 写 transcript JSON 到对象存储 + PG 归档元数据 + 审计；失败也记一行（status=failed），
/// 绝不抹掉 PG chat history，审计 payload 不含 transcript 原文。
async fn archive_chat(
    state: &AppState,
    ctx: &IamContext,
    chat: &Chat,
) -> crate::infra::persistence::repositories::intelligence::chat_archives::ChatArchive {
    use crate::infra::persistence::repositories::intelligence::chat_archives::ChatArchive;

    let messages = state
        .intelligence
        .chats
        .list_messages(&chat.id)
        .await
        .unwrap_or_default();
    let total_prompt: i64 = messages.iter().filter_map(|m| m.prompt_tokens).sum();
    let total_completion: i64 = messages.iter().filter_map(|m| m.completion_tokens).sum();
    let total_cost: f64 = messages.iter().filter_map(|m| m.cost_usd).sum();
    let transcript = json!({
        "chat": {
            "id": chat.id.0,
            "org_id": chat.org_id.0,
            "provider": chat.provider,
            "provider_id": chat.provider_id,
            "model": chat.model,
            "title": chat.title,
            "analysis_mode": chat.analysis_mode,
            "time_range_start_micros": chat.time_range_start_micros,
            "time_range_end_micros": chat.time_range_end_micros,
            "created_at_micros": chat.created_at.0,
            "updated_at_micros": chat.updated_at.0,
        },
        "messages": messages.iter().map(|m| json!({
            "id": m.id.0,
            "role": m.role,
            "content": m.content,
            "prompt_template_id": m.prompt_template_id,
            "prompt_builtin_key": m.prompt_builtin_key,
            "prompt_version": m.prompt_version,
            "prompt_hash": m.prompt_hash,
            "evidence": m.evidence_json,
            "prompt_tokens": m.prompt_tokens,
            "completion_tokens": m.completion_tokens,
            "cost_usd": m.cost_usd,
            "created_at_micros": m.created_at.0,
        })).collect::<Vec<_>>(),
        "token_usage": { "prompt_tokens": total_prompt, "completion_tokens": total_completion },
        "cost_usd_total": total_cost,
        "archived_at_micros": TimestampMicros::now().0,
    });

    let body = serde_json::to_string(&transcript).unwrap_or_else(|_| "{}".into());
    let sha = prompt_hash(&body);
    let bytes = body.len() as i64;
    let object_key = format!(
        "intelligence/chat/{}/{}/transcript-{}.json",
        chat.org_id.0,
        chat.id.0,
        TimestampMicros::now().0
    );

    let write_ok = match ObjPath::parse(&object_key) {
        Ok(path) => state
            .storage
            .object_store
            .put(&path, PutPayload::from(body.into_bytes()))
            .await
            .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };

    let archive = match write_ok {
        Ok(_) => {
            let _ = state
                .intelligence
                .chats
                .set_archive_object_key(&chat.id, &object_key)
                .await;
            ChatArchive {
                id: Id::new(),
                chat_id: chat.id.clone(),
                org_id: chat.org_id.clone(),
                object_key: Some(object_key.clone()),
                sha256: Some(sha.clone()),
                bytes,
                status: "ok".into(),
                error: None,
                created_by: Some(ctx.user_id.0.clone()),
                created_at: TimestampMicros::now(),
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, chat = %chat.id.0, "chat archive write failed");
            ChatArchive {
                id: Id::new(),
                chat_id: chat.id.clone(),
                org_id: chat.org_id.clone(),
                object_key: None,
                sha256: None,
                bytes: 0,
                status: "failed".into(),
                error: Some(err),
                created_by: Some(ctx.user_id.0.clone()),
                created_at: TimestampMicros::now(),
            }
        }
    };

    let saved = state
        .intelligence
        .chat_archives
        .record(archive.clone())
        .await
        .unwrap_or(archive);
    // 审计：含 object_key / checksum / status，不含 transcript 原文。
    activity_audit::record(
        state,
        ctx,
        "intelligence.chat.archived",
        "intelligence_chat",
        &chat.id.0,
        json!({
            "status": saved.status,
            "object_key": saved.object_key,
            "sha256": saved.sha256,
            "bytes": saved.bytes,
            "error": saved.error,
        }),
    )
    .await;
    saved
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Provider 不存在 / env 缺失 → 落一条 error assistant + 返一条 SSE event:error。
async fn error_stream_response(
    state: &AppState,
    ctx: &IamContext,
    chat: &Chat,
    msg: String,
) -> Result<Response> {
    let _ = state
        .intelligence
        .chats
        .append_message(ChatMessage::minimal(
            Id::new(),
            chat.id.clone(),
            ctx.org_id.clone(),
            "assistant",
            format!("[error: {msg}]"),
            TimestampMicros::now(),
        ))
        .await;
    let body = format!("event: error\ndata: {}\n\n", json!({ "message": msg }));
    let stream = segmented_result_stream(
        stream::once(async move { Ok::<_, std::convert::Infallible>(bytes::Bytes::from(body)) }),
        "intelligence.http.sse",
        "sse",
    );
    Ok(Response::builder()
        .status(200)
        .header(CONTENT_TYPE, "text/event-stream")
        .header("cache-control", "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| Response::new(Body::empty())))
}

fn system_message(text: String) -> ExternalChatMessage {
    ExternalChatMessage {
        id: Id::new(),
        chat_id: Id(String::new()),
        role: MessageRole::System,
        content: text,
        tool_call_id: None,
        tool_calls: Value::Null,
        created_at: TimestampMicros::now(),
        prompt_tokens: None,
        completion_tokens: None,
    }
}

/// 把 [`builtin_tools`] 转成 provider 期望的 tools schema。
/// OpenAI / OpenAI-compatible：`{type:function, function:{name,description,parameters}}`；
/// Anthropic：`{name, description, input_schema}`。
fn tools_schema_for(provider: Provider, resolution: &toolsets::ToolsetResolution) -> Value {
    // 内置工具按默认 Agent Profile + 组织 Toolset + 工具策略过滤；MCP 工具必须
    // 同时满足 Profile 网络策略、Server 健康状态和显式启用状态。
    let mut arr: Vec<Value> = builtin_tools()
        .into_iter()
        .filter(|t| resolution.builtin_enabled(&t.name))
        .map(|t| tool_schema_entry(provider, &t.name, &t.description, &t.input_schema))
        .collect();
    arr.extend(
        resolution
            .mcp_tools
            .values()
            .filter(|tool| resolution.mcp_tool(&tool.name).is_some())
            .map(|tool| {
                tool_schema_entry(provider, &tool.name, &tool.description, &tool.input_schema)
            }),
    );
    arr.sort_by(|left, right| tool_schema_name(left).cmp(tool_schema_name(right)));
    Value::Array(arr)
}

fn tool_schema_name(value: &Value) -> &str {
    value
        .get("name")
        .or_else(|| value.pointer("/function/name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// 单个工具 schema 条目（按 provider 期望格式）。
fn tool_schema_entry(
    provider: Provider,
    name: &str,
    description: &str,
    input_schema: &Value,
) -> Value {
    match provider {
        Provider::Anthropic => json!({
            "name": name,
            "description": description,
            "input_schema": input_schema,
        }),
        Provider::OpenAi | Provider::OpenAiCompatible => json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": input_schema,
            }
        }),
    }
}

fn history_to_external(rows: &[ChatMessage]) -> Vec<ExternalChatMessage> {
    rows.iter()
        .map(|m| ExternalChatMessage {
            id: m.id.clone(),
            chat_id: m.chat_id.clone(),
            role: match m.role.as_str() {
                "assistant" => MessageRole::Assistant,
                "tool" => MessageRole::Tool,
                "system" => MessageRole::System,
                _ => MessageRole::User,
            },
            content: m.content.clone(),
            tool_call_id: m.tool_result_for.clone(),
            tool_calls: m.tool_calls_json.clone().unwrap_or(Value::Null),
            created_at: m.created_at,
            prompt_tokens: m.prompt_tokens.map(|v| v as i32),
            completion_tokens: m.completion_tokens.map(|v| v as i32),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purpose_mapping() {
        assert_eq!(purpose_for_mode("root_cause"), Some("root_cause"));
        assert_eq!(purpose_for_mode("anomaly"), Some("anomaly_analysis"));
        assert_eq!(purpose_for_mode("freeform"), None);
    }

    #[test]
    fn summarize_extracts_row_count() {
        let result =
            r#"[{"type":"json","json":{"rows":[[1],[2],[3]],"scanned_rows":99,"took_ms":7}}]"#;
        let (rows, scanned, took, summary) = summarize_tool_result(result);
        assert_eq!(rows, Some(3));
        assert_eq!(scanned, Some(99));
        assert_eq!(took, Some(7));
        assert_eq!(summary, "3 rows");
    }

    #[test]
    fn summarize_text_result() {
        let result = r#"[{"type":"text","text":"trace not found"}]"#;
        let (_, _, _, summary) = summarize_tool_result(result);
        assert_eq!(summary, "trace not found");
    }

    #[test]
    fn openai_tools_schema_has_function_wrapper() {
        let schema = tools_schema_for(Provider::OpenAi, &toolsets::ToolsetResolution::default());
        let arr = schema.as_array().unwrap();
        assert!(!arr.is_empty());
        assert_eq!(arr[0]["type"], "function");
        assert!(arr[0]["function"]["name"].is_string());
    }

    #[test]
    fn anthropic_tools_schema_uses_input_schema() {
        let schema = tools_schema_for(Provider::Anthropic, &toolsets::ToolsetResolution::default());
        let arr = schema.as_array().unwrap();
        assert!(arr[0]["input_schema"].is_object());
        assert!(arr[0]["name"].is_string());
    }

    #[test]
    fn final_answer_instruction_hides_internal_execution_details() {
        let instruction = answer_presentation_instruction(false);
        assert!(instruction.contains("Never expose internal tool/function names"));
        assert!(instruction.contains("\"limitations\""));
        assert!(instruction.contains("rendered separately"));
    }

    #[test]
    fn observability_query_instruction_prevents_known_query_mistakes() {
        let instruction = observability_query_instruction();
        assert!(instruction.contains("list_streams"));
        assert!(instruction.contains("never invent timestamp"));
        assert!(instruction.contains("label_values()"));
        assert!(instruction.contains("Do not repeat equivalent empty queries"));
    }

    #[test]
    fn regeneration_reuses_the_user_question_and_drops_previous_answers() {
        let now = TimestampMicros(1);
        let chat_id = Id("chat-1".into());
        let org_id = Id("org-1".into());
        let user = ChatMessage::minimal(
            Id("user-1".into()),
            chat_id.clone(),
            org_id.clone(),
            "user",
            "谁正在负责生产环境值班？",
            now,
        );
        let answer = ChatMessage::minimal(
            Id("answer-1".into()),
            chat_id,
            org_id,
            "assistant",
            "旧回答",
            now,
        );

        let history = regeneration_history(
            vec![user.clone(), answer],
            "user-1",
            " 谁正在负责生产环境值班？ ",
        )
        .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, user.id);
    }

    #[test]
    fn message_response_uses_frontend_timestamp_contract() {
        let response = to_message_resp(ChatMessage::minimal(
            Id("message-1".into()),
            Id("chat-1".into()),
            Id("org-1".into()),
            "assistant",
            "回答",
            TimestampMicros(42),
        ));
        let value = serde_json::to_value(response).unwrap();

        assert_eq!(value["created_at_micros"], 42);
        assert!(value.get("created_at").is_none());
    }
}
