// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The evidence a root-cause analysis rests on. Each piece has a label the
//! analysis cites it by (`E1`, `E2`, …), what it is, the service it is about,
//! and the figures behind it. Figures read from telemetry are kept only for
//! readers who may read that telemetry: everyone who can see the incident
//! sees what each piece is about, but the numbers stay with those allowed to
//! read the data they came from.

use std::collections::BTreeMap;

use kernel::time::TimestampMicros;
use serde::{Deserialize, Serialize};

/// The most pieces of evidence an analysis keeps.
pub const MAX_RCA_EVIDENCE: usize = 30;

/// What a piece of evidence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    /// An APM insight: a figure that moved, a version that regressed, where time goes.
    Insight,
    /// An error group first seen around when the incident began.
    NewErrorGroup,
    /// A trace APM kept as an example of a service's requests.
    SampleTrace,
    /// Error-level log lines of a service against the same stretch a week before.
    ErrorLogs,
    /// A message pattern of error-level logs that the stretch just before did not have.
    NewLogPattern,
    /// The values of a business key (a tenant, a plan) whose requests failed most.
    BusinessImpact,
    /// A field whose values the failed requests have apart from the others.
    AttributeSkew,
    /// A change reported, or an edit of the alert rule.
    Change,
    /// A past incident like this one.
    SimilarIncident,
    /// What an investigation in Mole Agent cited.
    Investigation,
}

/// Where a piece of evidence was read from, which decides who may read its figures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Apm,
    Traces,
    Logs,
    /// What the organization keeps about incidents, rules and changes: whoever sees the
    /// incident may read it.
    Incident,
}

impl EvidenceSource {
    /// Any one of these lets a reader see the figures; none are asked of [`Self::Incident`].
    pub fn permissions(self) -> &'static [&'static str] {
        match self {
            Self::Apm => &["apm.services.read", "sys.telemetry.read"],
            Self::Traces | Self::Logs => &["streams.query", "sys.telemetry.read"],
            Self::Incident => &[],
        }
    }
}

/// Where a page can show a piece of evidence; each part is said only when known.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLink {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_id: Option<String>,
    /// An in-product address starting with `/`, as an investigation cited it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<TimestampMicros>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<TimestampMicros>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcaEvidence {
    /// What the analysis cites it by: `E1`, `E2`, ….
    pub label: String,
    pub kind: EvidenceKind,
    pub source: EvidenceSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// What it is about, without figures: an error type and message, an endpoint, a change, a
    /// past incident's summary, what an investigation said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// A finer kind: for an insight, its kind and the figure that moved (`anomaly.p95`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// When it happened or began.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<TimestampMicros>,
    /// The figures behind it, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub figures: BTreeMap<String, f64>,
    /// The figures were left out for a reader who may not read where they came from.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub withheld: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<EvidenceLink>,
}

impl RcaEvidence {
    pub fn new(kind: EvidenceKind, source: EvidenceSource) -> Self {
        Self {
            label: String::new(),
            kind,
            source,
            service: None,
            subject: None,
            detail: None,
            at: None,
            figures: BTreeMap::new(),
            withheld: false,
            link: None,
        }
    }
}

/// Labels the pieces `E1`, `E2`, … in their order, keeping the first [`MAX_RCA_EVIDENCE`].
pub fn label_evidence(mut evidence: Vec<RcaEvidence>) -> Vec<RcaEvidence> {
    evidence.truncate(MAX_RCA_EVIDENCE);

    for (index, piece) in evidence.iter_mut().enumerate() {
        piece.label = format!("E{}", index + 1);
    }

    evidence
}

/// Leaves out the figures a reader may not read: `holds` says whether the reader holds a
/// permission.
pub fn withhold_figures(evidence: &mut [RcaEvidence], holds: impl Fn(&str) -> bool) {
    for piece in evidence {
        let permissions = piece.source.permissions();

        if !permissions.is_empty() && !permissions.iter().any(|permission| holds(permission)) {
            piece.withheld = !piece.figures.is_empty() || piece.withheld;
            piece.figures.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(kind: EvidenceKind, source: EvidenceSource) -> RcaEvidence {
        let mut piece = RcaEvidence::new(kind, source);
        piece.figures.insert("value".into(), 1.5);
        piece
    }

    #[test]
    fn pieces_are_labelled_in_order_and_capped() {
        let evidence = (0..MAX_RCA_EVIDENCE + 5)
            .map(|_| piece(EvidenceKind::Insight, EvidenceSource::Apm))
            .collect();

        let labelled = label_evidence(evidence);

        assert_eq!(labelled.len(), MAX_RCA_EVIDENCE);
        assert_eq!(labelled[0].label, "E1");
        assert_eq!(
            labelled[MAX_RCA_EVIDENCE - 1].label,
            format!("E{MAX_RCA_EVIDENCE}")
        );
    }

    #[test]
    fn figures_stay_only_with_readers_of_their_source() {
        let mut evidence = vec![
            piece(EvidenceKind::Insight, EvidenceSource::Apm),
            piece(EvidenceKind::ErrorLogs, EvidenceSource::Logs),
            piece(EvidenceKind::Change, EvidenceSource::Incident),
        ];

        withhold_figures(&mut evidence, |permission| {
            permission == "apm.services.read"
        });

        assert!(!evidence[0].withheld && !evidence[0].figures.is_empty());
        assert!(evidence[1].withheld && evidence[1].figures.is_empty());
        assert!(!evidence[2].withheld && !evidence[2].figures.is_empty());
    }
}
