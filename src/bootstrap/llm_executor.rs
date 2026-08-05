// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! LLM 评估节点的 ingest pipeline 执行器（`FunctionExecutor` 的一路实装）。
//!
//! 放 bootstrap：需要 intelligence adapter + AI provider repo（解密 key），infra 不依赖
//! intelligence。由 bootstrap 在 `[functions].llm_eval_enabled = true` 时注入
//! `ChainedFunctionExecutor`。
//!
//! 语义：`function.source` 是给模型的系统指令；当前事件 JSON 作 user 输入；模型文本
//! 结果写回事件的 `params_schema.output_field` 字段（缺省 `_llm_eval`）。
//!
//! ⚠️ **每条命中事件触发一次模型调用** —— 成本/延迟随 ingest 量线性放大，仅宜用于低
//! 吞吐流。失败（未配 provider / 模型报错）按单步失败处理：该 event 进 `IngestResult.errors`。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    api::rca::pick_provider,
    app::ingestion::FunctionExecutor,
    domain::function::Function,
    infra::persistence::repositories::intelligence::model_providers::ModelProviderRepository,
    intelligence::chat::{
        ChatMessage, CompletionRequest, MessageRole, Provider, adapter_from_parts,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 模型结果默认写回的事件字段。
const DEFAULT_OUTPUT_FIELD: &str = "_llm_eval";

pub struct LlmFunctionExecutor {
    intelligence_model_providers: Arc<dyn ModelProviderRepository>,
    default_max_tokens: i32,
    temperature: f32,
}

impl LlmFunctionExecutor {
    pub fn new(intelligence_model_providers: Arc<dyn ModelProviderRepository>) -> Self {
        Self {
            intelligence_model_providers,
            default_max_tokens: 512,
            temperature: 0.0,
        }
    }
}

#[async_trait]
impl FunctionExecutor for LlmFunctionExecutor {
    async fn run(&self, function: &Function, event: &mut Value) -> Result<()> {
        // 选 org 的可用 provider（enabled + key_set）。
        let providers = self
            .intelligence_model_providers
            .list(&function.org_id)
            .await?;
        let provider = pick_provider(&providers).ok_or_else(|| {
            Error::invalid(
                "llm eval: no enabled AI provider with an API key configured for this org",
            )
        })?;
        let key = self
            .intelligence_model_providers
            .get_plaintext_key(&function.org_id, &provider.id)
            .await?
            .ok_or_else(|| Error::invalid("llm eval: provider key not set"))?;
        let adapter = adapter_from_parts(
            Provider::parse(&provider.provider)?,
            provider.base_url.clone(),
            key,
        )?;

        let now = TimestampMicros::now();
        // 指令 = function.source；输入 = 当前事件 JSON（紧凑）。
        let input = serde_json::to_string(event).unwrap_or_default();
        let req = CompletionRequest {
            model: provider.default_model.clone(),
            messages: vec![
                msg(MessageRole::System, function.source.clone(), now),
                msg(MessageRole::User, input, now),
            ],
            tools: None,
            tool_choice: crate::intelligence::chat::ToolChoice::None,
            max_tokens: provider
                .max_tokens
                .map(|m| m as i32)
                .or(Some(self.default_max_tokens)),
            temperature: Some(self.temperature),
        };
        let resp = adapter.complete(req).await?;
        let text = resp.content.unwrap_or_default();

        // 写回结果到配置的输出字段（缺省 `_llm_eval`）。
        let field = output_field(&function.params_schema);
        if let Value::Object(map) = event {
            map.insert(field, Value::String(text));
        }
        Ok(())
    }
}

/// 从 `params_schema.output_field` 取结果写回字段名；缺省 / 空 → `_llm_eval`。
fn output_field(params_schema: &Value) -> String {
    params_schema
        .get("output_field")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_OUTPUT_FIELD)
        .to_string()
}

fn msg(role: MessageRole, content: String, now: TimestampMicros) -> ChatMessage {
    ChatMessage {
        id: Id::new(),
        chat_id: Id(String::new()),
        role,
        content,
        tool_call_id: None,
        tool_calls: Value::Null,
        created_at: now,
        prompt_tokens: None,
        completion_tokens: None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn output_field_defaults_and_overrides() {
        assert_eq!(output_field(&json!({})), "_llm_eval");
        assert_eq!(output_field(&json!({"output_field": ""})), "_llm_eval");
        assert_eq!(output_field(&json!({"output_field": "  "})), "_llm_eval");
        assert_eq!(
            output_field(&json!({"output_field": "severity"})),
            "severity"
        );
    }
}
