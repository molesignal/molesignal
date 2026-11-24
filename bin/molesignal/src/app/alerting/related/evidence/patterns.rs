// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Message patterns of an incident's services' error-level logs around the time it began that
//! the stretch of the same length just before did not have: what started being said when the
//! trouble did. Read from a sample, as the Patterns view of Logs reads them.

use domain::{
    alerting::rca_evidence::{EvidenceKind, EvidenceLink, EvidenceSource, RcaEvidence},
    stream::{MOLESIGNAL_SYSTEM_STREAM, StreamRepository, StreamType},
};
use kernel::{
    Result,
    time::{TimeRange, TimestampMicros},
};

use super::Scope;
use crate::app::{
    apm::correlation::{LOG_ERROR_LEVELS, LOG_LEVEL_FIELDS, LOG_SERVICE_FIELDS},
    log_patterns::{Change, Pattern, PatternQuery, analyze, message_field},
    query::{
        QueryService,
        baseline::Baseline,
        statement::{first_field, ident, literal},
    },
};

/// The most streams read: each is a count and a sample of two stretches.
const MOST_STREAMS: usize = 2;
/// The most new patterns told, of all streams.
const TOLD_PATTERNS: usize = 3;
/// The patterns of a stream looked through for new ones.
const LOOKED_THROUGH: usize = 50;

pub(super) async fn new_patterns(
    query: &QueryService,
    streams: &dyn StreamRepository,
    scope: &Scope<'_>,
) -> Result<Vec<RcaEvidence>> {
    let org_id = &scope.incident.org_id;

    let mut chosen = streams
        .list(org_id)
        .await?
        .into_iter()
        .filter(|stream| {
            stream.stream_type == StreamType::LOGS && stream.name != MOLESIGNAL_SYSTEM_STREAM
        })
        .filter_map(|stream| {
            let service = first_field(&stream, &LOG_SERVICE_FIELDS)?;
            let level = first_field(&stream, &LOG_LEVEL_FIELDS)?;
            let message = message_field(&stream.schema, &stream.name, None).ok()?;
            Some((stream.name, service, level, message))
        })
        .collect::<Vec<_>>();

    chosen.sort_by(|left, right| {
        (left.0 != "default", &left.0).cmp(&(right.0 != "default", &right.0))
    });
    chosen.truncate(MOST_STREAMS);

    let services = scope
        .services
        .iter()
        .map(|service| format!("'{}'", literal(service)))
        .collect::<Vec<_>>()
        .join(", ");

    let mut found = Vec::<(String, Pattern)>::new();

    for (stream, service, level, message) in &chosen {
        let filter = format!(
            " AND (lower(CAST(\"{level}\" AS VARCHAR)) IN ({LOG_ERROR_LEVELS})) AND (\"{service}\" IN \
             ({services}))",
            level = ident(level),
            service = ident(service),
        );

        let asked = PatternQuery {
            org_id,
            stream,
            message,
            level: Some(level),
            filter,
            window: scope.window,
            baseline: Baseline::Previous,
            limit: LOOKED_THROUGH,
            tokens: false,
        };

        // A stream that cannot be read leaves the others to tell.
        let Ok(report) = analyze(query, &asked).await else {
            tracing::debug!(stream = %stream, "error log patterns could not be read");
            continue;
        };

        found.extend(
            report
                .patterns
                .into_iter()
                .filter(|pattern| pattern.change == Some(Change::New))
                .map(|pattern| (stream.clone(), pattern)),
        );
    }

    Ok(told(found, scope.window))
}

/// The new patterns with the most lines, as evidence.
fn told(mut found: Vec<(String, Pattern)>, window: TimeRange) -> Vec<RcaEvidence> {
    found.sort_by(|left, right| {
        lines(&right.1)
            .total_cmp(&lines(&left.1))
            .then_with(|| left.1.pattern.cmp(&right.1.pattern))
    });
    found.truncate(TOLD_PATTERNS);

    found
        .into_iter()
        .map(|(stream, pattern)| {
            let mut piece = RcaEvidence::new(EvidenceKind::NewLogPattern, EvidenceSource::Logs);

            piece.subject = Some(pattern.pattern.clone());
            piece.detail = pattern.example.clone();
            piece.at = pattern.first_seen.map(TimestampMicros);
            piece.figures.insert("lines".into(), lines(&pattern));

            piece.link = Some(EvidenceLink {
                stream: Some(stream),
                from: Some(window.start),
                to: Some(window.end),
                ..EvidenceLink::default()
            });

            piece
        })
        .collect()
}

/// A pattern's lines over the whole window: estimated when the window was sampled.
fn lines(pattern: &Pattern) -> f64 {
    pattern.estimated_count.unwrap_or(pattern.count as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(text: &str, count: usize, estimated: Option<f64>) -> Pattern {
        Pattern {
            pattern: text.into(),
            tokens: Vec::new(),
            count,
            share: 0.0,
            estimated_count: estimated,
            first_seen: Some(5),
            last_seen: Some(9),
            levels: Vec::new(),
            baseline_count: Some(0),
            change: Some(Change::New),
            ratio: None,
            fragment: None,
            example: Some(format!("{text} example")),
        }
    }

    #[test]
    fn the_new_patterns_with_the_most_lines_are_told_with_their_stream_and_example() {
        let window = TimeRange::new(TimestampMicros(0), TimestampMicros(100));

        let told = told(
            vec![
                ("app".into(), pattern("pool exhausted", 4, Some(400.0))),
                ("app".into(), pattern("retry <number>", 30, None)),
                ("web".into(), pattern("tls handshake failed", 2, None)),
                ("web".into(), pattern("cache miss", 1, None)),
            ],
            window,
        );

        assert_eq!(told.len(), 3);
        assert_eq!(told[0].subject.as_deref(), Some("pool exhausted"));
        assert_eq!(told[0].figures.get("lines"), Some(&400.0));
        assert_eq!(told[0].detail.as_deref(), Some("pool exhausted example"));
        assert_eq!(told[0].at, Some(TimestampMicros(5)));
        assert_eq!(told[1].subject.as_deref(), Some("retry <number>"));
        assert_eq!(
            told[2]
                .link
                .as_ref()
                .and_then(|link| link.stream.as_deref()),
            Some("web")
        );
    }
}
