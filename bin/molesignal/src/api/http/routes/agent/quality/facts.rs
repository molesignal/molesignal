// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 打分要的事实：对象出自哪里、那一轮用了什么、规则打分的输入，以及交给评审的材料。
//!
//! 对话回答从会话、消息、Run、自动化的运行记录与评价里读；事件 RCA 从 RCA、事件与评价里读。
//! 对象没了（会话删了、消息找不到）时为 `None`。

use serde_json::Value;

use crate::{
    agent::{
        answer::{Headings, StructuredAnswer, conclusion},
        automations::AutomationRunStatus,
        commands::{CommandRecord, CommandSource},
        feedback::{FeedbackRating, FeedbackReason, FeedbackTarget},
        runs::{RunStatus, TranscriptStep},
        scoring::{
            Attribution, DimensionScore, ScoreSubject, ScoreTarget, TargetSource,
            evaluation::Material,
            rules::{self, AnswerFacts},
        },
    },
    api::{AppState, rca::incident_facts},
    app::scoring::ParserSyntax,
    infra::persistence::repositories::agent::chats::Chat,
    shared::{Error, Result, ids::Id},
};

/// 列表里问题与回答的开头最多这么多字符。
const EXCERPT_CHARS: usize = 300;

/// 一个对象打分要的全部事实。
pub(super) struct Facts {
    pub(super) subject: ScoreSubject,
    /// 交给评审的问题：对话是问题（斜杠命令是展开后的），RCA 是事件的事实。
    pub(super) question: String,
    pub(super) steps: Vec<TranscriptStep>,
    /// 交给评审的回答：结构化的排成 Markdown。
    pub(super) answer: String,
    pub(super) kind: Kind,
    pub(super) feedback: Option<(FeedbackRating, Vec<FeedbackReason>)>,
}

pub(super) enum Kind {
    Answer {
        content: String,
        /// 只生成查询的一轮不要求 JSON 形状。
        structured_expected: bool,
        status: RunStatus,
        /// 自动化里途中被挡住了。
        blocked: bool,
    },
    Rca {
        summary: String,
    },
}

impl Facts {
    pub(super) fn rules(&self) -> Vec<DimensionScore> {
        let feedback = self
            .feedback
            .as_ref()
            .map(|(rating, reasons)| (*rating, reasons.as_slice()));

        match &self.kind {
            Kind::Answer {
                content,
                structured_expected,
                status,
                blocked,
            } => rules::score_answer(
                &AnswerFacts {
                    content,
                    steps: &self.steps,
                    structured_expected: *structured_expected,
                    status: *status,
                    blocked: *blocked,
                    feedback,
                },
                &ParserSyntax,
            ),
            Kind::Rca { summary } => rules::score_rca(summary, feedback),
        }
    }

    pub(super) fn material<'a>(&'a self, note: Option<&'a str>) -> Material<'a> {
        Material {
            kind: self.subject.target_kind,
            question: &self.question,
            findings: &self.steps,
            answer: &self.answer,
            note,
        }
    }

    pub(super) fn thumbs_down(&self) -> bool {
        matches!(self.feedback, Some((FeedbackRating::Down, _)))
    }
}

/// 交给评审的回答里各部分的标题：材料都用英文写。
fn evaluator_headings() -> Headings {
    Headings {
        likely_causes: "Likely causes".into(),
        evidence: "Evidence".into(),
        next_steps: "Next steps".into(),
        limitations: "Limitations".into(),
        related_links: "Related actions".into(),
        confidence_high: "Confidence: high".into(),
        confidence_medium: "Confidence: medium".into(),
        confidence_low: "Confidence: low".into(),
    }
}

/// 交给评审的回答：结构化的排成 Markdown，其余原样。
pub(super) fn readable(content: &str) -> String {
    match StructuredAnswer::parse(content) {
        Some(answer) => answer.to_markdown(&evaluator_headings()),
        None => content.trim().to_string(),
    }
}

/// 会话里的回答出自哪里。个人作用域的自动化，它的运行也是个人会话：只有本人读得到。
pub(super) fn chat_source(chat: &Chat) -> TargetSource {
    match (chat.scope.as_str(), chat.automation_id.is_some()) {
        ("personal", _) => TargetSource::PersonalChat,
        (_, true) => TargetSource::Automation,
        ("team", false) => TargetSource::TeamChat,
        _ => TargetSource::PersonalChat,
    }
}

fn excerpt(text: &str) -> String {
    let text = text.trim();

    if text.chars().count() <= EXCERPT_CHARS {
        return text.to_string();
    }

    let mut cut = text.chars().take(EXCERPT_CHARS).collect::<String>();
    cut.push('…');
    cut
}

pub(super) async fn gather(
    state: &AppState,
    org_id: &Id,
    kind: ScoreTarget,
    target_id: &str,
    chat_id: Option<&Id>,
) -> Result<Option<Facts>> {
    match kind {
        ScoreTarget::ChatAnswer => match chat_id {
            Some(chat_id) => chat_answer(state, org_id, chat_id, target_id).await,
            None => Ok(None),
        },
        ScoreTarget::IncidentRca => incident_rca(state, org_id, target_id).await,
    }
}

async fn chat_answer(
    state: &AppState,
    org_id: &Id,
    chat_id: &Id,
    message_id: &str,
) -> Result<Option<Facts>> {
    let chat = match state.agent.chats.get_chat(org_id, chat_id).await {
        Ok(chat) => chat,
        Err(Error::NotFound(_)) => return Ok(None),
        Err(error) => return Err(error),
    };

    let messages = state.agent.chats.list_messages(&chat.id).await?;

    let Some(position) = messages
        .iter()
        .position(|message| message.id.0 == message_id && message.role == "assistant")
    else {
        return Ok(None);
    };

    let message = &messages[position];
    let asked = messages[..position]
        .iter()
        .rev()
        .find(|message| message.role == "user");
    let command = asked
        .and_then(|asked| asked.command_json.clone())
        .and_then(|command| serde_json::from_value::<CommandRecord>(command).ok());

    let question = command
        .as_ref()
        .map(|command| command.model_text.clone())
        .filter(|text| !text.trim().is_empty())
        .or_else(|| asked.map(|asked| asked.content.clone()))
        .unwrap_or_default();

    let asker = asked
        .and_then(|asked| asked.user_id.clone())
        .unwrap_or_else(|| chat.user_id.clone());

    let steps = message
        .tool_transcript
        .clone()
        .and_then(|steps| serde_json::from_value::<Vec<TranscriptStep>>(steps).ok())
        .unwrap_or_default();

    let run = match &message.run_id {
        Some(run_id) => state.agent.runs.get(org_id, run_id).await.ok(),
        None => None,
    };

    let analysis_mode = run
        .as_ref()
        .and_then(|run| run.request.pointer("/message/analysisMode"))
        .and_then(Value::as_str);

    let blocked = match (&chat.automation_id, &message.run_id) {
        (Some(_), Some(run_id)) => state
            .agent
            .automations
            .run_for_agent_run(run_id)
            .await?
            .is_some_and(|record| record.status == AutomationRunStatus::Blocked),
        _ => false,
    };

    let source = chat_source(&chat);
    let skill = command
        .as_ref()
        .filter(|command| command.source == CommandSource::Skill);

    // 个人对话只有本人读得到：列表里不记它的问题与回答。
    let (question_excerpt, answer_excerpt) = if source.is_private() {
        (String::new(), String::new())
    } else {
        (excerpt(&question), excerpt(&conclusion(&message.content)))
    };

    let subject = ScoreSubject {
        org_id: org_id.clone(),
        target_kind: ScoreTarget::ChatAnswer,
        target_id: message_id.to_string(),
        source,
        question: question_excerpt,
        answer: answer_excerpt,
        attribution: Attribution {
            chat_id: Some(chat.id.clone()),
            team_id: chat.team_id.as_deref().map(Id::from_string),
            automation_id: chat.automation_id.as_deref().map(Id::from_string),
            user_id: Some(asker),
            prompt_template_id: message.prompt_template_id.clone(),
            prompt_version: message.prompt_version,
            prompt_key: message.prompt_builtin_key.clone(),
            provider_id: chat.provider_id.as_deref().map(Id::from_string),
            model: Some(chat.model.clone()).filter(|model| !model.is_empty()),
            skill: skill.map(|command| command.name.trim_start_matches('/').to_string()),
            skill_version: skill.and_then(|command| command.skill_version.clone()),
        },
        target_at: message.created_at,
    };

    Ok(Some(Facts {
        subject,
        question,
        steps,
        answer: readable(&message.content),
        kind: Kind::Answer {
            content: message.content.clone(),
            structured_expected: analysis_mode != Some("query_generation"),
            status: run.as_ref().map_or(RunStatus::Succeeded, |run| run.status),
            blocked,
        },
        feedback: feedback_of(state, org_id, FeedbackTarget::ChatMessage, message_id).await?,
    }))
}

async fn incident_rca(state: &AppState, org_id: &Id, incident_id: &str) -> Result<Option<Facts>> {
    let incident_id = Id::from_string(incident_id);

    let Some(rca) = state
        .agent
        .incident_rca
        .get(&incident_id)
        .await?
        .filter(|rca| rca.org_id == *org_id)
    else {
        return Ok(None);
    };

    let incident = match state.alerting.service.incidents.get(&incident_id).await {
        Ok(incident) if incident.org_id == *org_id => incident,
        Ok(_) | Err(Error::NotFound(_)) => return Ok(None),
        Err(error) => return Err(error),
    };

    let subject = ScoreSubject {
        org_id: org_id.clone(),
        target_kind: ScoreTarget::IncidentRca,
        target_id: incident_id.0.clone(),
        source: TargetSource::IncidentRca,
        question: excerpt(&incident.summary),
        answer: excerpt(&rca.summary),
        attribution: Attribution {
            prompt_key: rca.prompt_builtin_key.clone(),
            model: rca.model.clone(),
            ..Attribution::default()
        },
        target_at: rca.updated_at,
    };

    Ok(Some(Facts {
        subject,
        question: incident_facts(&incident),
        steps: Vec::new(),
        answer: rca.summary.clone(),
        kind: Kind::Rca {
            summary: rca.summary.clone(),
        },
        feedback: feedback_of(state, org_id, FeedbackTarget::IncidentRca, &incident_id.0).await?,
    }))
}

/// 对象收到的评价：有人评为没帮助就算没帮助（原因合在一起），否则有人说有帮助就算有帮助。
async fn feedback_of(
    state: &AppState,
    org_id: &Id,
    target: FeedbackTarget,
    target_id: &str,
) -> Result<Option<(FeedbackRating, Vec<FeedbackReason>)>> {
    let given = state
        .agent
        .feedback
        .list_for_target(org_id, target, target_id)
        .await?;

    let mut reasons = given
        .iter()
        .filter(|feedback| feedback.rating == FeedbackRating::Down)
        .flat_map(|feedback| feedback.reasons.iter().copied())
        .collect::<Vec<_>>();

    reasons.sort_unstable();
    reasons.dedup();

    if given
        .iter()
        .any(|feedback| feedback.rating == FeedbackRating::Down)
    {
        return Ok(Some((FeedbackRating::Down, reasons)));
    }

    Ok((!given.is_empty()).then(|| (FeedbackRating::Up, Vec::new())))
}
