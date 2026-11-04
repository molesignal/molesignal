// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 调模型：评审打分，以及晋级检查里的重新回答。
//!
//! 评审用它指定的模型服务与模型，没有指定时用组织第一个可用的模型服务与它的默认模型。system
//! 是组织的 `evaluation` Prompt，填进评分标准与写理由用的语言，后面接上固定的输出格式。用量都记
//! 在 `evaluation` 名下。

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::{
    agent::{
        chat::{
            ChatMessage, CompletionRequest, MessageRole, ModelCallObserver, Provider,
            ProviderAdapter, ToolChoice, TransportOptions, adapter_from_parts, observed_complete,
        },
        scoring::{
            Evaluator,
            evaluation::{
                EVALUATION_PURPOSE, Material, VERDICT_INSTRUCTION, Verdict, material_text,
                output_language, parse_verdict,
            },
        },
        usage::UsageOrigin,
    },
    api::{
        AppState,
        rca::{pick_provider, system_user_id},
    },
    app::agent_usage::UsageScope,
    infra::persistence::repositories::agent::{
        model_providers::ModelProvider,
        prompts::{prompt_hash, render_prompt},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 评审回答最多这么多 token：一个分数和两句理由。
const VERDICT_TOKENS: i32 = 400;
/// 重新回答最多这么多 token。
const ANSWER_TOKENS: i32 = 2_000;

/// 评审给的分，和它用的模型与 token。
pub(super) struct Evaluated {
    pub(super) verdict: Verdict,
    pub(super) model: String,
    pub(super) tokens: i64,
}

/// 准备好调用的模型。
pub(super) struct Model {
    pub(super) provider: ModelProvider,
    pub(super) model: String,
    adapter: Arc<dyn ProviderAdapter>,
}

/// 组织里的一个模型服务：指定了就用它（要启用并填了 API key），否则用第一个可用的。
pub(super) async fn model(
    state: &AppState,
    org_id: &Id,
    provider_id: Option<&Id>,
    model: Option<&str>,
) -> Result<Model> {
    let provider = match provider_id {
        Some(id) => {
            let provider = state.agent.model_providers.get(org_id, id).await?;

            if !provider.enabled || !provider.key_set {
                return Err(Error::invalid(format!(
                    "model provider `{}` is off or has no API key",
                    provider.name
                )));
            }

            provider
        }
        None => {
            pick_provider(&state.agent.model_providers.list(org_id).await?).ok_or_else(|| {
                Error::invalid("no enabled AI model provider with an API key in this organization")
            })?
        }
    };

    let key = state
        .agent
        .model_providers
        .get_plaintext_key(org_id, &provider.id)
        .await?
        .ok_or_else(|| Error::invalid("the model provider has no API key"))?;

    let adapter = adapter_from_parts(
        Provider::parse(&provider.provider)?,
        provider.base_url.clone(),
        key,
        TransportOptions::from_timeout_ms(provider.timeout_ms),
    )?;

    let model = model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| provider.default_model.clone());

    Ok(Model {
        provider,
        model,
        adapter,
    })
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

/// 调一次模型：一条 system、一条 user，不给工具。返回回答与用掉的 token。
pub(super) async fn complete(
    state: &AppState,
    org_id: &Id,
    model: &Model,
    (system, user): (String, String),
    (max_tokens, temperature): (i32, f32),
) -> Result<(String, i64)> {
    let recorder: Arc<dyn ModelCallObserver> = state.agent.usage.recorder(UsageScope {
        provider_id: Some(model.provider.id.clone()),
        prices: model.provider.prices(),
        prompt_hash: Some(prompt_hash(&system)),
        ..UsageScope::new(org_id.clone(), UsageOrigin::Evaluation)
    });

    let request = CompletionRequest {
        model: model.model.clone(),
        messages: vec![
            message(MessageRole::System, system),
            message(MessageRole::User, user),
        ],
        tools: None,
        tool_choice: ToolChoice::None,
        max_tokens: Some(max_tokens),
        temperature: Some(temperature),
    };

    let response = observed_complete(model.adapter.as_ref(), request, Some(&recorder)).await?;
    let tokens = i64::from(response.prompt_tokens) + i64::from(response.completion_tokens);

    Ok((response.content.unwrap_or_default(), tokens))
}

/// 重新回答一次，给晋级检查用。
pub(super) async fn answer(
    state: &AppState,
    org_id: &Id,
    model: &Model,
    system: String,
    user: String,
) -> Result<(String, i64)> {
    complete(state, org_id, model, (system, user), (ANSWER_TOKENS, 0.2)).await
}

/// 评审评一份材料。
pub(super) async fn evaluate(
    state: &AppState,
    evaluator: &Evaluator,
    material: &Material<'_>,
) -> Result<Evaluated> {
    let model = model(
        state,
        &evaluator.org_id,
        evaluator.provider_id.as_ref(),
        evaluator.model.as_deref(),
    )
    .await?;

    let template = state
        .agent
        .prompts
        .resolve(&evaluator.org_id, &system_user_id(), EVALUATION_PURPOSE)
        .await?;

    let mut vars = Map::new();
    vars.insert("rubric".into(), Value::String(evaluator.rubric.clone()));
    vars.insert(
        "output_language".into(),
        Value::String(output_language(&evaluator.language).into()),
    );

    let system = format!(
        "{}\n\n{VERDICT_INSTRUCTION}",
        render_prompt(&template.body, &vars)
    );

    let (content, tokens) = complete(
        state,
        &evaluator.org_id,
        &model,
        (system, material_text(material)),
        (VERDICT_TOKENS, evaluator.temperature),
    )
    .await?;

    Ok(Evaluated {
        verdict: parse_verdict(&content)?,
        model: model.model,
        tokens,
    })
}
