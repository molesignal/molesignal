// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OpenAI Chat Completion (`POST /v1/chat/completions`) 的 Model Gateway adapter。
//!
//! 设计：
//! - 每个 SSE 帧 `data: <json>` 是一个 `chat.completion.chunk`；
//! - 一个 message 由多个 chunk 拼成（按 `delta.content` / `delta.tool_calls[].function.arguments`
//!   增量追加）；
//! - tool_calls 增量按 `tool_calls[].index` 排序，arguments 是分片字符串，需要 buffer
//!   到 `finish_reason: tool_calls` 时再 yield；
//! - usage 块（`stream_options: {include_usage: true}`）只会在末尾出现；
//! - `data: [DONE]` 是流结束哨兵，忽略 JSON 解析。

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures::stream::StreamExt;

use crate::{
    intelligence::chat::{
        ChatStream, ChunkOrToolCall, CompletionRequest, CompletionResponse, MessageRole, Provider,
        ProviderAdapter, ToolChoice, providers::sse::response_to_sse,
    },
    shared::{Error, Result, trace_stream::segmented_result_stream},
};

#[derive(Debug, Clone)]
pub struct OpenAiAdapter {
    pub base_url: String,
    pub api_key: String,
    pub client: reqwest::Client,
}

impl OpenAiAdapter {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }

    pub(crate) fn body(&self, req: &CompletionRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                    MessageRole::System => "system",
                };
                let mut v = serde_json::json!({ "role": role, "content": m.content });
                if let Some(tcid) = &m.tool_call_id {
                    v["tool_call_id"] = serde_json::Value::String(tcid.clone());
                }
                if !m.tool_calls.is_null() {
                    v["tool_calls"] = m.tool_calls.clone();
                }
                v
            })
            .collect();
        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "stream": stream,
        });
        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(mt) = req.max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }
        if let Some(tools) = &req.tools {
            body["tools"] = tools.clone();
        }
        body["tool_choice"] = match &req.tool_choice {
            ToolChoice::Auto => serde_json::json!("auto"),
            ToolChoice::None => serde_json::json!("none"),
            ToolChoice::Required => serde_json::json!("required"),
            ToolChoice::Specific(name) => serde_json::json!({
                "type": "function",
                "function": {"name": name}
            }),
        };
        if stream {
            body["stream_options"] = serde_json::json!({ "include_usage": true });
        }
        body
    }

    #[tracing::instrument(
        name = "gen_ai.request",
        skip_all,
        fields(
            otel.kind = "client",
            gen_ai.provider.name = "openai",
            gen_ai.request.model = %req.model,
            molesignal.gen_ai.streaming = stream
        )
    )]
    pub(crate) async fn send(
        &self,
        req: &CompletionRequest,
        stream: bool,
    ) -> Result<reqwest::Response> {
        req.validate_tool_choice()?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let request = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&self.body(req, stream));
        let resp = crate::shared::http_trace::send(
            &self.client,
            request,
            crate::shared::http_trace::HttpTarget::ThirdParty,
        )
        .await
        .map_err(|e| Error::internal(format!("openai request: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::internal(format!("openai status {status}: {body}")));
        }
        Ok(resp)
    }
}

#[async_trait]
impl ProviderAdapter for OpenAiAdapter {
    fn provider(&self) -> Provider {
        Provider::OpenAi
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let resp = self.send(&req, false).await?;
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::internal(format!("openai decode: {e}")))?;
        let choice = &v["choices"][0];
        let content = choice["message"]["content"].as_str().map(|s| s.to_string());
        let mut tool_calls = Vec::new();
        if let Some(tcs) = choice["message"]["tool_calls"].as_array() {
            for tc in tcs {
                tool_calls.push(crate::intelligence::chat::RequestedToolCall {
                    id: tc["id"].as_str().unwrap_or_default().to_string(),
                    name: tc["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    arguments: serde_json::from_str(
                        tc["function"]["arguments"].as_str().unwrap_or("null"),
                    )
                    .unwrap_or(serde_json::Value::Null),
                });
            }
        }
        Ok(CompletionResponse {
            content,
            tool_calls,
            prompt_tokens: v["usage"]["prompt_tokens"].as_i64().unwrap_or(0) as i32,
            completion_tokens: v["usage"]["completion_tokens"].as_i64().unwrap_or(0) as i32,
            finish_reason: choice["finish_reason"]
                .as_str()
                .unwrap_or("stop")
                .to_string(),
        })
    }

    async fn complete_stream(&self, req: CompletionRequest) -> Result<ChatStream> {
        let resp = self.send(&req, true).await?;
        let mut sse = response_to_sse(resp);

        // 用 channel 暴露 stream，逐 SSE 帧解析并 push 增量；arguments buffer 在闭包内。
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChunkOrToolCall>>(32);
        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            // tool_calls buffer：(index -> (id, name, accumulated args))
            let mut tc_bufs: HashMap<i64, (String, String, String)> = HashMap::new();
            while let Some(item) = sse.next().await {
                let evt = match item {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                };
                if evt.data == "[DONE]" {
                    continue;
                }
                let v: serde_json::Value = match serde_json::from_str(&evt.data) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::debug!(error = %e, "openai non-json sse frame ignored");
                        continue;
                    }
                };
                // usage 终结块（choices == []，但 usage 存在）。
                if let Some(usage) = v.get("usage").and_then(|u| u.as_object())
                    && !usage.is_empty()
                {
                    let pt = usage
                        .get("prompt_tokens")
                        .and_then(|x| x.as_i64())
                        .unwrap_or(0) as i32;
                    let ct = usage
                        .get("completion_tokens")
                        .and_then(|x| x.as_i64())
                        .unwrap_or(0) as i32;
                    let _ = tx
                        .send(Ok(ChunkOrToolCall::Usage {
                            prompt_tokens: pt,
                            completion_tokens: ct,
                        }))
                        .await;
                }
                let Some(choice) = v["choices"].get(0) else {
                    continue;
                };
                let delta = &choice["delta"];
                if let Some(text) = delta["content"].as_str()
                    && !text.is_empty()
                {
                    let _ = tx.send(Ok(ChunkOrToolCall::Text(text.to_string()))).await;
                }
                if let Some(tcs) = delta["tool_calls"].as_array() {
                    for tc in tcs {
                        let idx = tc["index"].as_i64().unwrap_or(0);
                        let entry = tc_bufs
                            .entry(idx)
                            .or_insert_with(|| ("".to_string(), "".to_string(), "".to_string()));
                        if let Some(id) = tc["id"].as_str()
                            && !id.is_empty()
                        {
                            entry.0 = id.to_string();
                        }
                        if let Some(name) = tc["function"]["name"].as_str()
                            && !name.is_empty()
                        {
                            entry.1 = name.to_string();
                        }
                        if let Some(args) = tc["function"]["arguments"].as_str() {
                            entry.2.push_str(args);
                        }
                    }
                }
                if let Some(fr) = choice["finish_reason"].as_str()
                    && (fr == "tool_calls" || fr == "stop" || fr == "length")
                {
                    // 把 buffered tool_calls flush 出去
                    let mut indices: Vec<i64> = tc_bufs.keys().copied().collect();
                    indices.sort_unstable();
                    for idx in indices {
                        if let Some((id, name, args)) = tc_bufs.remove(&idx) {
                            let _ = tx
                                .send(Ok(ChunkOrToolCall::ToolCall {
                                    id,
                                    name,
                                    arguments: args,
                                }))
                                .await;
                        }
                    }
                    let _ = tx.send(Ok(ChunkOrToolCall::Done(fr.to_string()))).await;
                }
            }
        });
        Ok(segmented_result_stream(
            tokio_stream::wrappers::ReceiverStream::new(rx),
            "gen_ai.provider.stream",
            "ai",
        ))
    }
}

/// 工厂：根据 env 构造 adapter，避免 bootstrap 阶段散落字段。
pub fn from_env(env_prefix: &str) -> Result<Arc<OpenAiAdapter>> {
    let api_key_var = format!("{env_prefix}_API_KEY");
    let base_url_var = format!("{env_prefix}_BASE_URL");
    let api_key = std::env::var(&api_key_var)
        .map_err(|_| Error::invalid(format!("missing env {api_key_var}")))?;
    let base_url =
        std::env::var(&base_url_var).unwrap_or_else(|_| "https://api.openai.com/v1".into());
    Ok(Arc::new(OpenAiAdapter::new(base_url, api_key)))
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;

    fn mk_req(model: &str) -> CompletionRequest {
        use crate::{
            intelligence::chat::{ChatMessage, MessageRole},
            shared::{ids::Id, time::TimestampMicros},
        };
        CompletionRequest {
            model: model.into(),
            messages: vec![ChatMessage {
                id: Id::new(),
                chat_id: Id("s".into()),
                role: MessageRole::User,
                content: "hi".into(),
                tool_call_id: None,
                tool_calls: serde_json::Value::Null,
                created_at: TimestampMicros(0),
                prompt_tokens: None,
                completion_tokens: None,
            }],
            tools: None,
            tool_choice: ToolChoice::Auto,
            max_tokens: None,
            temperature: None,
        }
    }

    fn tool_req(choice: ToolChoice) -> CompletionRequest {
        let mut req = mk_req("gpt-4o");
        req.tools = Some(serde_json::json!([{
            "type": "function",
            "function": {
                "name": "prepare_dashboard",
                "description": "Prepare a Dashboard",
                "parameters": {"type": "object"}
            }
        }]));
        req.tool_choice = choice;
        req
    }

    #[test]
    fn maps_all_tool_choice_modes_into_openai_request_body() {
        let adapter = OpenAiAdapter::new("http://localhost", "test");
        let cases = [
            (ToolChoice::Auto, serde_json::json!("auto")),
            (ToolChoice::None, serde_json::json!("none")),
            (ToolChoice::Required, serde_json::json!("required")),
            (
                ToolChoice::Specific("prepare_dashboard".into()),
                serde_json::json!({
                    "type": "function",
                    "function": {"name": "prepare_dashboard"}
                }),
            ),
        ];
        for (choice, expected) in cases {
            let req = tool_req(choice);
            req.validate_tool_choice().unwrap();
            assert_eq!(adapter.body(&req, false)["tool_choice"], expected);
        }
    }

    #[tokio::test]
    async fn parses_sse_chunks() {
        let srv = MockServer::start().await;
        let body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1}}\n\n",
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

        let ad = OpenAiAdapter::new(srv.uri(), "test");
        let mut stream = ad.complete_stream(mk_req("gpt-4o")).await.unwrap();
        let mut text = String::new();
        let mut usage = (0i32, 0i32);
        let mut done = None;
        while let Some(item) = stream.next().await {
            match item.unwrap() {
                ChunkOrToolCall::Text(t) => text.push_str(&t),
                ChunkOrToolCall::Usage {
                    prompt_tokens,
                    completion_tokens,
                } => usage = (prompt_tokens, completion_tokens),
                ChunkOrToolCall::Done(r) => done = Some(r),
                ChunkOrToolCall::ToolCall { .. } => panic!("unexpected tool call"),
            }
        }
        assert_eq!(text, "hi");
        assert_eq!(usage, (3, 1));
        assert_eq!(done.as_deref(), Some("stop"));
    }

    /// task 5.3：429 → adapter 应返 `Err`，让 AgentLoop 把它转成 AgentStreamEvent::Error。
    #[tokio::test]
    async fn rate_limited_returns_err() {
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("content-type", "application/json")
                    .set_body_string(r#"{"error":{"message":"rate limited"}}"#),
            )
            .mount(&srv)
            .await;
        let ad = OpenAiAdapter::new(srv.uri(), "test");
        let result = ad.complete_stream(mk_req("gpt-4o")).await;
        let err = match result {
            Ok(_) => panic!("expected Err for HTTP 429"),
            Err(e) => e,
        };
        let msg = err.to_string();
        assert!(msg.contains("429"), "{msg}");
    }

    #[tokio::test]
    async fn buffers_tool_call_arguments_across_chunks() {
        let srv = MockServer::start().await;
        let body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"get_w\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"loc\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\":\\\"SF\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
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

        let ad = OpenAiAdapter::new(srv.uri(), "test");
        let mut stream = ad.complete_stream(mk_req("gpt-4o")).await.unwrap();
        let mut got_tool: Option<(String, String, String)> = None;
        let mut done = None;
        while let Some(item) = stream.next().await {
            match item.unwrap() {
                ChunkOrToolCall::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    got_tool = Some((id, name, arguments));
                }
                ChunkOrToolCall::Done(r) => done = Some(r),
                _ => {}
            }
        }
        let (id, name, args) = got_tool.expect("expected one tool call");
        assert_eq!(id, "call_a");
        assert_eq!(name, "get_w");
        assert_eq!(args, "{\"loc\":\"SF\"}");
        assert_eq!(done.as_deref(), Some("tool_calls"));
    }
}
