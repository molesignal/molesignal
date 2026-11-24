// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The evidence as a model reads it: one labelled line a piece, with its
//! figures written out in their units.

use domain::alerting::rca_evidence::{EvidenceKind, RcaEvidence};

use super::super::brief::minute;

/// The evidence as text for a model, one line a piece; empty when there is none.
pub fn render_evidence(evidence: &[RcaEvidence]) -> String {
    if evidence.is_empty() {
        return String::new();
    }

    let lines = evidence.iter().map(line).collect::<Vec<_>>().join("\n");
    format!("Evidence, each piece labelled for citing:\n{lines}")
}

fn line(piece: &RcaEvidence) -> String {
    let mut parts = vec![format!("[{}] {}", piece.label, kind_words(piece.kind))];

    if let Some(service) = &piece.service {
        parts.push(service.clone());
    }

    if let Some(detail) = piece
        .detail
        .as_deref()
        .filter(|_| piece.kind == EvidenceKind::Insight)
    {
        parts.push(detail.replace(['_', '.'], " "));
    }

    if let Some(subject) = &piece.subject {
        parts.push(subject.clone());
    }

    // The values a business key or a field was found with.
    if let Some(detail) = piece.detail.as_deref().filter(|_| {
        matches!(
            piece.kind,
            EvidenceKind::BusinessImpact | EvidenceKind::AttributeSkew
        )
    }) {
        parts.push(detail.to_string());
    }

    if let Some(figures) = figures(piece) {
        parts.push(figures);
    }

    if let Some(trace) = piece
        .link
        .as_ref()
        .and_then(|link| link.trace_id.as_deref())
    {
        parts.push(format!("trace {trace}"));
    }

    if let Some(at) = piece.at {
        parts.push(format!(
            "{} {}",
            if piece.kind == EvidenceKind::Insight {
                "began"
            } else {
                "at"
            },
            minute(at)
        ));
    }

    parts.join(" · ")
}

fn kind_words(kind: EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::Insight => "APM insight",
        EvidenceKind::NewErrorGroup => "New error group",
        EvidenceKind::SampleTrace => "Sample trace",
        EvidenceKind::ErrorLogs => "Error-level log lines",
        EvidenceKind::NewLogPattern => "New error log pattern",
        EvidenceKind::BusinessImpact => "Business impact",
        EvidenceKind::AttributeSkew => "Field where failed requests differ",
        EvidenceKind::Change => "Change",
        EvidenceKind::SimilarIncident => "Similar past incident",
        EvidenceKind::Investigation => "Investigation finding",
    }
}

fn figures(piece: &RcaEvidence) -> Option<String> {
    let figure = |name: &str| piece.figures.get(name).copied();

    match piece.kind {
        EvidenceKind::Insight => {
            let metric = piece
                .detail
                .as_deref()
                .and_then(|detail| detail.split('.').nth(1));
            let value = |value: f64| in_unit(metric, value);
            let mut told = vec![format!("now {}", value(figure("value")?))];

            if let Some(previous) = figure("previous") {
                told.push(format!("the stretch before {}", value(previous)));
            }

            if let Some(last_week) = figure("lastWeek") {
                told.push(format!("a week before {}", value(last_week)));
            }

            if let Some(calls) = figure("calls") {
                told.push(format!("{calls:.0} calls"));
            }

            if let Some(p95) = figure("p95Time") {
                told.push(format!("P95 {}", milliseconds(p95)));
            }

            Some(told.join(", "))
        }
        EvidenceKind::NewErrorGroup => Some(format!("{:.0} occurrences", figure("occurrences")?)),
        EvidenceKind::SampleTrace => Some(milliseconds(figure("duration")?)),
        EvidenceKind::ErrorLogs => Some(format!(
            "{:.0} in the window, {:.0} in the same window a week before",
            figure("now")?,
            figure("lastWeek").unwrap_or_default()
        )),
        EvidenceKind::NewLogPattern => Some(format!("{:.0} lines in the window", figure("lines")?)),
        EvidenceKind::BusinessImpact => {
            let mut told = format!(
                "{:.0} of {:.0} requests failed across {:.0} values",
                figure("errors")?,
                figure("requests").unwrap_or_default(),
                figure("values").unwrap_or_default()
            );

            if let Some(important) = figure("important").filter(|important| *important > 0.0) {
                told.push_str(&format!(", {important:.0} of them marked important"));
            }

            Some(told)
        }
        EvidenceKind::AttributeSkew => Some(format!(
            "{:.1}% of failed requests against {:.1}% of the others",
            figure("failedShare")? * 100.0,
            figure("otherShare").unwrap_or_default() * 100.0
        )),
        EvidenceKind::Change | EvidenceKind::SimilarIncident | EvidenceKind::Investigation => None,
    }
}

/// An insight's figure in the unit of what moved: a rate as a percentage, a P95 in
/// milliseconds, throughput in requests a second, error log lines a minute.
pub(super) fn in_unit(metric: Option<&str>, value: f64) -> String {
    match metric {
        Some("error_rate") => format!("{:.2}%", value * 100.0),
        Some("p95") => milliseconds(value),
        Some("throughput") => format!("{value:.2} req/s"),
        Some("log_errors") => format!("{value:.1} error log lines/min"),
        _ => format!("{value:.2}"),
    }
}

/// Microseconds as milliseconds.
fn milliseconds(micros: f64) -> String {
    format!("{:.1} ms", micros / 1_000.0)
}
