// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OpenAI-compatible Mole Intelligence Model Gateway adapter。
//!
//! Together / vLLM / Groq / Ollama 等：协议与 OpenAI v1/chat/completions 一致，
//! 只差 `base_url` 与 API key。直接 wrap [`OpenAiAdapter`]。

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    intelligence::chat::{
        ChatStream, CompletionRequest, CompletionResponse, Provider, ProviderAdapter,
        providers::openai::OpenAiAdapter,
    },
    shared::Result,
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleAdapter {
    inner: OpenAiAdapter,
}

impl OpenAiCompatibleAdapter {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            inner: OpenAiAdapter::new(base_url, api_key),
        }
    }
}

#[async_trait]
impl ProviderAdapter for OpenAiCompatibleAdapter {
    fn provider(&self) -> Provider {
        Provider::OpenAiCompatible
    }
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        self.inner.complete(req).await
    }
    async fn complete_stream(&self, req: CompletionRequest) -> Result<ChatStream> {
        self.inner.complete_stream(req).await
    }
}

pub fn from_env(env_prefix: &str) -> Result<Arc<OpenAiCompatibleAdapter>> {
    use crate::shared::Error;
    let api_key_var = format!("{env_prefix}_API_KEY");
    let base_url_var = format!("{env_prefix}_BASE_URL");
    let api_key = std::env::var(&api_key_var)
        .map_err(|_| Error::invalid(format!("missing env {api_key_var}")))?;
    let base_url = std::env::var(&base_url_var)
        .map_err(|_| Error::invalid(format!("missing env {base_url_var}")))?;
    Ok(Arc::new(OpenAiCompatibleAdapter::new(base_url, api_key)))
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;
    use crate::{
        intelligence::chat::{ChunkOrToolCall, MessageRole, ToolChoice},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn tool_req(choice: ToolChoice) -> CompletionRequest {
        CompletionRequest {
            model: "llama-3".into(),
            messages: Vec::new(),
            tools: Some(serde_json::json!([{
                "type": "function",
                "function": {
                    "name": "prepare_dashboard",
                    "parameters": {"type": "object"}
                }
            }])),
            tool_choice: choice,
            max_tokens: None,
            temperature: None,
        }
    }

    #[test]
    fn maps_all_tool_choice_modes_into_compatible_request_body() {
        let adapter = OpenAiCompatibleAdapter::new("http://localhost", "test");
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
            assert_eq!(adapter.inner.body(&req, false)["tool_choice"], expected);
        }
    }

    #[tokio::test]
    async fn compatible_adapter_reuses_openai_parser() {
        let srv = MockServer::start().await;
        let body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"yo\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1}}\n\n",
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
        let ad = OpenAiCompatibleAdapter::new(srv.uri(), "test");
        let req = crate::intelligence::chat::CompletionRequest {
            model: "llama-3".into(),
            messages: vec![crate::intelligence::chat::ChatMessage {
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
            tool_choice: crate::intelligence::chat::ToolChoice::Auto,
            max_tokens: None,
            temperature: None,
        };
        let mut stream = ad.complete_stream(req).await.unwrap();
        let mut text = String::new();
        while let Some(item) = stream.next().await {
            if let ChunkOrToolCall::Text(t) = item.unwrap() {
                text.push_str(&t);
            }
        }
        assert_eq!(text, "yo");
        assert_eq!(ad.provider(), Provider::OpenAiCompatible);
    }
}
