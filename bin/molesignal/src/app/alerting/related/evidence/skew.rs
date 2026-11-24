// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Fields whose values the failed requests of the incident's services have apart from their other
//! requests around the time it began: a region, a version, a tenant that the failures crowd into.
//! Read from a sample of each, as the comparison on the Traces page reads it.

use domain::{
    alerting::rca_evidence::{EvidenceKind, EvidenceLink, EvidenceSource, RcaEvidence},
    query::{QueryLanguage, QueryRequest, StreamHint},
    stream::{StreamRepository, StreamType},
};
use kernel::{Result, time::TimeRange};

use super::Scope;
use crate::app::{
    attribute_diff::{Comparison, Selection, SpanComparison, compare_spans},
    query::{QueryService, statement::literal},
    trace::reading::{choose_traces_stream, entry_clause},
};

/// The most rows read of each set.
const SAMPLE_ROWS: usize = 500;
/// Fewer failed requests than this say nothing a field could be blamed for.
const FEWEST_FAILED: usize = 10;
/// A field is told when the two sets are at least this far apart on it.
const FAR_APART: f64 = 0.3;
/// The most fields told.
const TOLD_FIELDS: usize = 3;

pub(super) async fn skewed_fields(
    query: &QueryService,
    streams: &dyn StreamRepository,
    scope: &Scope<'_>,
) -> Result<Vec<RcaEvidence>> {
    let org_id = &scope.incident.org_id;

    let traces = streams
        .list(org_id)
        .await?
        .into_iter()
        .filter(|stream| stream.stream_type == StreamType::TRACES)
        .collect();

    let Some(definition) = choose_traces_stream(traces) else {
        return Ok(Vec::new());
    };

    let services = scope
        .services
        .iter()
        .map(|service| format!("'{}'", literal(service)))
        .collect::<Vec<_>>();

    let mut clauses = vec![format!("\"service.name\" IN ({})", services.join(", "))];
    clauses.extend(entry_clause(&definition.schema));

    let selection = Selection {
        errors: true,
        ..Selection::default()
    };

    let asked = SpanComparison {
        definition: &definition,
        clauses,
        selection: &selection,
        limit: SAMPLE_ROWS,
    };

    let run = |statement: String| {
        query.run(QueryRequest {
            org_id: org_id.clone(),
            language: QueryLanguage::Sql,
            statement,
            time_range: scope.window,
            stream: Some(StreamHint {
                name: definition.name.clone(),
                stream_type: StreamType::TRACES,
            }),
            limit: Some(SAMPLE_ROWS),
            step: None,
            federation_clusters: Vec::new(),
        })
    };

    let comparison = compare_spans(asked, run).await?;

    Ok(told(&comparison, scope.window))
}

/// The fields furthest apart, each with the value the failed requests crowd into. The link is the
/// window: a page reads the field and the value off the piece to show those requests.
fn told(comparison: &Comparison, window: TimeRange) -> Vec<RcaEvidence> {
    if comparison.selected < FEWEST_FAILED {
        return Vec::new();
    }

    comparison
        .dimensions
        .iter()
        .filter(|dimension| dimension.score >= FAR_APART)
        .filter_map(|dimension| {
            let crowded = dimension
                .values
                .iter()
                .find(|share| share.selected > share.baseline)?;

            let mut piece = RcaEvidence::new(EvidenceKind::AttributeSkew, EvidenceSource::Traces);

            piece.subject = Some(dimension.field.clone());
            piece.detail = Some(
                crowded
                    .value
                    .clone()
                    .unwrap_or_else(|| "(no value)".to_string()),
            );
            piece.figures.insert("failedShare".into(), crowded.selected);
            piece.figures.insert("otherShare".into(), crowded.baseline);
            piece.figures.insert("score".into(), dimension.score);
            piece
                .figures
                .insert("failed".into(), comparison.selected as f64);

            piece.link = Some(EvidenceLink {
                from: Some(window.start),
                to: Some(window.end),
                ..EvidenceLink::default()
            });

            Some(piece)
        })
        .take(TOLD_FIELDS)
        .collect()
}

#[cfg(test)]
mod tests {
    use kernel::time::TimestampMicros;

    use super::*;
    use crate::app::attribute_diff::{Dimension, ValueShare};

    fn dimension(field: &str, score: f64, value: Option<&str>) -> Dimension {
        Dimension {
            field: field.into(),
            score,
            values: vec![
                ValueShare {
                    value: Some("calm".into()),
                    selected: 0.1,
                    baseline: 0.6,
                },
                ValueShare {
                    value: value.map(str::to_string),
                    selected: 0.8,
                    baseline: 0.1,
                },
            ],
        }
    }

    #[test]
    fn the_fields_far_apart_are_told_with_the_value_failures_crowd_into() {
        let window = TimeRange::new(TimestampMicros(1), TimestampMicros(2));
        let comparison = Comparison {
            selected: 40,
            baseline: 500,
            sampled: true,
            dimensions: vec![
                dimension("cloud.region", 0.7, Some("eu-west-1")),
                dimension("tenant.id", 0.5, None),
                dimension("http.route", 0.1, Some("/cart")),
            ],
        };

        let told = told(&comparison, window);

        assert_eq!(told.len(), 2);
        assert_eq!(told[0].subject.as_deref(), Some("cloud.region"));
        assert_eq!(told[0].detail.as_deref(), Some("eu-west-1"));
        assert_eq!(told[0].figures.get("failedShare"), Some(&0.8));
        assert_eq!(told[1].detail.as_deref(), Some("(no value)"));

        let few = Comparison {
            selected: 3,
            ..comparison
        };
        assert!(super::told(&few, window).is_empty());
    }
}
