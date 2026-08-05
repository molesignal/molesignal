// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence chat streaming integration test。
//!
//! 端到端：起一个 wiremock 假 OpenAI → 配 env var 让 `adapter_from_env(OpenAi)` 命中它
//! → 调 `AgentLoop::run_stream` → 验 AgentStreamEvent 序列 + DB 落 row。
//!
//! 默认 `#[ignore]`：本测要求 docker（Postgres testcontainer） + 网络可达。
//! CI 在 `MS_RUN_IT=1` 时跑：
//!
//! ```bash
//! MS_RUN_IT=1 cargo test --test it_intelligence_chat --features  -- --ignored
//! ```
//!
//! 当前实装：仅做 AgentLoop ↔ wiremock 的 streaming smoke（不接 Pg），覆盖任务
//! 描述中"真跑 SSE → 验 bytes 前缀"语义。完整 DB 验证留 follow-up。

use std::sync::Arc;

use molesignal::{
    intelligence::{
        chat::{AgentLoop, AgentStreamEvent, MessageRole, providers::OpenAiAdapter},
        tools::{ToolAuthContext, ToolCall, ToolDispatcher, ToolResult},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

struct NoopDispatcher;
#[async_trait::async_trait]
impl ToolDispatcher for NoopDispatcher {
    async fn dispatch(&self, _ctx: &ToolAuthContext, _call: ToolCall) -> Result<ToolResult> {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "wiremock + docker required; set MS_RUN_IT=1 to enable"]
async fn end_to_end_openai_stream_yields_text_chunks_and_done() {
    if std::env::var("MS_RUN_IT").unwrap_or_default() != "1" {
        return;
    }
    let srv = MockServer::start().await;
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" there\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&srv)
        .await;

    let adapter = Arc::new(OpenAiAdapter::new(srv.uri(), "test-key"))
        as Arc<dyn molesignal::intelligence::chat::ProviderAdapter>;
    let dispatcher: Arc<dyn ToolDispatcher> = Arc::new(NoopDispatcher);
    let chat_loop = Arc::new(AgentLoop::new(adapter, dispatcher));
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
    let mut text = String::new();
    let mut done_seen = false;
    while let Some(evt) = stream.next().await {
        match evt {
            AgentStreamEvent::Chunk(t) => text.push_str(&t),
            AgentStreamEvent::Done {
                prompt_tokens,
                completion_tokens,
                ..
            } => {
                assert_eq!(prompt_tokens, 3);
                assert_eq!(completion_tokens, 2);
                done_seen = true;
            }
            AgentStreamEvent::Error(e) => panic!("unexpected error: {e}"),
            _ => {}
        }
    }
    assert_eq!(text, "hi there");
    assert!(done_seen);
}
