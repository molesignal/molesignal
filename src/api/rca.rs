// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Incident 根因分析（RCA）生成器：聚合 incident 跨信号上下文，调 LLM 产出根因摘要。
//!
//! 由两处复用，逻辑单点不漂移：
//! - 后台 `rca_sweeper`（bootstrap，依赖 api）周期对活跃 incident 自动生成；
//! - `POST /alerts/incidents/{id}/rca`（HTTP）按需手动触发。
//!
//! provider key 解密即用即弃，绝不落库/日志；provider/model/token/prompt_hash 一并写
//! `incident_rca` 可审计。RCA 是 intelligence 能力，是否启用由调用方按 license feature 决定。

use std::sync::Arc;

use serde_json::{Map, Value};

use crate::{
    api::AppState,
    domain::alerting::{
        incident::{Incident, IncidentRca},
        repositories::IncidentRcaRepository,
    },
    infra::persistence::repositories::intelligence::{
        model_providers::{ModelProvider, ModelProviderRepository},
        prompts::{AgentPromptRepository, prompt_hash, render_prompt},
    },
    intelligence::chat::{
        ChatMessage, CompletionRequest, MessageRole, Provider, adapter_from_parts,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// root_cause prompt 的 purpose（与迁移 seed 的 builtin 行一致）。
const RCA_PURPOSE: &str = "root_cause";
/// 证据文本里样本行上限（incident 已裁到 20，这里再压一层控制 prompt 体积）。
const MAX_EVIDENCE_SAMPLES: usize = 10;

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
    intelligence_model_providers: Arc<dyn ModelProviderRepository>,
    intelligence_prompts: Arc<dyn AgentPromptRepository>,
    incident_rca: Arc<dyn IncidentRcaRepository>,
    cfg: RcaGenConfig,
}

impl RcaGenerator {
    pub fn new(
        intelligence_model_providers: Arc<dyn ModelProviderRepository>,
        intelligence_prompts: Arc<dyn AgentPromptRepository>,
        incident_rca: Arc<dyn IncidentRcaRepository>,
        cfg: RcaGenConfig,
    ) -> Self {
        Self {
            intelligence_model_providers,
            intelligence_prompts,
            incident_rca,
            cfg,
        }
    }

    /// 从 [`AppState`] 装配（HTTP 按需触发用）。
    pub fn from_state(state: &AppState) -> Self {
        Self::new(
            state.intelligence.model_providers.clone(),
            state.intelligence.prompts.clone(),
            state.intelligence.incident_rca.clone(),
            RcaGenConfig::default(),
        )
    }

    /// 列出并选 org 可用 provider（enabled + key_set，取最近更新的一个）。
    pub async fn pick_provider_for(&self, org_id: &Id) -> Result<Option<ModelProvider>> {
        let providers = self.intelligence_model_providers.list(org_id).await?;
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
        // 解密 key → 构造 adapter（明文即用即弃，绝不落库/日志）。
        let key = self
            .intelligence_model_providers
            .get_plaintext_key(org_id, &provider.id)
            .await?
            .ok_or_else(|| Error::invalid("provider key not set"))?;
        let adapter = adapter_from_parts(
            Provider::parse(&provider.provider)?,
            provider.base_url.clone(),
            key,
        )?;

        // 解析 root_cause prompt（user→org→builtin）作系统消息；哨兵 user 落 org/builtin 默认。
        let tmpl = self
            .intelligence_prompts
            .resolve(org_id, &system_user_id(), RCA_PURPOSE)
            .await?;
        let rendered = render_prompt(&tmpl.body, &render_vars(incident, locale));
        let system_body = format!("{rendered}\n\n{}", locale.system_instruction());
        let phash = prompt_hash(&system_body);

        let messages = vec![
            msg(MessageRole::System, system_body, now),
            msg(MessageRole::User, build_evidence(incident, locale), now),
        ];
        let req = CompletionRequest {
            model: provider.default_model.clone(),
            messages,
            tools: None,
            tool_choice: crate::intelligence::chat::ToolChoice::None,
            max_tokens: provider
                .max_tokens
                .map(|m| m as i32)
                .or(Some(self.cfg.default_max_tokens)),
            temperature: Some(self.cfg.temperature),
        };
        let resp = adapter.complete(req).await?;
        let summary = resp.content.unwrap_or_default();
        if summary.trim().is_empty() {
            return Err(Error::internal("llm returned empty rca summary"));
        }
        let rca = IncidentRca {
            incident_id: incident.id.clone(),
            org_id: org_id.clone(),
            summary,
            provider: Some(provider.provider.clone()),
            model: Some(provider.default_model.clone()),
            prompt_builtin_key: tmpl.builtin_key.clone(),
            prompt_hash: Some(phash),
            prompt_tokens: resp.prompt_tokens,
            completion_tokens: resp.completion_tokens,
            finish_reason: Some(resp.finish_reason),
            created_at: now,
            updated_at: now,
        };
        self.incident_rca.upsert(rca).await
    }
}

/// 后台无真实用户：用哨兵 user_id 走 prompt 解析，落到 org/builtin 默认（不匹配任何 user override）。
fn system_user_id() -> Id {
    Id::from_string("system")
}

/// 选 org 的可用 provider：第一个 enabled 且已设 key 的（list 按 updated_at DESC，取最新）。
pub fn pick_provider(providers: &[ModelProvider]) -> Option<ModelProvider> {
    providers.iter().find(|p| p.enabled && p.key_set).cloned()
}

/// 构造 prompt 模板变量：incident 上下文 + 受控输出语言。
fn render_vars(incident: &Incident, locale: RcaOutputLocale) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert(
        "time_range".to_string(),
        Value::String(time_range_str(incident)),
    );
    m.insert("streams".to_string(), Value::String(streams_str(incident)));
    m.insert(
        "locale".to_string(),
        Value::String(locale.language_tag().to_string()),
    );
    m.insert(
        "output_language".to_string(),
        Value::String(locale.output_language().to_string()),
    );
    m
}

fn time_range_str(incident: &Incident) -> String {
    let end = incident.resolved_at.unwrap_or(incident.created_at);
    format!("{} .. {}", iso(incident.created_at), iso(end))
}

fn streams_str(incident: &Incident) -> String {
    if incident.affected_services.is_empty() {
        "n/a".to_string()
    } else {
        incident.affected_services.join(", ")
    }
}

/// 把 incident 的跨信号上下文渲染成给 LLM 的证据文本（user 消息）。
fn build_evidence(incident: &Incident, locale: RcaOutputLocale) -> String {
    let mut s = String::new();
    s.push_str(&format!("Incident: {}\n", incident.summary));
    s.push_str(&format!("Severity: {:?}\n", incident.severity));
    s.push_str(&format!("Status: {:?}\n", incident.status));
    s.push_str(&format!("Created: {}\n", iso(incident.created_at)));
    s.push_str(&format!(
        "Affected services: {}\n",
        join_or_na(&incident.affected_services)
    ));
    s.push_str(&format!("Hosts: {}\n", join_or_na(&incident.host_ids)));
    s.push_str(&format!("Trace IDs: {}\n", join_or_na(&incident.trace_ids)));
    if !incident.labels.is_empty() {
        let labels = incident
            .labels
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", ");
        s.push_str(&format!("Labels: {labels}\n"));
    }
    if !incident.annotations.is_empty() {
        let ann = incident
            .annotations
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", ");
        s.push_str(&format!("Annotations: {ann}\n"));
    }
    if let Some(q) = &incident.triggering_query {
        s.push_str(&format!(
            "Triggering query ({:?}): {}\n",
            q.language, q.statement
        ));
        if !q.sample_values.is_empty() {
            s.push_str("Sample values:\n");
            for sample in q.sample_values.iter().take(MAX_EVIDENCE_SAMPLES) {
                s.push_str(&format!("- ts={} value={}\n", iso(sample.ts), sample.value));
            }
        }
    }
    s.push('\n');
    s.push_str(locale.evidence_instruction());
    s
}

fn join_or_na(v: &[String]) -> String {
    if v.is_empty() {
        "n/a".to_string()
    } else {
        v.join(", ")
    }
}

/// micros → RFC3339（UTC）；越界回退到原始 micros 文本。
fn iso(ts: TimestampMicros) -> String {
    let secs = ts.0.div_euclid(1_000_000);
    let nanos = (ts.0.rem_euclid(1_000_000) * 1000) as u32;
    chrono::DateTime::from_timestamp(secs, nanos)
        .map(|d| d.to_rfc3339())
        .unwrap_or_else(|| ts.0.to_string())
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
    use std::collections::BTreeMap;

    use super::*;
    use crate::domain::{
        alerting::incident::{IncidentStatus, Severity, TriggeringQuery, TriggeringSample},
        query::QueryLanguage,
    };

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
            key_last4: None,
            key_set,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn incident() -> Incident {
        Incident {
            id: Id::from_string("i1"),
            org_id: Id::from_string("o1"),
            rule_id: Id::from_string("r1"),
            escalation_policy_id: Id::from_string("e1"),
            status: IncidentStatus::Open,
            severity: Severity::Critical,
            summary: "High error rate".into(),
            fingerprint: "fp".into(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: TimestampMicros(0),
            assignees: vec![],
            labels: BTreeMap::from([("env".to_string(), "prod".to_string())]),
            annotations: BTreeMap::new(),
            trace_ids: vec!["t-1".into()],
            host_ids: vec!["h-1".into()],
            affected_services: vec!["api".into(), "web".into()],
            triggering_query: Some(TriggeringQuery {
                language: QueryLanguage::Sql,
                statement: "SELECT count(*) FROM logs".into(),
                sample_values: vec![TriggeringSample {
                    ts: TimestampMicros(1_700_000_000_000_000),
                    value: 42.0,
                    labels: BTreeMap::new(),
                }],
            }),
            created_at: TimestampMicros(1_700_000_000_000_000),
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            resolved_by: None,
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
    fn render_vars_carries_time_range_and_services() {
        let vars = render_vars(&incident(), RcaOutputLocale::ZhCn);
        assert_eq!(
            vars.get("streams").and_then(|v| v.as_str()),
            Some("api, web")
        );
        assert_eq!(vars.get("locale").and_then(|v| v.as_str()), Some("zh-cn"));
        assert_eq!(
            vars.get("output_language").and_then(|v| v.as_str()),
            Some("Simplified Chinese")
        );
        let tr = vars.get("time_range").and_then(|v| v.as_str()).unwrap();
        assert!(tr.contains("2023-11-14"), "time_range should be ISO: {tr}");
    }

    #[test]
    fn build_evidence_includes_context_and_instruction() {
        let e = build_evidence(&incident(), RcaOutputLocale::EnUs);
        assert!(e.contains("High error rate"));
        assert!(e.contains("Affected services: api, web"));
        assert!(e.contains("Trace IDs: t-1"));
        assert!(e.contains("SELECT count(*) FROM logs"));
        assert!(e.contains("root-cause analysis"));
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

        let evidence = build_evidence(&incident(), RcaOutputLocale::ZhCn);
        assert!(evidence.contains("仅依据以上证据"));
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

    #[test]
    fn streams_falls_back_to_na() {
        let mut i = incident();
        i.affected_services.clear();
        assert_eq!(streams_str(&i), "n/a");
    }
}
