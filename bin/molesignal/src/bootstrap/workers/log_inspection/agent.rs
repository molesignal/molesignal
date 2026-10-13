// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时巡检调模型的那一段：取模型配置和密钥，组好消息，跑一遍带工具的 Agent 循环。
//!
//! 用的是 Mole Agent 聊天的同一个 `AgentLoop`（非流式），工具入口换成 [`super::dispatcher`]
//! 的 automation adapter。总超时不在这里：由 `app::log_inspection::InspectionRunner` 包在
//! 整个调用外面。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::dispatcher::{AutomationToolDispatcher, OrgToolPolicy};
use crate::{
    agent::{
        chat::{
            AgentLoop, ChatMessage, MessageRole, Provider, ProviderAdapter, adapter_from_parts,
        },
        telemetry::AGENT_STREAM,
        tool_control::ToolControlRepository,
        tools::{AgentExecutionPolicy, ToolAuthContext},
    },
    app::{
        intake::IntakeService,
        log_inspection::{AgentAnswer, AgentFailure, AgentRequest, InspectionAgent},
        tools::ToolRuntime,
    },
    domain::{
        intake::{IntakeBatch, RawEvent},
        stream::StreamType,
    },
    infra::persistence::repositories::agent::model_providers::{
        ModelProvider, ModelProviderRepository,
    },
    shared::{Error, ids::Id, time::TimestampMicros},
};

pub struct ModelAgent {
    providers: Arc<dyn ModelProviderRepository>,
    tool_control: Arc<dyn ToolControlRepository>,
    tools: Arc<ToolRuntime>,
    /// 模型调用的用量记到 `_agent_model_traces`，与聊天的用量在同一处统计。
    intake: Arc<IntakeService>,
}

/// 一个能用的模型配置。
struct UsableModel {
    row: ModelProvider,
    provider: Provider,
    adapter: Arc<dyn ProviderAdapter>,
}

impl ModelAgent {
    pub fn new(
        providers: Arc<dyn ModelProviderRepository>,
        tool_control: Arc<dyn ToolControlRepository>,
        tools: Arc<ToolRuntime>,
        intake: Arc<IntakeService>,
    ) -> Self {
        Self {
            providers,
            tool_control,
            tools,
            intake,
        }
    }

    /// 模型配置在这个组织里、已启用、有密钥，并且能据此构造出 adapter。
    async fn usable_model(
        &self,
        org_id: &Id,
        provider_id: &Id,
    ) -> Result<UsableModel, AgentFailure> {
        let row = match self.providers.get(org_id, provider_id).await {
            Ok(row) => row,
            Err(Error::NotFound(_)) => {
                return Err(AgentFailure::Model(format!(
                    "model provider `{}` does not exist",
                    provider_id.0
                )));
            }
            Err(error) => return Err(AgentFailure::Call(error.to_string())),
        };

        if !row.enabled {
            return Err(AgentFailure::Model(format!(
                "model provider `{}` is disabled",
                row.name
            )));
        }

        let provider = Provider::parse(&row.provider)
            .map_err(|error| AgentFailure::Model(error.to_string()))?;

        let key = self
            .providers
            .get_plaintext_key(org_id, provider_id)
            .await
            .map_err(|error| AgentFailure::Call(error.to_string()))?
            .ok_or_else(|| {
                AgentFailure::Model(format!("model provider `{}` has no API key", row.name))
            })?;

        let adapter = adapter_from_parts(provider, row.base_url.clone(), key)
            .map_err(|error| AgentFailure::Model(error.to_string()))?;

        Ok(UsableModel {
            row,
            provider,
            adapter,
        })
    }

    /// 尽力而为：记不上不影响巡检本身。
    async fn record_usage(&self, usage: &Usage<'_>) {
        let event = json!({
            "source": "log_inspection",
            "user_id": usage.user_id.0,
            "provider": usage.provider.as_str(),
            "provider_id": usage.provider_id.0,
            "model": usage.model,
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
            "total_tokens": usage.prompt_tokens + usage.completion_tokens,
            "error": usage.error,
            "tool_calls": usage.tool_calls,
            "gen_ai_system": usage.provider.as_str(),
            "gen_ai_request_model": usage.model,
        });

        let now = TimestampMicros::now();

        let recorded = self
            .intake
            .intake_internal_telemetry(IntakeBatch {
                batch_id: Id::new(),
                org_id: usage.org_id.clone(),
                stream: AGENT_STREAM.into(),
                stream_type: StreamType::LOGS,
                events: vec![RawEvent {
                    timestamp: now,
                    fields: event.as_object().cloned().unwrap_or_default(),
                }],
                received_at: now,
            })
            .await;

        if let Err(error) = recorded {
            tracing::debug!(%error, "log inspection model usage was not recorded");
        }
    }
}

/// 一次模型调用的用量。
struct Usage<'a> {
    org_id: &'a Id,
    user_id: &'a Id,
    provider: Provider,
    provider_id: &'a Id,
    model: &'a str,
    prompt_tokens: i64,
    completion_tokens: i64,
    tool_calls: usize,
    error: Option<String>,
}

fn message(role: MessageRole, content: String) -> ChatMessage {
    ChatMessage {
        id: Id::new(),
        chat_id: Id(String::new()),
        role,
        content,
        tool_call_id: None,
        tool_calls: Value::Null,
        created_at: TimestampMicros::now(),
        prompt_tokens: None,
        completion_tokens: None,
    }
}

#[async_trait]
impl InspectionAgent for ModelAgent {
    async fn check_model(&self, org_id: &Id, provider_id: &Id) -> Result<(), AgentFailure> {
        self.usable_model(org_id, provider_id).await.map(|_| ())
    }

    async fn inspect(&self, request: AgentRequest) -> Result<AgentAnswer, AgentFailure> {
        let org_id = request.identity.org_id.clone();
        let usable = self.usable_model(&org_id, &request.provider_id).await?;

        let model = request
            .model
            .clone()
            .unwrap_or_else(|| usable.row.default_model.clone());

        let policy = OrgToolPolicy::load(self.tool_control.as_ref(), &org_id)
            .await
            .map_err(|error| AgentFailure::Call(error.to_string()))?;

        // 组织把这些工具都停用时不带工具清单：模型只能如实报告它读不到日志。
        let tools_schema = Some(policy.tools_schema(usable.provider))
            .filter(|tools| tools.as_array().is_some_and(|tools| !tools.is_empty()));

        let dispatcher = Arc::new(AutomationToolDispatcher::new(
            self.tools.clone(),
            policy,
            request.stream.clone(),
            request.tool_calls.clone(),
        ));

        let mut history = request
            .instructions
            .into_iter()
            .map(|instruction| message(MessageRole::System, instruction))
            .collect::<Vec<_>>();

        history.push(message(MessageRole::User, request.task));

        // 只读：没有人在场审批，不允许提出任何变更。
        let context = ToolAuthContext::from_iam(&request.identity)
            .with_execution_policy(AgentExecutionPolicy::ReadOnly);

        let outcome = AgentLoop::new(usable.adapter, dispatcher)
            .run(
                &context,
                model.clone(),
                history,
                tools_schema,
                usable
                    .row
                    .max_tokens
                    .map(|tokens| i32::try_from(tokens).unwrap_or(i32::MAX)),
            )
            .await;

        let mut usage = Usage {
            org_id: &org_id,
            user_id: &request.identity.user_id,
            provider: usable.provider,
            provider_id: &request.provider_id,
            model: &model,
            prompt_tokens: 0,
            completion_tokens: 0,
            tool_calls: request.tool_calls.calls().len(),
            error: None,
        };

        match outcome {
            Ok(result) => {
                usage.prompt_tokens = i64::from(result.prompt_tokens_total);
                usage.completion_tokens = i64::from(result.completion_tokens_total);
                self.record_usage(&usage).await;

                Ok(AgentAnswer {
                    content: result.assistant_content,
                    prompt_tokens: usage.prompt_tokens,
                    completion_tokens: usage.completion_tokens,
                    model,
                })
            }
            Err(error) => {
                usage.error = Some(error.to_string());
                self.record_usage(&usage).await;

                Err(AgentFailure::Call(error.to_string()))
            }
        }
    }
}
