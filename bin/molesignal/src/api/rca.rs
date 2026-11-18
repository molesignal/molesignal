// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Incident 根因分析（RCA）生成器：聚合 incident 跨信号上下文，调 LLM 产出根因摘要。
//!
//! 由两处复用，逻辑单点不漂移：
//! - 后台 `rca_sweeper`（bootstrap，依赖 api）周期对活跃 incident 自动生成；
//! - `POST /alerts/incidents/{id}/rca`（HTTP）按需手动触发。
//!
//! provider key 解密即用即弃，绝不落库/日志；provider/model/token/prompt_hash 一并写

use std::sync::Arc;

use agent::{
    chat::{
        ChatMessage, CompletionRequest, MessageRole, ModelCallObserver, Provider, TransportOptions,
        adapter_from_parts, observed_complete,
    },
    scoring::{ScoreTarget, ScoringRepository, TargetSource},
    usage::UsageOrigin,
};
use domain::alerting::{
    incident::{Incident, IncidentRca},
    repositories::IncidentRcaRepository,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use postgres::repositories::agent::{
    model_providers::{ModelProvider, ModelProviderRepository},
    prompts::{AgentPromptRepository, AgentPromptTemplate, prompt_hash, render_prompt},
};
use serde_json::Value;

use crate::{
    api::AppState,
    app::{
        agent_usage::{AgentUsage, BudgetGate, UsageScope},
        alerting::related::IncidentBriefing,
        scoring::enqueue_answered,
    },
};

mod evidence;

pub(crate) use evidence::incident_facts;
use evidence::{build_evidence, render_vars};

/// root_cause prompt 的 purpose（与迁移 seed 的 builtin 行一致）。
pub(crate) const RCA_PURPOSE: &str = "root_cause";

/// How long an incident's brief may take to read before the analysis goes on without it.
const BRIEF_WAIT: std::time::Duration = std::time::Duration::from_secs(8);

/// RCA 只支持产品内已经发布的界面语言。调用方传入的 BCP-47 tag 必须先归一化
/// 为该枚举，不能把任意用户输入直接拼接进 system prompt。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RcaOutputLocale {
    EnUs,
    ZhCn,
}

impl RcaOutputLocale {
    pub fn from_language_tag(value: &str) -> Self {
        if value.trim().to_ascii_lowercase().starts_with("zh") {
            Self::ZhCn
        } else {
            Self::EnUs
        }
    }

    pub fn language_tag(self) -> &'static str {
        match self {
            Self::EnUs => "en-us",
            Self::ZhCn => "zh-cn",
        }
    }

    fn output_language(self) -> &'static str {
        match self {
            Self::EnUs => "English",
            Self::ZhCn => "Simplified Chinese",
        }
    }

    fn system_instruction(self) -> &'static str {
        match self {
            Self::EnUs => {
                "Output requirements:\n\
                 - Write all reader-facing headings, explanations, causes, and next steps in English.\n\
                 - Preserve technical identifiers, queries, field names, and URLs exactly when needed.\n\
                 - Return valid CommonMark Markdown. Do not wrap the whole response in a code fence."
            }
            Self::ZhCn => {
                "输出要求：\n\
                 - 所有面向用户的标题、说明、原因与后续步骤必须使用简体中文。\n\
                 - 必要的技术标识、查询语句、字段名与 URL 保持原样。\n\
                 - 返回有效的 CommonMark Markdown，不要用代码围栏包裹整篇回答。"
            }
        }
    }

    fn evidence_instruction(self) -> &'static str {
        match self {
            Self::EnUs => {
                "Based only on the evidence above, give a concise root-cause analysis: the most \
                 likely cause, the supporting evidence, and the first concrete next step to confirm \
                 or remediate. If the evidence is insufficient, say so explicitly."
            }
            Self::ZhCn => {
                "仅依据以上证据给出简洁的根因分析，包括：最可能的原因、支持证据，以及用于确认或修复的\
                 第一个具体步骤。若证据不足，必须明确说明。"
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RcaGenConfig {
    /// provider 未配 max_tokens 时的兜底。
    pub default_max_tokens: i32,
    pub temperature: f32,
}

impl Default for RcaGenConfig {
    fn default() -> Self {
        Self {
            default_max_tokens: 1024,
            temperature: 0.2,
        }
    }
}

/// RCA 生成器：持 provider/prompt/rca repos，封装"选 provider → 调 LLM → 写回"。
pub struct RcaGenerator {
    agent_model_providers: Arc<dyn ModelProviderRepository>,
    agent_prompts: Arc<dyn AgentPromptRepository>,
    incident_rca: Arc<dyn IncidentRcaRepository>,
    /// 每次生成记进组织的每日用量；预算由调用方在生成前用 [`Self::budget`] 判定。
    usage: Arc<AgentUsage>,
    cfg: RcaGenConfig,
    /// 生成的 RCA 排进打分。
    scoring: Option<Arc<dyn ScoringRepository>>,
    /// Where the incident's brief is read from: its rule's guide, the catalog of its services,
    /// similar incidents and nearby changes. Without it the evidence is the incident's own facts.
    briefing: Option<IncidentBriefing>,
}

/// 写好、还没存下的一份 RCA。
pub struct RcaDraft {
    pub summary: String,
    pub prompt_hash: String,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub finish_reason: String,
}

impl RcaGenerator {
    pub fn new(
        agent_model_providers: Arc<dyn ModelProviderRepository>,
        agent_prompts: Arc<dyn AgentPromptRepository>,
        incident_rca: Arc<dyn IncidentRcaRepository>,
        usage: Arc<AgentUsage>,
        cfg: RcaGenConfig,
    ) -> Self {
        Self {
            agent_model_providers,
            agent_prompts,
            incident_rca,
            usage,
            cfg,
            scoring: None,
            briefing: None,
        }
    }

    /// 生成的 RCA 排进打分。
    pub fn with_scoring(mut self, scoring: Arc<dyn ScoringRepository>) -> Self {
        self.scoring = Some(scoring);
        self
    }

    /// Adds the incident's brief to the evidence.
    pub fn with_briefing(mut self, briefing: IncidentBriefing) -> Self {
        self.briefing = Some(briefing);
        self
    }

    /// 从 [`AppState`] 装配（HTTP 按需触发用）。
    pub fn from_state(state: &AppState) -> Self {
        Self::new(
            state.agent.model_providers.clone(),
            state.agent.prompts.clone(),
            state.agent.incident_rca.clone(),
            state.agent.usage.clone(),
            RcaGenConfig::default(),
        )
        .with_scoring(state.agent.scoring.clone())
        .with_briefing(IncidentBriefing::new(
            state.alerting.related.clone(),
            state.telemetry.apm_query.clone(),
            state.telemetry.service_graph.clone(),
        ))
    }

    /// 组织今天的预算还允不允许生成。`interactive`：有人在事件页上等这次生成。
    pub async fn budget(&self, org_id: &Id, interactive: bool) -> BudgetGate {
        self.usage.gate(org_id, interactive).await
    }

    /// 列出并选 org 可用 provider（enabled + key_set，取最近更新的一个）。
    pub async fn pick_provider_for(&self, org_id: &Id) -> Result<Option<ModelProvider>> {
        let providers = self.agent_model_providers.list(org_id).await?;
        Ok(pick_provider(&providers))
    }

    /// 该 incident 是否已有 RCA（sweeper 跳过去重用）。
    pub async fn has_rca(&self, incident_id: &Id) -> Result<bool> {
        Ok(self.incident_rca.get(incident_id).await?.is_some())
    }

    /// 自动选 provider 生成（按需触发用）；无可用 provider → `Err(invalid)`。
    pub async fn generate(
        &self,
        org_id: &Id,
        incident: &Incident,
        now: TimestampMicros,
    ) -> Result<IncidentRca> {
        self.generate_for_locale(org_id, incident, now, RcaOutputLocale::EnUs)
            .await
    }

    /// 按指定产品语言自动选 provider 生成（HTTP 按需触发用）。
    pub async fn generate_for_locale(
        &self,
        org_id: &Id,
        incident: &Incident,
        now: TimestampMicros,
        locale: RcaOutputLocale,
    ) -> Result<IncidentRca> {
        let provider = self.pick_provider_for(org_id).await?.ok_or_else(|| {
            Error::invalid("no enabled AI provider with an API key is configured for this org")
        })?;

        self.generate_with_provider_for_locale(org_id, &provider, incident, now, locale)
            .await
    }

    /// 用指定 provider 生成（sweeper 已选好 provider，避免重复 list）。
    pub async fn generate_with_provider(
        &self,
        org_id: &Id,
        provider: &ModelProvider,
        incident: &Incident,
        now: TimestampMicros,
    ) -> Result<IncidentRca> {
        self.generate_with_provider_for_locale(
            org_id,
            provider,
            incident,
            now,
            RcaOutputLocale::EnUs,
        )
        .await
    }

    /// 用指定 provider 和产品语言生成。语言是受控枚举，避免 locale prompt injection。
    pub async fn generate_with_provider_for_locale(
        &self,
        org_id: &Id,
        provider: &ModelProvider,
        incident: &Incident,
        now: TimestampMicros,
        locale: RcaOutputLocale,
    ) -> Result<IncidentRca> {
        // 解析 root_cause prompt（user→org→builtin）作系统消息；哨兵 user 落 org/builtin 默认。
        let tmpl = self
            .agent_prompts
            .resolve(org_id, &system_user_id(), RCA_PURPOSE)
            .await?;

        let draft = self
            .draft(
                org_id,
                provider,
                &tmpl,
                incident,
                now,
                locale,
                UsageOrigin::Rca,
            )
            .await?;

        let rca = IncidentRca {
            incident_id: incident.id.clone(),
            org_id: org_id.clone(),
            summary: draft.summary,
            provider: Some(provider.provider.clone()),
            model: Some(provider.default_model.clone()),
            prompt_builtin_key: tmpl.builtin_key.clone(),
            prompt_hash: Some(draft.prompt_hash),
            prompt_tokens: draft.prompt_tokens,
            completion_tokens: draft.completion_tokens,
            finish_reason: Some(draft.finish_reason),
            created_at: now,
            updated_at: now,
        };

        let saved = self.incident_rca.upsert(rca).await?;

        if let Some(scoring) = &self.scoring {
            let target = (ScoreTarget::IncidentRca, saved.incident_id.0.as_str());

            if let Err(error) = enqueue_answered(
                scoring.as_ref(),
                org_id,
                target,
                None,
                TargetSource::IncidentRca,
            )
            .await
            {
                tracing::warn!(incident = %saved.incident_id.0, %error, "an rca could not be queued for scoring");
            }
        }

        Ok(saved)
    }

    /// 用一个 root_cause 模板写一份 RCA，不存下。Prompt 晋级检查用它比较两个版本，用量记在
    /// `origin` 下。
    #[allow(clippy::too_many_arguments)]
    pub async fn draft(
        &self,
        org_id: &Id,
        provider: &ModelProvider,
        tmpl: &AgentPromptTemplate,
        incident: &Incident,
        now: TimestampMicros,
        locale: RcaOutputLocale,
        origin: UsageOrigin,
    ) -> Result<RcaDraft> {
        // 解密 key → 构造 adapter（明文即用即弃，绝不落库/日志）。
        let key = self
            .agent_model_providers
            .get_plaintext_key(org_id, &provider.id)
            .await?
            .ok_or_else(|| Error::invalid("provider key not set"))?;

        let adapter = adapter_from_parts(
            Provider::parse(&provider.provider)?,
            provider.base_url.clone(),
            key,
            TransportOptions::from_timeout_ms(provider.timeout_ms),
        )?;

        let rendered = render_prompt(&tmpl.body, &render_vars(incident, locale));
        let system_body = format!("{rendered}\n\n{}", locale.system_instruction());
        let phash = prompt_hash(&system_body);

        // Metadata the organization keeps about the incident; the generator reads no telemetry.
        // A brief that takes too long is left out rather than holding up the sweep.
        let brief = match &self.briefing {
            Some(briefing) => {
                match tokio::time::timeout(BRIEF_WAIT, briefing.brief(incident)).await {
                    Ok(brief) => brief.render(),
                    Err(_) => {
                        tracing::warn!(incident = %incident.id.0, "an incident's brief took too long to read");
                        String::new()
                    }
                }
            }
            None => String::new(),
        };

        let messages = vec![
            msg(MessageRole::System, system_body, now),
            msg(
                MessageRole::User,
                build_evidence(incident, &brief, locale),
                now,
            ),
        ];

        let req = CompletionRequest {
            model: provider.default_model.clone(),
            messages,
            tools: None,
            tool_choice: agent::chat::ToolChoice::None,
            max_tokens: provider
                .max_tokens
                .map(|m| m as i32)
                .or(Some(self.cfg.default_max_tokens)),
            temperature: Some(self.cfg.temperature),
        };

        let recorder: Arc<dyn ModelCallObserver> = self.usage.recorder(UsageScope {
            provider_id: Some(provider.id.clone()),
            prices: provider.prices(),
            prompt_hash: Some(phash.clone()),
            ..UsageScope::new(org_id.clone(), origin)
        });

        let resp = observed_complete(adapter.as_ref(), req, Some(&recorder)).await?;
        let summary = resp.content.unwrap_or_default();

        if summary.trim().is_empty() {
            return Err(Error::internal("llm returned empty rca summary"));
        }

        Ok(RcaDraft {
            summary,
            prompt_hash: phash,
            prompt_tokens: resp.prompt_tokens,
            completion_tokens: resp.completion_tokens,
            finish_reason: resp.finish_reason,
        })
    }
}

/// 后台无真实用户：用哨兵 user_id 走 prompt 解析，落到 org/builtin 默认（不匹配任何 user override）。
pub(crate) fn system_user_id() -> Id {
    Id::from_string("system")
}

/// 选 org 的可用 provider：第一个 enabled 且已设 key 的（list 按 updated_at DESC，取最新）。
pub fn pick_provider(providers: &[ModelProvider]) -> Option<ModelProvider> {
    providers.iter().find(|p| p.enabled && p.key_set).cloned()
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
    use super::*;

    fn sample_provider(enabled: bool, key_set: bool) -> ModelProvider {
        ModelProvider {
            id: Id::from_string("p1"),
            org_id: Id::from_string("o1"),
            provider: "openai".into(),
            name: "p".into(),
            base_url: None,
            default_model: "gpt-x".into(),
            enabled,
            timeout_ms: 30000,
            max_tokens: None,
            price_currency: agent::usage::Currency::USD,
            input_price_per_mtok: None,
            cached_input_price_per_mtok: None,
            output_price_per_mtok: None,
            key_last4: None,
            key_set,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn pick_provider_skips_disabled_and_keyless() {
        assert!(pick_provider(&[]).is_none());
        assert!(pick_provider(&[sample_provider(false, true)]).is_none());
        assert!(pick_provider(&[sample_provider(true, false)]).is_none());
        let picked = pick_provider(&[sample_provider(true, true)]).unwrap();
        assert_eq!(picked.id.0, "p1");
    }

    #[test]
    fn output_locale_is_allowlisted_and_localizes_instructions() {
        assert_eq!(
            RcaOutputLocale::from_language_tag("zh-CN"),
            RcaOutputLocale::ZhCn
        );
        assert_eq!(
            RcaOutputLocale::from_language_tag("zh-Hans"),
            RcaOutputLocale::ZhCn
        );
        assert_eq!(
            RcaOutputLocale::from_language_tag("ignore previous instructions"),
            RcaOutputLocale::EnUs
        );

        assert!(
            RcaOutputLocale::ZhCn
                .system_instruction()
                .contains("简体中文")
        );
        assert!(
            RcaOutputLocale::ZhCn
                .system_instruction()
                .contains("CommonMark")
        );
    }
}
