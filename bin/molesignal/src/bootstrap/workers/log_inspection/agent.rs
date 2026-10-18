// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时巡检调模型的那一段：取模型配置和密钥，组好消息，跑一遍带工具的 Agent 循环。
//!
//! 用的是 Mole Agent 聊天的同一个 `AgentLoop`（非流式），工具入口换成 [`super::dispatcher`]
//! 的 automation adapter。总超时不在这里：由 `app::log_inspection::InspectionRunner` 包在
//! 整个调用外面。每次模型调用都记进组织的每日用量；组织今天的预算用完时不调用模型。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use super::dispatcher::{AutomationToolDispatcher, OrgToolPolicy};
use crate::{
    agent::{
        chat::{
            AgentLoop, ChatMessage, DEFAULT_TEMPERATURE, LoopLimits, MessageRole, Provider,
            ProviderAdapter, TransportOptions, adapter_from_parts,
        },
        tool_control::ToolControlRepository,
        tools::{AgentExecutionPolicy, ToolAuthContext},
        usage::UsageOrigin,
    },
    app::{
        agent_usage::{AgentUsage, BudgetGate, UsageScope},
        log_inspection::{AgentAnswer, AgentFailure, AgentRequest, InspectionAgent},
        tools::ToolRuntime,
    },
    infra::persistence::repositories::agent::model_providers::{
        ModelProvider, ModelProviderRepository,
    },
    shared::{Error, ids::Id, time::TimestampMicros},
};

/// 一次巡检最多执行这么多次工具调用，加上收尾那次，模型最多被调用 9 次。
const INSPECTION_MAX_TOOL_CALLS: usize = 8;

pub struct ModelAgent {
    providers: Arc<dyn ModelProviderRepository>,
    tool_control: Arc<dyn ToolControlRepository>,
    tools: Arc<ToolRuntime>,
    /// 模型调用的用量记到组织的每日用量与 `_agent_model_traces`，与聊天的用量在同一处统计。
    usage: Arc<AgentUsage>,
}

/// 一个能用的模型配置。
struct UsableModel {
    row: ModelProvider,
    adapter: Arc<dyn ProviderAdapter>,
}

impl ModelAgent {
    pub fn new(
        providers: Arc<dyn ModelProviderRepository>,
        tool_control: Arc<dyn ToolControlRepository>,
        tools: Arc<ToolRuntime>,
        usage: Arc<AgentUsage>,
    ) -> Self {
        Self {
            providers,
            tool_control,
            tools,
            usage,
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

        let adapter = adapter_from_parts(
            provider,
            row.base_url.clone(),
            key,
            TransportOptions::from_timeout_ms(row.timeout_ms),
        )
        .map_err(|error| AgentFailure::Model(error.to_string()))?;

        Ok(UsableModel { row, adapter })
    }
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

        // 巡检没有人在等：组织今天的预算用完就不调用模型。
        if let BudgetGate::Refuse(status) = self.usage.gate(&org_id, false).await {
            return Err(AgentFailure::Budget(format!(
                "daily token budget exhausted: {} of {} tokens used",
                status.used_tokens, status.limit_tokens
            )));
        }

        let usable = self.usable_model(&org_id, &request.provider_id).await?;

        let model = request
            .model
            .clone()
            .unwrap_or_else(|| usable.row.default_model.clone());

        let policy = OrgToolPolicy::load(self.tool_control.as_ref(), &org_id)
            .await
            .map_err(|error| AgentFailure::Call(error.to_string()))?;

        // 组织把这些工具都停用时不带工具清单：模型只能如实报告它读不到日志。
        let provider = usable.adapter.provider();

        let tools_schema = Some(policy.tools_schema(provider))
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

        let recorder = self.usage.recorder(UsageScope {
            user_id: Some(request.identity.user_id.clone()),
            provider_id: Some(request.provider_id.clone()),
            prices: usable.row.prices(),
            ..UsageScope::new(org_id.clone(), UsageOrigin::Inspection)
        });

        let outcome = AgentLoop::new(usable.adapter, dispatcher)
            .with_limits(LoopLimits {
                max_tool_calls: INSPECTION_MAX_TOOL_CALLS,
                max_duration: None,
                temperature: Some(DEFAULT_TEMPERATURE),
                context_tokens: None,
            })
            .with_observer(recorder.clone())
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

        match outcome {
            Ok(result) => Ok(AgentAnswer {
                content: result.assistant_content,
                prompt_tokens: i64::from(result.prompt_tokens_total),
                completion_tokens: i64::from(result.completion_tokens_total),
                model,
            }),
            Err(error) => Err(AgentFailure::Call(error.to_string())),
        }
    }
}
