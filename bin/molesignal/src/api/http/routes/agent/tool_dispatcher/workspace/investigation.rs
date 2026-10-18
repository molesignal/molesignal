// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `update_investigation`：把调查过程记到这个会话的调查上（步骤、证据、假设、结论），调查页
//! 照着显示。只记这个会话自己的调查。

use serde::Deserialize;
use serde_json::{Value, json};

use super::{WorkspaceCall, parse};
use crate::{
    agent::{
        model::{
            ConfidenceLevel, FactStatus, HypothesisStatus, InvestigationEvidence,
            InvestigationHypothesis, InvestigationStatus, InvestigationStep, StepStatus,
        },
        tools::ToolResult,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Record {
    Step,
    Evidence,
    Hypothesis,
    Conclusion,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    record: Record,
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    confidence: Option<ConfidenceLevel>,
    #[serde(default)]
    fact_status: Option<FactStatus>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    query: Option<String>,
}

fn status<T: serde::de::DeserializeOwned>(value: Option<&str>, default: T) -> T {
    value
        .and_then(|value| serde_json::from_value(Value::String(value.into())).ok())
        .unwrap_or(default)
}

pub(super) async fn execute(call: &WorkspaceCall<'_>, arguments: Value) -> Result<ToolResult> {
    let Some(investigation_id) = call.ctx.investigation_id().map(Id::from_string) else {
        return Ok(ToolResult::error(
            "this chat has no investigation to record; it is for chats started from an incident",
        ));
    };

    let args: Args = parse(arguments)?;
    let org_id = call.ctx.iam_context().org_id.clone();
    let repository = &call.state.agent.repository;
    let detail = repository
        .get_investigation(&org_id, &investigation_id)
        .await?;
    let now = TimestampMicros::now();

    let recorded = match args.record {
        Record::Step => {
            let step = repository
                .append_step(InvestigationStep {
                    id: Id::new(),
                    investigation_id: investigation_id.clone(),
                    org_id,
                    position: detail.steps.len() as i32 + 1,
                    title: args.title,
                    status: status(args.status.as_deref(), StepStatus::Succeeded),
                    tool_name: args.tool,
                    input: json!({ "query": args.query }),
                    output_summary: Some(args.summary),
                    conclusion_impact: None,
                    error: None,
                    started_at: Some(now),
                    ended_at: Some(now),
                    created_at: now,
                })
                .await?;

            json!({ "step": step.id.0, "position": step.position })
        }
        Record::Evidence => {
            let evidence = repository
                .append_evidence(InvestigationEvidence {
                    id: Id::new(),
                    investigation_id: investigation_id.clone(),
                    step_id: detail.steps.last().map(|step| step.id.clone()),
                    org_id,
                    kind: args.tool.clone().unwrap_or_else(|| "observation".into()),
                    label: args.title,
                    fact_status: args.fact_status.unwrap_or(FactStatus::Verified),
                    source_ref: json!({ "tool": args.tool, "chat_id": call.workspace.chat_id.0 }),
                    query: args.query,
                    parameters: Value::Null,
                    summary: args.summary,
                    created_at: now,
                })
                .await?;

            json!({ "evidence": evidence.id.0 })
        }
        Record::Hypothesis => {
            // 同一句话的假设是同一条：更新它的状态与把握。
            let existing = detail
                .hypotheses
                .iter()
                .find(|hypothesis| hypothesis.statement.eq_ignore_ascii_case(args.title.trim()));

            let hypothesis = repository
                .upsert_hypothesis(InvestigationHypothesis {
                    id: existing.map_or_else(Id::new, |hypothesis| hypothesis.id.clone()),
                    investigation_id: investigation_id.clone(),
                    org_id,
                    statement: args.title.trim().to_string(),
                    confidence: args.confidence.unwrap_or(ConfidenceLevel::Medium),
                    status: status(args.status.as_deref(), HypothesisStatus::Proposed),
                    evidence_ids: detail.evidence.iter().map(|item| item.id.clone()).collect(),
                    created_at: existing.map_or(now, |hypothesis| hypothesis.created_at),
                    updated_at: now,
                })
                .await?;

            json!({ "hypothesis": hypothesis.id.0, "status": hypothesis.status })
        }
        Record::Conclusion => {
            let mut investigation = detail.investigation;
            investigation.summary = Some(args.summary);
            investigation.confidence = args.confidence.or(investigation.confidence);
            investigation.status = InvestigationStatus::Completed;
            investigation.completed_at = Some(now);
            investigation.updated_at = now;
            repository.update_investigation(investigation).await?;

            json!({ "concluded": true })
        }
    };

    Ok(ToolResult::json(
        json!({ "investigation": investigation_id.0, "recorded": recorded }),
    ))
}
