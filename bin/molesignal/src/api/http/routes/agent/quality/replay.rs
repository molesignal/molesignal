// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 晋级检查里用一个 Prompt 版本重新回答一次。
//!
//! 对话回答：system 是要比的 `system` Prompt（填进组织名与原来那一轮的时间），加上「工具已经
//! 查过」的说明与回答要写成的样子；user 是原来的问题和那一轮工具查到的结果，工具不再调用。事件
//! RCA：用要比的 `root_cause` Prompt 和事件的事实重写一份。两个版本用同一个模型、同样的材料。

use serde_json::{Map, Value};

use super::{
    evaluating::{self, Model},
    facts::Facts,
};
use crate::{
    agent::{
        scoring::{ScoreTarget, evaluation::findings_text},
        usage::UsageOrigin,
    },
    api::{
        AppState,
        http::routes::agent::chat::answer_presentation_instruction,
        rca::{RcaGenerator, RcaOutputLocale},
    },
    infra::persistence::repositories::agent::prompts::{AgentPromptTemplate, render_prompt},
    shared::{Result, ids::Id, time::TimestampMicros},
};

const REPLAY_INSTRUCTION: &str = "The investigation for this question already ran, and the \
     results of its tools come with the question. Do not call tools. Answer from those results \
     only, and say what they do not show.";

/// 用 `template` 重新回答一次；返回回答与用掉的 token。
pub(super) async fn answer_with(
    state: &AppState,
    org_id: &Id,
    template: &AgentPromptTemplate,
    facts: &Facts,
    model: &Model,
) -> Result<(String, i64)> {
    match facts.subject.target_kind {
        ScoreTarget::ChatAnswer => {
            let system = system_text(state, org_id, template, facts).await;
            let findings = findings_text(&facts.steps);

            let user = if findings.is_empty() {
                facts.question.clone()
            } else {
                format!(
                    "{}\n\n## What the tools found\n\n{findings}",
                    facts.question
                )
            };

            evaluating::answer(state, org_id, model, system, user).await
        }
        ScoreTarget::IncidentRca => {
            let incident = state
                .alerting
                .service
                .incidents
                .get(&Id::from_string(facts.subject.target_id.as_str()))
                .await?;

            let draft = RcaGenerator::from_state(state)
                .draft(
                    org_id,
                    &model.provider,
                    template,
                    &incident,
                    TimestampMicros::now(),
                    RcaOutputLocale::EnUs,
                    UsageOrigin::Evaluation,
                )
                .await?;

            Ok((
                draft.summary,
                i64::from(draft.prompt_tokens) + i64::from(draft.completion_tokens),
            ))
        }
    }
}

async fn system_text(
    state: &AppState,
    org_id: &Id,
    template: &AgentPromptTemplate,
    facts: &Facts,
) -> String {
    let org_name = state
        .iam
        .service
        .orgs
        .get(org_id)
        .await
        .map(|org| org.name)
        .unwrap_or_else(|_| org_id.0.clone());

    let mut vars = Map::new();
    vars.insert("org_name".into(), Value::String(org_name));
    vars.insert(
        "current_time".into(),
        Value::String(facts.subject.target_at.to_datetime().to_rfc3339()),
    );

    format!(
        "{}\n\n{REPLAY_INSTRUCTION}\n\n{}",
        render_prompt(&template.body, &vars),
        answer_presentation_instruction(false)
    )
}
