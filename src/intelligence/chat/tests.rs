// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Mutex;

use super::*;
use crate::intelligence::tools::ToolContent;

#[test]
fn provider_string_roundtrip() {
    for p in [
        Provider::OpenAi,
        Provider::Anthropic,
        Provider::OpenAiCompatible,
    ] {
        assert_eq!(Provider::parse(p.as_str()).unwrap(), p);
    }
    assert!(Provider::parse("garbage").is_err());
}

// Mock provider：第一次回 tool_call，第二次回最终文本
struct ScriptedProvider {
    calls: Mutex<usize>,
}
#[async_trait]
impl ProviderAdapter for ScriptedProvider {
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        let mut n = self.calls.lock().unwrap();
        *n += 1;
        if *n == 1 {
            Ok(CompletionResponse {
                content: Some("I will inspect the internal data first.".into()),
                tool_calls: vec![RequestedToolCall {
                    id: "tc-1".into(),
                    name: "query_logs".into(),
                    arguments: serde_json::json!({"sql": "SELECT 1"}),
                }],
                prompt_tokens: 50,
                completion_tokens: 30,
                finish_reason: "tool_calls".into(),
            })
        } else {
            Ok(CompletionResponse {
                content: Some("There were 100 errors.".into()),
                tool_calls: vec![],
                prompt_tokens: 80,
                completion_tokens: 20,
                finish_reason: "stop".into(),
            })
        }
    }
}

struct EchoDispatcher;
#[async_trait]
impl ToolDispatcher for EchoDispatcher {
    async fn dispatch(&self, _ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult> {
        Ok(ToolResult {
            content: vec![ToolContent::Json {
                json: serde_json::json!({"echoed": call.name}),
            }],
            is_error: false,
        })
    }
}

fn ctx() -> ToolAuthContext {
    ToolAuthContext {
        user_id: "u1".into(),
        org_id: "orgA".into(),
        chat_id: None,
        investigation_id: None,
        execution_policy: Default::default(),
        query_generation_only: false,
    }
}

fn user_msg(s: &str) -> ChatMessage {
    ChatMessage {
        id: Id::new(),
        chat_id: Id("s1".into()),
        role: MessageRole::User,
        content: s.into(),
        tool_call_id: None,
        tool_calls: serde_json::Value::Null,
        created_at: TimestampMicros::now(),
        prompt_tokens: None,
        completion_tokens: None,
    }
}

#[tokio::test]
async fn loop_executes_one_tool_call_then_stops() {
    let p = Arc::new(ScriptedProvider {
        calls: Mutex::new(0),
    });
    let d = Arc::new(EchoDispatcher);
    let loop_ = AgentLoop::new(p, d);
    let r = loop_
        .run(
            &ctx(),
            "gpt-4o".into(),
            vec![user_msg("how many errors?")],
            None,
            Some(1024),
        )
        .await
        .unwrap();
    assert_eq!(r.tool_calls_made, 1);
    assert!(r.assistant_content.contains("100 errors"));
    assert_eq!(r.prompt_tokens_total, 130); // 50 + 80
    assert_eq!(r.completion_tokens_total, 50); // 30 + 20
}

// Provider 永远返 tool_call → 触发 MAX_TOOL_LOOPS 后走无工具收尾。
struct LoopForeverProvider;
#[async_trait]
impl ProviderAdapter for LoopForeverProvider {
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            content: None,
            tool_calls: vec![RequestedToolCall {
                id: "tc".into(),
                name: "query_logs".into(),
                arguments: serde_json::Value::Null,
            }],
            prompt_tokens: 1,
            completion_tokens: 1,
            finish_reason: "tool_calls".into(),
        })
    }
}

#[tokio::test]
async fn loop_finalizes_after_max_loops() {
    let p = Arc::new(LoopForeverProvider);
    let d = Arc::new(EchoDispatcher);
    let loop_ = AgentLoop::new(p, d);
    let r = loop_
        .run(&ctx(), "m".into(), vec![user_msg("hi")], None, None)
        .await
        .unwrap();
    assert_eq!(r.tool_calls_made, MAX_TOOL_LOOPS);
    assert!(r.assistant_content.contains("工具调用次数"));
}

/// task 2.2：tool call 回灌的 stream 版本。
/// 第一次调 provider 返 tool_call，第二次返文本 → stream 应依次给出
/// ToolStart / ToolEnd / Chunk / Done。
#[tokio::test]
async fn stream_run_executes_tool_then_finalizes() {
    use futures::StreamExt;

    let p: Arc<dyn ProviderAdapter> = Arc::new(ScriptedProvider {
        calls: Mutex::new(0),
    });
    let d: Arc<dyn ToolDispatcher> = Arc::new(EchoDispatcher);
    let loop_ = Arc::new(AgentLoop::new(p, d));
    let mut stream = loop_.clone().run_stream(
        ctx(),
        "gpt-4o".into(),
        vec![user_msg("how many errors?")],
        None,
        Some(1024),
    );

    let mut events: Vec<AgentStreamEvent> = Vec::new();
    while let Some(e) = stream.next().await {
        events.push(e);
    }
    // 必须含至少一个 ToolStart + ToolEnd + Done（含最终文本）。
    let mut saw_tool_start = false;
    let mut saw_tool_end = false;
    let mut final_content = String::new();
    let mut visible_chunks = String::new();
    for e in &events {
        match e {
            AgentStreamEvent::ToolStart { name, .. } => {
                assert_eq!(name, "query_logs");
                saw_tool_start = true;
            }
            AgentStreamEvent::ToolEnd { is_error, .. } => {
                assert!(!is_error);
                saw_tool_end = true;
            }
            AgentStreamEvent::Done { content, .. } => {
                final_content = content.clone();
            }
            AgentStreamEvent::Error(e) => panic!("unexpected error: {e}"),
            AgentStreamEvent::Chunk(chunk) => visible_chunks.push_str(chunk),
        }
    }
    assert!(saw_tool_start, "ToolStart missing in {events:?}");
    assert!(saw_tool_end, "ToolEnd missing in {events:?}");
    assert!(
        final_content.contains("100 errors"),
        "Done.content was {final_content:?}"
    );
    assert!(
        !visible_chunks.contains("internal data"),
        "intermediate tool narration leaked into visible chunks: {visible_chunks:?}"
    );
}

/// MAX_TOOL_LOOPS 在 stream 模式下应走 Done，不把内部预算错误暴露给用户。
#[tokio::test]
async fn stream_run_finalizes_after_max_loops() {
    use futures::StreamExt;

    let p: Arc<dyn ProviderAdapter> = Arc::new(LoopForeverProvider);
    let d: Arc<dyn ToolDispatcher> = Arc::new(EchoDispatcher);
    let loop_ = Arc::new(AgentLoop::new(p, d));
    let mut stream = loop_
        .clone()
        .run_stream(ctx(), "m".into(), vec![user_msg("hi")], None, None);
    let mut saw_error = false;
    let mut tool_starts = 0usize;
    let mut final_content = String::new();
    while let Some(e) = stream.next().await {
        match e {
            AgentStreamEvent::ToolStart { .. } => tool_starts += 1,
            AgentStreamEvent::Done { content, .. } => final_content = content,
            AgentStreamEvent::Error(_) => saw_error = true,
            AgentStreamEvent::Chunk(_) | AgentStreamEvent::ToolEnd { .. } => {}
        }
    }
    assert!(!saw_error, "budget exhaustion should not emit Error");
    assert_eq!(tool_starts, MAX_TOOL_LOOPS);
    assert!(final_content.contains("工具调用次数"), "{final_content}");
}

struct DsmlAfterBudgetProvider;

#[async_trait]
impl ProviderAdapter for DsmlAfterBudgetProvider {
    fn provider(&self) -> Provider {
        Provider::OpenAiCompatible
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        if req.tools.is_none() {
            return Ok(CompletionResponse {
                content: Some(
                    "<｜｜DSML｜｜tool_calls><｜｜DSML｜｜invoke name=\"query_logs\">".into(),
                ),
                tool_calls: vec![],
                prompt_tokens: 1,
                completion_tokens: 1,
                finish_reason: "stop".into(),
            });
        }
        Ok(CompletionResponse {
            content: Some("Let me run another query.".into()),
            tool_calls: vec![RequestedToolCall {
                id: "tc".into(),
                name: "query_logs".into(),
                arguments: serde_json::Value::Null,
            }],
            prompt_tokens: 1,
            completion_tokens: 1,
            finish_reason: "tool_calls".into(),
        })
    }
}

#[test]
fn dsml_tool_transport_is_not_a_product_answer() {
    assert!(contains_internal_tool_transport(
        "<|DSML|tool_calls><|DSML|invoke name=\"query_logs\">"
    ));
    assert!(contains_internal_tool_transport(
        "<｜｜DSML｜｜tool_calls><｜｜DSML｜｜parameter>"
    ));
    assert!(!contains_internal_tool_transport(
        "{\"summary\":\"checkout-api error rate increased\"}"
    ));
}

#[tokio::test]
async fn loop_replaces_dsml_budget_final_with_safe_fallback() {
    let loop_ = AgentLoop::new(Arc::new(DsmlAfterBudgetProvider), Arc::new(EchoDispatcher));
    let result = loop_
        .run(
            &ctx(),
            "m".into(),
            vec![user_msg("hi")],
            Some(serde_json::json!([])),
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.tool_calls_made, MAX_TOOL_LOOPS);
    assert!(!result.assistant_content.contains("DSML"));
    assert!(result.assistant_content.contains("工具调用次数"));
}

#[tokio::test]
async fn stream_replaces_dsml_budget_final_without_leaking_internal_narration() {
    use futures::StreamExt;

    let loop_ = Arc::new(AgentLoop::new(
        Arc::new(DsmlAfterBudgetProvider),
        Arc::new(EchoDispatcher),
    ));
    let mut stream = loop_.run_stream(
        ctx(),
        "m".into(),
        vec![user_msg("hi")],
        Some(serde_json::json!([])),
        None,
    );
    let mut visible_chunks = String::new();
    let mut final_content = String::new();
    while let Some(event) = stream.next().await {
        match event {
            AgentStreamEvent::Chunk(chunk) => visible_chunks.push_str(&chunk),
            AgentStreamEvent::Done { content, .. } => final_content = content,
            AgentStreamEvent::Error(error) => panic!("unexpected error: {error}"),
            AgentStreamEvent::ToolStart { .. } | AgentStreamEvent::ToolEnd { .. } => {}
        }
    }
    assert!(!visible_chunks.contains("DSML"));
    assert!(!visible_chunks.contains("another query"));
    assert!(visible_chunks.contains("工具调用次数"));
    assert_eq!(visible_chunks, final_content);
}

struct DashboardAuthoringProvider {
    choices: Mutex<Vec<ToolChoice>>,
}

#[async_trait]
impl ProviderAdapter for DashboardAuthoringProvider {
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let call_number = {
            let mut choices = self.choices.lock().unwrap();
            choices.push(req.tool_choice.clone());
            choices.len()
        };
        let tool_call = |id: &str, name: &str, arguments| CompletionResponse {
            content: None,
            tool_calls: vec![RequestedToolCall {
                id: id.into(),
                name: name.into(),
                arguments,
            }],
            prompt_tokens: 10,
            completion_tokens: 5,
            finish_reason: "tool_calls".into(),
        };
        match call_number {
            1 => Ok(tool_call(
                "prepare-invalid",
                "prepare_dashboard",
                serde_json::json!({"authoringVersion": 1}),
            )),
            2 => {
                assert!(req.messages.iter().any(|message| {
                    message.role == MessageRole::Tool
                        && message.content.contains("CONTRACT_REQUIRED")
                        && message.content.contains("/title")
                }));
                Ok(tool_call(
                    "prepare-repaired",
                    "prepare_dashboard",
                    serde_json::json!({
                        "authoringVersion": 1,
                        "title": "Checkout health",
                        "time": {"from": "now-1h", "to": "now"},
                        "elements": []
                    }),
                ))
            }
            3 => {
                assert!(req.messages.iter().any(|message| {
                    message.role == MessageRole::Tool
                        && message.content.contains("/ai/dashboard-drafts/draft-1")
                        && message.content.contains("0123456789abcdef")
                }));
                Ok(tool_call(
                    "propose",
                    "propose_dashboard_creation",
                    serde_json::json!({
                        "draft_id": "draft-1",
                        "expected_hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                        "reason": "Create the reviewed Dashboard",
                        "impact": "Adds one Dashboard"
                    }),
                ))
            }
            _ => Ok(CompletionResponse {
                content: Some(
                    r#"{"summary":"Dashboard preview is ready for confirmation","evidence":[],"likely_causes":[],"limitations":[],"suggested_next_steps":[],"related_links":[{"label":"Review preview","route":"/ai/dashboard-drafts/draft-1"}],"confidence":"high"}"#.into(),
                ),
                tool_calls: Vec::new(),
                prompt_tokens: 10,
                completion_tokens: 5,
                finish_reason: "stop".into(),
            }),
        }
    }
}

#[derive(Default)]
struct DashboardAuthoringDispatcher {
    calls: Mutex<Vec<String>>,
    prepare_attempts: Mutex<usize>,
    repair_first: bool,
}

#[async_trait]
impl ToolDispatcher for DashboardAuthoringDispatcher {
    async fn dispatch(&self, _ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult> {
        self.calls.lock().unwrap().push(call.name.clone());
        match call.name.as_str() {
            "prepare_dashboard" => {
                let attempt = {
                    let mut attempts = self.prepare_attempts.lock().unwrap();
                    *attempts += 1;
                    *attempts
                };
                if attempt == 1 && self.repair_first {
                    return Err(Error::validation(
                        "dashboard authoring specification is invalid",
                        vec![crate::shared::contracts::ContractIssue::new(
                            "CONTRACT_REQUIRED",
                            "/title",
                            "title is required",
                            true,
                        )],
                    ));
                }
                Ok(ToolResult {
                    content: vec![ToolContent::Json {
                        json: serde_json::json!({
                            "draft_id": "draft-1",
                            "model_hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                            "preview_route": "/ai/dashboard-drafts/draft-1"
                        }),
                    }],
                    is_error: false,
                })
            }
            "propose_dashboard_creation" => Ok(ToolResult {
                content: vec![ToolContent::Json {
                    json: serde_json::json!({
                        "approval": {"id": "approval-1", "status": "approved"}
                    }),
                }],
                is_error: false,
            }),
            other => Err(Error::invalid(format!("unexpected tool `{other}`"))),
        }
    }
}

#[tokio::test]
async fn dashboard_chat_forces_prepare_repairs_once_and_then_proposes() {
    let provider = Arc::new(DashboardAuthoringProvider {
        choices: Mutex::new(Vec::new()),
    });
    let dispatcher = Arc::new(DashboardAuthoringDispatcher {
        repair_first: true,
        ..DashboardAuthoringDispatcher::default()
    });
    let loop_ = AgentLoop::new(provider.clone(), dispatcher.clone());
    let tools = serde_json::json!([
        {"name": "prepare_dashboard", "input_schema": {"type": "object"}},
        {"name": "propose_dashboard_creation", "input_schema": {"type": "object"}}
    ]);
    let result = loop_
        .run_with_tool_choice(
            &ctx(),
            "gpt-4o".into(),
            vec![user_msg("创建最近一小时 checkout 服务错误率仪表盘")],
            Some(tools),
            Some(2048),
            ToolChoice::Specific("prepare_dashboard".into()),
        )
        .await
        .unwrap();

    assert_eq!(result.tool_calls_made, 3);
    assert!(result.assistant_content.contains("Review preview"));
    assert_eq!(
        *dispatcher.calls.lock().unwrap(),
        vec![
            "prepare_dashboard",
            "prepare_dashboard",
            "propose_dashboard_creation"
        ]
    );
    assert_eq!(
        *provider.choices.lock().unwrap(),
        vec![
            ToolChoice::Specific("prepare_dashboard".into()),
            ToolChoice::Auto,
            ToolChoice::Auto,
            ToolChoice::Auto,
        ]
    );
}

struct PreviewOnlyProvider {
    choices: Mutex<Vec<ToolChoice>>,
}

#[async_trait]
impl ProviderAdapter for PreviewOnlyProvider {
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let call_number = {
            let mut choices = self.choices.lock().unwrap();
            choices.push(req.tool_choice.clone());
            choices.len()
        };
        if call_number == 1 {
            return Ok(CompletionResponse {
                content: None,
                tool_calls: vec![RequestedToolCall {
                    id: "prepare".into(),
                    name: "prepare_dashboard".into(),
                    arguments: serde_json::json!({"authoringVersion": 1}),
                }],
                prompt_tokens: 5,
                completion_tokens: 3,
                finish_reason: "tool_calls".into(),
            });
        }
        assert!(req.tools.as_ref().is_some_and(|tools| {
            tools.as_array().is_some_and(|tools| {
                tools
                    .iter()
                    .all(|tool| tool["name"].as_str() != Some("propose_dashboard_creation"))
            })
        }));
        Ok(CompletionResponse {
            content: Some("Preview ready; creation is unavailable in this chat.".into()),
            tool_calls: Vec::new(),
            prompt_tokens: 5,
            completion_tokens: 3,
            finish_reason: "stop".into(),
        })
    }
}

#[tokio::test]
async fn dashboard_chat_degrades_to_preview_when_proposal_tool_is_absent() {
    let provider = Arc::new(PreviewOnlyProvider {
        choices: Mutex::new(Vec::new()),
    });
    let dispatcher = Arc::new(DashboardAuthoringDispatcher::default());
    let loop_ = AgentLoop::new(provider.clone(), dispatcher.clone());
    let tools = serde_json::json!([
        {"name": "get_dashboard_capabilities", "input_schema": {"type": "object"}},
        {"name": "prepare_dashboard", "input_schema": {"type": "object"}}
    ]);
    let result = loop_
        .run_with_tool_choice(
            &ctx(),
            "gpt-4o".into(),
            vec![user_msg("创建最近一小时服务延迟仪表盘")],
            Some(tools),
            None,
            ToolChoice::Specific("prepare_dashboard".into()),
        )
        .await
        .unwrap();
    assert_eq!(result.tool_calls_made, 1);
    assert!(result.assistant_content.contains("creation is unavailable"));
    assert_eq!(
        dispatcher.calls.lock().unwrap().as_slice(),
        ["prepare_dashboard"]
    );
    assert_eq!(
        provider.choices.lock().unwrap().as_slice(),
        [
            ToolChoice::Specific("prepare_dashboard".into()),
            ToolChoice::Auto
        ]
    );
}
