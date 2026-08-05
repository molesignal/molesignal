// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Anthropic Messages API 的 Mole Intelligence Model Gateway adapter。
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
    intelligence::chat::{
        ChatStream, ChunkOrToolCall, CompletionRequest, CompletionResponse, MessageRole, Provider,
        ProviderAdapter, ToolChoice, providers::sse::response_to_sse,
    },
    shared::{Error, Result, trace_stream::segmented_result_stream},
};

pub const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct AnthropicAdapter {
    pub base_url: String,
    pub api_key: String,
    pub client: reqwest::Client,
}

impl AnthropicAdapter {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }

    fn body(&self, req: &CompletionRequest, stream: bool) -> serde_json::Value {
        let (system, messages): (Option<String>, Vec<serde_json::Value>) = {
            let mut sys: Option<String> = None;
            let mut out: Vec<serde_json::Value> = Vec::new();
            for m in &req.messages {
                match m.role {
                    MessageRole::System => {
                        sys = Some(m.content.clone());
                    }
                    MessageRole::User => {
                        out.push(serde_json::json!({ "role": "user", "content": m.content }));
                    }
                    MessageRole::Assistant => {
                        out.push(serde_json::json!({ "role": "assistant", "content": m.content }));
                    }
                    MessageRole::Tool => {
                        // Anthropic tool_result 走 user role + content blocks。
                        out.push(serde_json::json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                                "content": m.content,
                            }],
                        }));
                    }
                }
            }
            (sys, out)
        };
        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "max_tokens": req.max_tokens.unwrap_or(1024),
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
        let request = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&self.body(req, stream));
        let resp = crate::shared::http_trace::send(
            &self.client,
            request,
            crate::shared::http_trace::HttpTarget::ThirdParty,
        )
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
                        tool_calls.push(crate::intelligence::chat::RequestedToolCall {
                            id: b["id"].as_str().unwrap_or_default().to_string(),
                            name: b["name"].as_str().unwrap_or_default().to_string(),
                            arguments: b["input"].clone(),
                        });
                    }
                    _ => {}
                }
            }
        }
        Ok(CompletionResponse {
            content: Some(content),
            tool_calls,
            prompt_tokens: v["usage"]["input_tokens"].as_i64().unwrap_or(0) as i32,
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
                        if let Some(u) = v["message"]["usage"].as_object() {
                            input_tokens =
                                u.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
                        }
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
            intelligence::chat::{ChatMessage, MessageRole},
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
}
