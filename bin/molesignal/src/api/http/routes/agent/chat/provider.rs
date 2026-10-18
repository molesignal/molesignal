// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮对话用哪个模型服务：显式指定 → 会话上次用的 → 组织里第一个启用的；都没有时退回
//! 开发用的环境变量。模型服务的超时与单价跟着一起带出来。

use std::sync::Arc;

use crate::{
    agent::{
        chat::{Provider, ProviderAdapter, TransportOptions, adapter_from_env, adapter_from_parts},
        usage::ModelPrices,
    },
    api::AppState,
    app::iam::IamContext,
    infra::persistence::repositories::agent::chats::Chat,
    shared::{Error, Result, ids::Id},
};

/// 解析出的 provider adapter + 运行参数。
pub(super) struct ResolvedProvider {
    pub(super) model: String,
    pub(super) adapter: Arc<dyn ProviderAdapter>,
    pub(super) max_tokens: Option<i32>,
    pub(super) provider: Provider,
    pub(super) provider_id: Option<String>,
    /// 模型服务上填的单价；没有时不计算成本。
    pub(super) prices: Option<ModelPrices>,
}

pub(super) async fn resolve_provider(
    state: &AppState,
    ctx: &IamContext,
    chat: &Chat,
    explicit_provider_id: Option<&str>,
    explicit_model: Option<&str>,
) -> Result<ResolvedProvider> {
    // 1) 找 provider 行。
    let row = if let Some(pid) = explicit_provider_id {
        Some(
            state
                .agent
                .model_providers
                .get(&ctx.org_id, &Id(pid.to_string()))
                .await?,
        )
    } else if let Some(pid) = chat.provider_id.as_deref() {
        state
            .agent
            .model_providers
            .get(&ctx.org_id, &Id(pid.to_string()))
            .await
            .ok()
    } else {
        state
            .agent
            .model_providers
            .list(&ctx.org_id)
            .await?
            .into_iter()
            .find(|p| p.enabled)
    };

    if let Some(p) = row {
        if !p.enabled {
            return Err(Error::invalid(format!(
                "model provider `{}` is disabled",
                p.id.0
            )));
        }

        let provider = Provider::parse(&p.provider)?;

        let key = state
            .agent
            .model_providers
            .get_plaintext_key(&ctx.org_id, &p.id)
            .await?
            .ok_or_else(|| Error::invalid(format!("model provider `{}` has no API key", p.id.0)))?;

        let adapter = adapter_from_parts(
            provider,
            p.base_url.clone(),
            key,
            TransportOptions::from_timeout_ms(p.timeout_ms),
        )?;

        let model = explicit_model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if chat.model.is_empty() {
                    p.default_model.clone()
                } else {
                    chat.model.clone()
                }
            });

        return Ok(ResolvedProvider {
            model,
            adapter,
            max_tokens: p.max_tokens.map(|m| m as i32),
            provider,
            prices: p.prices(),
            provider_id: Some(p.id.0),
        });
    }

    // 2) env fallback（dev）。
    let provider = Provider::parse(&chat.provider)?;
    let adapter = adapter_from_env(provider)?;

    let model = explicit_model
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(chat.model.as_str())
        .to_string();

    Ok(ResolvedProvider {
        model,
        adapter,
        max_tokens: None,
        provider,
        provider_id: None,
        prices: None,
    })
}
