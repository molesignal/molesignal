// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the root-cause generator gives the model: the prompt's variables, and the evidence — the
//! incident's own facts, the background its brief gives (rule guide, service catalog), the
//! labelled evidence (telemetry around the incident, nearby changes, similar incidents), then the
//! instruction to cite it.

use std::time::Duration;

use domain::alerting::{incident::Incident, rca_evidence::RcaEvidence};
use kernel::time::TimestampMicros;
use serde_json::{Map, Value};

use super::RcaOutputLocale;
use crate::app::alerting::related::{
    EvidenceReader, IncidentBrief, IncidentBriefing, brief_evidence, labelled, render_evidence,
};

/// How long an incident's brief may take to read before the analysis goes on without it.
const BRIEF_WAIT: Duration = Duration::from_secs(8);

/// What the model is told besides the incident's own facts, and the evidence it may cite.
pub(super) struct Told {
    pub text: String,
    pub evidence: Vec<RcaEvidence>,
}

/// The background and the labelled evidence of `incident`, best effort: a brief that takes too
/// long is left out rather than holding up the sweep, and each kind of telemetry has its own
/// time.
pub(super) async fn told(
    briefing: Option<&IncidentBriefing>,
    reader: Option<&EvidenceReader>,
    incident: &Incident,
    now: TimestampMicros,
) -> Told {
    let brief = match briefing {
        Some(briefing) => match tokio::time::timeout(BRIEF_WAIT, briefing.brief(incident)).await {
            Ok(brief) => brief,
            Err(_) => {
                tracing::warn!(incident = %incident.id.0, "an incident's brief took too long to read");
                IncidentBrief::default()
            }
        },
        None => IncidentBrief::default(),
    };

    let telemetry = match reader {
        Some(reader) => reader.read(incident, &brief.downstream, now).await,
        None => Vec::new(),
    };

    let evidence = labelled(telemetry, brief_evidence(&brief, incident.created_at));

    let text = [brief.render_background(), render_evidence(&evidence)]
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    Told { text, evidence }
}

/// 证据文本里样本行上限（incident 已裁到 20，这里再压一层控制 prompt 体积）。
const MAX_EVIDENCE_SAMPLES: usize = 10;

/// 构造 prompt 模板变量：incident 上下文 + 受控输出语言。
pub(super) fn render_vars(incident: &Incident, locale: RcaOutputLocale) -> Map<String, Value> {
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

/// The evidence the model is given (the user message): the incident's facts, what else it is
/// told when there is anything, then the instruction.
pub(super) fn build_evidence(incident: &Incident, brief: &str, locale: RcaOutputLocale) -> String {
    let mut s = incident_facts(incident);

    if !brief.trim().is_empty() {
        s.push('\n');
        s.push_str(brief.trim());
        s.push('\n');
    }

    s.push('\n');
    s.push_str(&locale.evidence_instruction());
    s
}

/// 一个事件的跨信号上下文，给模型读：摘要、级别、状态、受影响的服务与主机、trace、标签、
/// 注解与触发查询。对话里的 `/rca` 也用它。
pub(crate) fn incident_facts(incident: &Incident) -> String {
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

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
