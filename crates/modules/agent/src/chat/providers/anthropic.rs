// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Anthropic Messages API 的 Mole Agent Model Gateway adapter。
//!
//! 重要差异：
//! - URL `{base_url}/v1/messages`；`anthropic-version` header 必须；
//! - SSE 帧有 `event:` 名（`message_start` / `content_block_start` / `content_block_delta` /
//!   `message_delta` / `message_stop`）；
//! - 文本增量：`event: content_block_delta` + `delta.type == "text_delta"`，从 `delta.text` 取；
//! - tool use：`content_block_start` 携 `content_block.type == "tool_use"` + id / name；
//!   随后 `content_block_delta` 的 `delta.type == "input_json_delta"` 增量给 `partial_json`；
//! - usage：`message_start` 给 `input_tokens`，`message_delta` 给 `output_tokens`。

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures::stream::StreamExt;

use crate::{
    agent::chat::{
        ChatMessage, ChatStream, ChunkOrToolCall, CompletionRequest, CompletionResponse,
        MessageRole, Provider, ProviderAdapter, ToolChoice,
        providers::{
            sse::response_to_sse,
            transport::{TransportOptions, send_with_retry},
        },
    },
    shared::{Error, Result, trace_stream::segmented_result_stream},
};

pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Messages API 必须给 `max_tokens`；模型服务没有配置时用这个值，够写完一份结构化结论。
pub const DEFAULT_MAX_TOKENS: i32 = 4096;

#[derive(Debug, Clone)]
pub struct AnthropicAdapter {
    pub base_url: String,
    pub api_key: String,
    pub client: reqwest::Client,
    pub transport: TransportOptions,
}

impl AnthropicAdapter {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        let transport = TransportOptions::default();

        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            client: transport.client(),
            transport,
        }
    }

    /// 换用模型服务自己的超时与重试设置。
    pub fn with_transport(mut self, transport: TransportOptions) -> Self {
        self.client = transport.client();
        self.transport = transport;
        self
    }

    fn body(&self, req: &CompletionRequest, stream: bool) -> serde_json::Value {
        let (system, messages) = messages_body(&req.messages);

        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            "stream": stream,
        });

        if let Some(s) = system {
            body["system"] = serde_json::Value::String(s);
        }

        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }

        if let Some(tools) = &req.tools {
            body["tools"] = tools.clone();
        }

        body["tool_choice"] = match &req.tool_choice {
            ToolChoice::Auto => serde_json::json!({"type": "auto"}),
            ToolChoice::None => serde_json::json!({"type": "none"}),
            ToolChoice::Required => serde_json::json!({"type": "any"}),
            ToolChoice::Specific(name) => {
                serde_json::json!({"type": "tool", "name": name})
            }
        };

        body
    }

    #[tracing::instrument(
        name = "gen_ai.request",
        skip_all,
        fields(
            otel.kind = "client",
            gen_ai.provider.name = "anthropic",
            gen_ai.request.model = %req.model,
            molesignal.gen_ai.streaming = stream
        )
    )]
    async fn send(&self, req: &CompletionRequest, stream: bool) -> Result<reqwest::Response> {
        req.validate_tool_choice()?;
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let body = self.body(req, stream);

        let resp = send_with_retry(self.transport, || {
            let request = self
                .client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .json(&body);

            crate::shared::http_trace::send(
                &self.client,
                request,
                crate::shared::http_trace::HttpTarget::ThirdParty,
            )
        })
        .await
        .map_err(|e| Error::internal(format!("anthropic request: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();

            return Err(Error::internal(format!(
                "anthropic status {status}: {body}"
            )));
        }

        Ok(resp)
    }
}

/// 把对话历史转成 Messages API 的 `system` 与 `messages`。
///
/// - 多条 system 消息按顺序合成一段，缺了哪一条模型都会少一部分指令；
/// - assistant 消息里的工具调用写成 `tool_use` 块，后面的 `tool_result` 才有对应；
/// - 紧挨着的多条工具结果放进同一条 user 消息，这是 API 对一次多工具调用的要求。
fn messages_body(messages: &[ChatMessage]) -> (Option<String>, Vec<serde_json::Value>) {
    let mut system: Vec<&str> = Vec::new();
    let mut out: Vec<serde_json::Value> = Vec::new();

    for m in messages {
        match m.role {
            MessageRole::System => {
                if !m.content.trim().is_empty() {
                    system.push(&m.content);
                }
            }
            MessageRole::User => {
                out.push(serde_json::json!({ "role": "user", "content": m.content }));
            }
            MessageRole::Assistant => {
                out.push(assistant_message(m));
            }
            MessageRole::Tool => {
                let block = serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.content,
                });

                let appended = out.last_mut().is_some_and(|last| {
                    let is_tool_results = last["role"] == "user"
                        && last["content"].as_array().is_some_and(|blocks| {
                            blocks.iter().all(|b| b["type"] == "tool_result")
                        });

                    if is_tool_results && let Some(blocks) = last["content"].as_array_mut() {
                        blocks.push(block.clone());
                    }

                    is_tool_results
                });

                if !appended {
                    out.push(serde_json::json!({ "role": "user", "content": [block] }));
                }
            }
        }
    }

    let system = (!system.is_empty()).then(|| system.join("\n\n"));

    (system, out)
}

/// assistant 消息：没有工具调用时是纯文本；有工具调用时是文本块加 `tool_use` 块。
fn assistant_message(m: &ChatMessage) -> serde_json::Value {
    let Some(calls) = m.tool_calls.as_array().filter(|calls| !calls.is_empty()) else {
        return serde_json::json!({ "role": "assistant", "content": m.content });
    };

    let mut blocks = Vec::with_capacity(calls.len() + 1);

    if !m.content.trim().is_empty() {
        blocks.push(serde_json::json!({ "type": "text", "text": m.content }));
    }

    for call in calls {
        // 历史里存的是 OpenAI 形状：`function.arguments` 是 JSON 字符串。
        let input = call["function"]["arguments"]
            .as_str()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));

        blocks.push(serde_json::json!({
            "type": "tool_use",
            "id": call["id"].as_str().unwrap_or_default(),
            "name": call["function"]["name"].as_str().unwrap_or_default(),
            "input": input,
        }));
    }

    serde_json::json!({ "role": "assistant", "content": blocks })
}

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn provider(&self) -> Provider {
        Provider::Anthropic
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let resp = self.send(&req, false).await?;

        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::internal(format!("anthropic decode: {e}")))?;

        let mut content = String::new();
        let mut tool_calls = Vec::new();

        if let Some(blocks) = v["content"].as_array() {
            for b in blocks {
                match b["type"].as_str() {
                    Some("text") => {
                        if let Some(t) = b["text"].as_str() {
                            content.push_str(t);
                        }
                    }
                    Some("tool_use") => {
                        tool_calls.push(crate::agent::chat::RequestedToolCall {
                            id: b["id"].as_str().unwrap_or_default().to_string(),
                            name: b["name"].as_str().unwrap_or_default().to_string(),
                            arguments: b["input"].clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        let (prompt_tokens, cached_prompt_tokens) = input_usage(&v["usage"]);

        Ok(CompletionResponse {
            content: Some(content),
            tool_calls,
            prompt_tokens,
            cached_prompt_tokens,
            completion_tokens: v["usage"]["output_tokens"].as_i64().unwrap_or(0) as i32,
            finish_reason: v["stop_reason"].as_str().unwrap_or("end_turn").to_string(),
        })
    }

    async fn complete_stream(&self, req: CompletionRequest) -> Result<ChatStream> {
        let resp = self.send(&req, true).await?;
        let mut sse = response_to_sse(resp);

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChunkOrToolCall>>(32);

        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            // content_block_start 时记录 index → (kind, id, name)；text_delta / input_json_delta 引用它
            let mut blocks: HashMap<i64, (String, String, String, String)> = HashMap::new();
            let mut input_tokens: i32 = 0;
            let mut cached_input_tokens: i32 = 0;
            let mut output_tokens: i32 = 0;

            while let Some(item) = sse.next().await {
                let evt = match item {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                };

                let v: serde_json::Value = match serde_json::from_str(&evt.data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                match evt.event.as_str() {
                    "message_start" => {
                        (input_tokens, cached_input_tokens) = input_usage(&v["message"]["usage"]);
                    }
                    "content_block_start" => {
                        let idx = v["index"].as_i64().unwrap_or(0);

                        let kind = v["content_block"]["type"]
                            .as_str()
                            .unwrap_or("text")
                            .to_string();

                        let id = v["content_block"]["id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();

                        let name = v["content_block"]["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();

                        blocks.insert(idx, (kind, id, name, String::new()));
                    }
                    "content_block_delta" => {
                        let idx = v["index"].as_i64().unwrap_or(0);
                        let dtype = v["delta"]["type"].as_str().unwrap_or("");

                        if dtype == "text_delta" {
                            if let Some(t) = v["delta"]["text"].as_str()
                                && !t.is_empty()
                            {
                                let _ = tx.send(Ok(ChunkOrToolCall::Text(t.to_string()))).await;
                            }
                        } else if dtype == "input_json_delta"
                            && let Some(partial) = v["delta"]["partial_json"].as_str()
                            && let Some(entry) = blocks.get_mut(&idx)
                        {
                            entry.3.push_str(partial);
                        }
                    }
                    "content_block_stop" => {
                        let idx = v["index"].as_i64().unwrap_or(0);

                        if let Some((kind, id, name, args)) = blocks.remove(&idx)
                            && kind == "tool_use"
                        {
                            let _ = tx
                                .send(Ok(ChunkOrToolCall::ToolCall {
                                    id,
                                    name,
                                    arguments: args,
                                }))
                                .await;
                        }
                    }
                    "message_delta" => {
                        if let Some(u) = v["usage"].as_object() {
                            output_tokens =
                                u.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
                        }
                    }
                    "message_stop" => {
                        let _ = tx
                            .send(Ok(ChunkOrToolCall::Usage {
                                prompt_tokens: input_tokens,
                                cached_prompt_tokens: cached_input_tokens,
                                completion_tokens: output_tokens,
                            }))
                            .await;

                        let _ = tx.send(Ok(ChunkOrToolCall::Done("stop".into()))).await;
                    }
                    _ => {}
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

pub fn from_env(env_prefix: &str) -> Result<Arc<AnthropicAdapter>> {
    let api_key_var = format!("{env_prefix}_API_KEY");
    let base_url_var = format!("{env_prefix}_BASE_URL");
    let api_key = std::env::var(&api_key_var)
        .map_err(|_| Error::invalid(format!("missing env {api_key_var}")))?;
    let base_url =
        std::env::var(&base_url_var).unwrap_or_else(|_| "https://api.anthropic.com".into());
    Ok(Arc::new(AnthropicAdapter::new(base_url, api_key)))
}

/// Anthropic 的 `input_tokens` 不含缓存：读缓存的记在 `cache_read_input_tokens`，写缓存的记在
/// `cache_creation_input_tokens`。返回全部输入（三者之和）与其中命中缓存的部分。
fn input_usage(usage: &serde_json::Value) -> (i32, i32) {
    let count = |key: &str| usage[key].as_i64().unwrap_or(0).max(0);
    let read = count("cache_read_input_tokens");
    let total = count("input_tokens") + read + count("cache_creation_input_tokens");

    (total as i32, read as i32)
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;

    fn mk_req() -> CompletionRequest {
        use crate::{
            agent::chat::{ChatMessage, MessageRole},
            shared::{ids::Id, time::TimestampMicros},
        };
        CompletionRequest {
            model: "claude-3-5-sonnet-20241022".into(),
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
            max_tokens: Some(512),
            temperature: None,
        }
    }

    fn tool_req(choice: ToolChoice) -> CompletionRequest {
        let mut req = mk_req();
        req.tools = Some(serde_json::json!([{
            "name": "prepare_dashboard",
            "description": "Prepare a Dashboard",
            "input_schema": {"type": "object"}
        }]));
        req.tool_choice = choice;
        req
    }

    fn message(role: MessageRole, content: &str) -> ChatMessage {
        use crate::shared::{ids::Id, time::TimestampMicros};

        ChatMessage {
            id: Id::new(),
            chat_id: Id("s".into()),
            role,
            content: content.into(),
            tool_call_id: None,
            tool_calls: serde_json::Value::Null,
            created_at: TimestampMicros(0),
            prompt_tokens: None,
            completion_tokens: None,
        }
    }

    #[test]
    fn every_system_message_reaches_the_system_prompt() {
        let (system, messages) = messages_body(&[
            message(MessageRole::System, "You are Mole Agent."),
            message(MessageRole::System, "Advice only."),
            message(MessageRole::User, "why is checkout slow?"),
            message(MessageRole::System, "Answer in JSON."),
        ]);

        assert_eq!(
            system.as_deref(),
            Some("You are Mole Agent.\n\nAdvice only.\n\nAnswer in JSON.")
        );
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn tool_calls_in_history_become_tool_use_blocks_with_their_results() {
        let mut assistant = message(MessageRole::Assistant, "Checking the logs.");
        assistant.tool_calls = serde_json::json!([
            {"id": "toolu_1", "type": "function",
             "function": {"name": "query_logs", "arguments": "{\"sql\":\"SELECT 1\"}"}},
            {"id": "toolu_2", "type": "function",
             "function": {"name": "list_streams", "arguments": ""}}
        ]);

        let mut first = message(MessageRole::Tool, "[{\"type\":\"text\",\"text\":\"1\"}]");
        first.tool_call_id = Some("toolu_1".into());
        let mut second = message(MessageRole::Tool, "[]");
        second.tool_call_id = Some("toolu_2".into());

        let (_, messages) = messages_body(&[
            message(MessageRole::User, "how many errors?"),
            assistant,
            first,
            second,
        ]);

        assert_eq!(messages.len(), 3, "{messages:#?}");

        let blocks = messages[1]["content"].as_array().unwrap();
        assert_eq!(
            blocks[0],
            serde_json::json!({"type": "text", "text": "Checking the logs."})
        );
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["id"], "toolu_1");
        assert_eq!(blocks[1]["name"], "query_logs");
        assert_eq!(blocks[1]["input"], serde_json::json!({"sql": "SELECT 1"}));
        assert_eq!(blocks[2]["input"], serde_json::json!({}));

        let results = messages[2]["content"].as_array().unwrap();
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["tool_use_id"], "toolu_1");
        assert_eq!(results[1]["tool_use_id"], "toolu_2");
    }

    #[test]
    fn max_tokens_defaults_to_room_for_a_full_answer() {
        let adapter = AnthropicAdapter::new("http://localhost", "test");
        let mut req = mk_req();
        req.max_tokens = None;

        assert_eq!(adapter.body(&req, false)["max_tokens"], DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn maps_all_tool_choice_modes_into_anthropic_request_body() {
        let adapter = AnthropicAdapter::new("http://localhost", "test");
        let cases = [
            (ToolChoice::Auto, serde_json::json!({"type": "auto"})),
            (ToolChoice::None, serde_json::json!({"type": "none"})),
            (ToolChoice::Required, serde_json::json!({"type": "any"})),
            (
                ToolChoice::Specific("prepare_dashboard".into()),
                serde_json::json!({"type": "tool", "name": "prepare_dashboard"}),
            ),
        ];
        for (choice, expected) in cases {
            let req = tool_req(choice);
            req.validate_tool_choice().unwrap();
            assert_eq!(adapter.body(&req, false)["tool_choice"], expected);
        }
    }

    #[tokio::test]
    async fn parses_message_delta_text() {
        let srv = MockServer::start().await;
        let body = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"usage\":{\"input_tokens\":5}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .mount(&srv)
            .await;
        let ad = AnthropicAdapter::new(srv.uri(), "test");
        let mut stream = ad.complete_stream(mk_req()).await.unwrap();
        let mut text = String::new();
        let mut usage = (0, 0);
        let mut done = None;
        while let Some(item) = stream.next().await {
            match item.unwrap() {
                ChunkOrToolCall::Text(t) => text.push_str(&t),
                ChunkOrToolCall::Usage {
                    prompt_tokens,
                    completion_tokens,
                    ..
                } => usage = (prompt_tokens, completion_tokens),
                ChunkOrToolCall::Done(r) => done = Some(r),
                ChunkOrToolCall::ToolCall { .. } => panic!("unexpected"),
            }
        }
        assert_eq!(text, "hi");
        assert_eq!(usage, (5, 1));
        assert_eq!(done.as_deref(), Some("stop"));
    }

    #[tokio::test]
    async fn parses_input_json_delta_tool_use() {
        let srv = MockServer::start().await;
        let body = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"usage\":{\"input_tokens\":5}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_a\",\"name\":\"get_w\",\"input\":{}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"loc\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\":\\\"SF\\\"}\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":1}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .mount(&srv)
            .await;
        let ad = AnthropicAdapter::new(srv.uri(), "test");
        let mut stream = ad.complete_stream(mk_req()).await.unwrap();
        let mut got_tool: Option<(String, String, String)> = None;
        while let Some(item) = stream.next().await {
            if let ChunkOrToolCall::ToolCall {
                id,
                name,
                arguments,
            } = item.unwrap()
            {
                got_tool = Some((id, name, arguments));
            }
        }
        let (id, name, args) = got_tool.expect("expected tool call");
        assert_eq!(id, "toolu_a");
        assert_eq!(name, "get_w");
        assert_eq!(args, "{\"loc\":\"SF\"}");
    }

    #[test]
    fn input_counts_the_cache_and_says_what_was_read_from_it() {
        let usage = serde_json::json!({
            "input_tokens": 20,
            "cache_read_input_tokens": 300,
            "cache_creation_input_tokens": 50,
            "output_tokens": 7
        });
        assert_eq!(input_usage(&usage), (370, 300));
        assert_eq!(input_usage(&serde_json::json!({"input_tokens": 5})), (5, 0));
    }
}
