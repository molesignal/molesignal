// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence analysis 端到端集成。
//!
//! 两层覆盖：
//! - **AgentLoop ↔ wiremock**（无 docker，常规 `cargo test` 即跑）：provider 错误路径
//!   → `AgentStreamEvent::Error`，证明 5xx 被翻译成 error 事件而非 panic。
//! - **HTTP handler ↔ TestServer**（需 docker Postgres + 本地对象存储，`MS_RUN_IT=1` 才跑）：
//!   PG provider 解析 → SSE happy path → prompt 元数据持久化（builtin_key/version/hash）→
//!   transcript 归档（object_key + sha256 + status）。
//!
//! 其余场景在别处单测：tool 证据抽取见 `intelligence::chat::tests::summarize_*`；跨 org tool
//! 隔离见 `intelligence_mcp_dispatcher::tests::list_streams_uses_ctx_org_and_ignores_arg_org_id`；
//! 加密 key 往返 / prompt 解析顺序 / 软删见 `infra` crate `it_ai_anomaly_chat`。

mod common;

use std::sync::Arc;

use molesignal::{
    intelligence::{
        chat::{AgentLoop, AgentStreamEvent, MessageRole, providers::OpenAiAdapter},
        tools::{ToolAuthContext, ToolCall, ToolDispatcher, ToolResult},
    },
    shared::{LicenseGate, Result as SharedResult, ids::Id, time::TimestampMicros},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

struct NoopDispatcher;
#[async_trait::async_trait]
impl ToolDispatcher for NoopDispatcher {
    async fn dispatch(&self, _ctx: &ToolAuthContext, _call: ToolCall) -> SharedResult<ToolResult> {
        Ok(ToolResult {
            content: vec![],
            is_error: false,
        })
    }
}

fn user_msg(s: &str) -> molesignal::intelligence::chat::ChatMessage {
    molesignal::intelligence::chat::ChatMessage {
        id: Id::new(),
        chat_id: Id("s".into()),
        role: MessageRole::User,
        content: s.into(),
        tool_call_id: None,
        tool_calls: serde_json::Value::Null,
        created_at: TimestampMicros::now(),
        prompt_tokens: None,
        completion_tokens: None,
    }
}

/// provider 返 5xx → AgentLoop 发一条 `Error` 事件并收尾（不 panic）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chatloop_provider_error_path_emits_error_event() {
    let srv = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&srv)
        .await;

    let adapter = Arc::new(OpenAiAdapter::new(srv.uri(), "test-key"))
        as Arc<dyn molesignal::intelligence::chat::ProviderAdapter>;
    let chat_loop = Arc::new(AgentLoop::new(adapter, Arc::new(NoopDispatcher)));
    let mut stream = chat_loop.clone().run_stream(
        ToolAuthContext {
            user_id: "u1".into(),
            org_id: "orgA".into(),
            chat_id: None,
            investigation_id: None,
            execution_policy: Default::default(),
            query_generation_only: false,
        },
        "gpt-4o".into(),
        vec![user_msg("hi")],
        None,
        None,
    );

    use futures::StreamExt;
    let mut error_seen = false;
    while let Some(evt) = stream.next().await {
        if let AgentStreamEvent::Error(_) = evt {
            error_seen = true;
        }
    }
    assert!(error_seen, "expected an Error event from a 5xx provider");
}

// ---------------------------------------------------------------------------
// HTTP handler 端到端（MS_RUN_IT gated）
// ---------------------------------------------------------------------------

/// 测试用 license：仅放开 `intelligence` feature。
struct IntelligenceLicense;
impl LicenseGate for IntelligenceLicense {
    fn has_feature(&self, name: &str) -> bool {
        name == "intelligence"
    }
    fn add_ingest_bytes(&self, _n: u64) -> bool {
        true
    }
    fn expired(&self, _now_micros: i64) -> bool {
        false
    }
    fn issued_to(&self) -> &str {
        "test"
    }
    fn reset_daily(&self) {}
    fn features(&self) -> Vec<String> {
        vec!["intelligence".into()]
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_chat_prompt_metadata_and_archive() {
    if common::skip_unless_enabled() {
        eprintln!("skipping http_chat_prompt_metadata_and_archive (set MS_RUN_IT=1)");
        return;
    }
    let server = common::TestServer::start().await;
    // 放开 intelligence license。
    server
        .state
        .platform
        .license_holder
        .replace(Arc::new(IntelligenceLicense));

    // 假 OpenAI：返一条简单 SSE completion（无 tool call）。
    let openai = MockServer::start().await;
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"root cause: bad deploy\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":5}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&openai)
        .await;

    // 1) 建 PG provider 行（base_url 指向 wiremock）。
    let input = molesignal::infra::persistence::repositories::intelligence::model_providers::ModelProviderInput {
        id: Id::new(),
        org_id: server.root_org_id.clone(),
        provider: "openai".into(),
        name: "wiremock".into(),
        base_url: Some(openai.uri()),
        default_model: "gpt-4o".into(),
        enabled: true,
        timeout_ms: 30_000,
        max_tokens: Some(1024),
    };
    let provider = server
        .state
        .intelligence
        .model_providers
        .create(input, Some("sk-test-1234"))
        .await
        .expect("create provider");

    // 2) 建会话（root_cause 模式，绑定 provider）。
    let create: serde_json::Value = server
        .client
        .post(format!("{}/api/v1/intelligence/chat", server.base_url))
        .header("authorization", format!("Bearer {}", server.root_token))
        .json(&serde_json::json!({
            "provider": "openai",
            "model": "gpt-4o",
            "title": "rca",
            "provider_id": provider.id.0,
            "analysis_mode": "root_cause",
        }))
        .send()
        .await
        .expect("create chat")
        .json()
        .await
        .expect("chat json");
    let chat_id = create["id"].as_str().expect("chat id").to_string();

    // 3) POST 一条消息，读 SSE。
    let sse = server
        .client
        .post(format!(
            "{}/api/v1/intelligence/chat/{}/messages",
            server.base_url, chat_id
        ))
        .header("authorization", format!("Bearer {}", server.root_token))
        .header("accept", "text/event-stream")
        .json(&serde_json::json!({
            "content": "why did errors spike?",
            "analysis_mode": "root_cause",
            "time_range": { "start_micros": 1_000_000, "end_micros": 2_000_000 },
            "stream_hints": ["app-logs"],
            "provider_id": provider.id.0,
        }))
        .send()
        .await
        .expect("post message")
        .text()
        .await
        .expect("sse body");
    assert!(sse.contains("event: chunk"), "SSE happy path: {sse}");
    assert!(sse.contains("event: done"), "SSE done: {sse}");

    // 4) 断言 assistant 消息持久化了 prompt 元数据（root_cause builtin）。
    let messages = server
        .state
        .intelligence
        .chats
        .list_messages(&Id(chat_id.clone()))
        .await
        .expect("list messages");
    let assistant = messages
        .iter()
        .find(|m| m.role == "assistant")
        .expect("assistant message present");
    assert_eq!(
        assistant.prompt_builtin_key.as_deref(),
        Some("analysis.root_cause"),
        "root_cause prompt builtin_key persisted"
    );
    assert!(
        assistant.prompt_hash.is_some(),
        "rendered prompt hash persisted"
    );
    assert!(
        assistant.prompt_version.is_some(),
        "prompt version persisted"
    );

    // 5) 归档 transcript → intelligence_chat_archives 落一行（status ok + object_key）。
    let archive: serde_json::Value = server
        .client
        .post(format!(
            "{}/api/v1/intelligence/chat/{}/archive",
            server.base_url, chat_id
        ))
        .header("authorization", format!("Bearer {}", server.root_token))
        .send()
        .await
        .expect("archive")
        .json()
        .await
        .expect("archive json");
    assert_eq!(archive["status"], "ok", "archive ok: {archive}");
    assert!(
        archive["object_key"].as_str().is_some(),
        "archive object_key set"
    );
    let archives = server
        .state
        .intelligence
        .chat_archives
        .list_for_chat(&Id(chat_id.clone()))
        .await
        .expect("list archives");
    assert_eq!(archives.len(), 1);
    assert_eq!(archives[0].status, "ok");
    assert!(archives[0].sha256.is_some());
}
