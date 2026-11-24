// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Error-level log lines of an incident's services around the time it
//! began, against the same stretch a week before. Only counts are read.

use std::collections::{BTreeMap, BTreeSet};

use domain::{
    alerting::rca_evidence::{EvidenceKind, EvidenceLink, EvidenceSource, RcaEvidence},
    query::{QueryLanguage, QueryRequest, StreamHint},
    stream::{MOLESIGNAL_SYSTEM_STREAM, StreamRepository, StreamType},
};
use kernel::{
    Result,
    time::{TimeRange, TimestampMicros},
};

use super::Scope;
use crate::app::{
    apm::{
        ApmQueryService, OrgNames,
        correlation::{LOG_ERROR_LEVELS, LOG_LEVEL_FIELDS, LOG_SERVICE_FIELDS},
    },
    query::{
        QueryService,
        statement::{count, first_field, ident},
    },
};

const MOST_STREAMS: usize = 4;
const MOST_NAMES: usize = 200;
const TOLD_SERVICES: usize = 4;
const WEEK_MICROS: i64 = 7 * 86_400_000_000;

/// What the logs of one service said.
#[derive(Debug, Default)]
pub(super) struct ErrorLines {
    pub now: u64,
    pub last_week: u64,
    pub streams: BTreeSet<String>,
}

pub(super) async fn error_logs(
    apm: &ApmQueryService,
    query: &QueryService,
    streams: &dyn StreamRepository,
    scope: &Scope<'_>,
) -> Result<Vec<RcaEvidence>> {
    let org_id = &scope.incident.org_id;
    let names = OrgNames::of(apm.org_settings(org_id).await.as_ref());

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
            Some((stream.name, service, level))
        })
        .collect::<Vec<_>>();

    chosen.sort_by(|left, right| {
        (left.0 != "default", &left.0).cmp(&(right.0 != "default", &right.0))
    });
    chosen.truncate(MOST_STREAMS);

    let week_before = TimeRange::new(
        TimestampMicros(scope.window.start.0 - WEEK_MICROS),
        TimestampMicros(scope.window.end.0 - WEEK_MICROS),
    );

    let mut by_service = BTreeMap::<String, ErrorLines>::new();

    for (stream, service, level) in &chosen {
        let statement = format!(
            "SELECT \"{service}\" AS svc, count(*) AS errors FROM \"{from}\" \
             WHERE lower(CAST(\"{level}\" AS VARCHAR)) IN ({LOG_ERROR_LEVELS}) \
             GROUP BY \"{service}\" ORDER BY errors DESC LIMIT {MOST_NAMES}",
            service = ident(service),
            from = ident(stream),
            level = ident(level),
        );

        let (now, before) = tokio::join!(
            counts(query, scope, stream, statement.clone(), scope.window),
            counts(query, scope, stream, statement, week_before),
        );

        // A stream that cannot be read leaves the others to tell.
        let (Ok(now), Ok(before)) = (now, before) else {
            tracing::debug!(stream = %stream, "error log counts could not be read");
            continue;
        };

        for (raw, errors, week) in now
            .into_iter()
            .map(|(name, errors)| (name, errors, false))
            .chain(
                before
                    .into_iter()
                    .map(|(name, errors)| (name, errors, true)),
            )
        {
            let name = names.service(&raw);

            if !scope.services.contains(&name) {
                continue;
            }

            let entry = by_service.entry(name).or_default();
            entry.streams.insert(stream.clone());

            if week {
                entry.last_week += errors;
            } else {
                entry.now += errors;
            }
        }
    }

    Ok(told(by_service, scope.window))
}

/// The services with error lines now or a week before, the most now first.
pub(super) fn told(
    by_service: BTreeMap<String, ErrorLines>,
    window: TimeRange,
) -> Vec<RcaEvidence> {
    let mut told = by_service
        .into_iter()
        .filter(|(_, lines)| lines.now > 0 || lines.last_week > 0)
        .collect::<Vec<_>>();

    told.sort_by_key(|(name, lines)| (std::cmp::Reverse(lines.now), name.clone()));
    told.truncate(TOLD_SERVICES);

    told.into_iter()
        .map(|(service, lines)| {
            let mut piece = RcaEvidence::new(EvidenceKind::ErrorLogs, EvidenceSource::Logs);

            piece.service = Some(service);
            piece.subject = lines.streams.iter().next().cloned();
            piece.figures.insert("now".into(), lines.now as f64);

            piece
                .figures
                .insert("lastWeek".into(), lines.last_week as f64);

            piece.link = Some(EvidenceLink {
                stream: lines.streams.into_iter().next(),
                from: Some(window.start),
                to: Some(window.end),
                ..EvidenceLink::default()
            });

            piece
        })
        .collect()
}

async fn counts(
    query: &QueryService,
    scope: &Scope<'_>,
    stream: &str,
    statement: String,
    range: TimeRange,
) -> Result<Vec<(String, u64)>> {
    let result = query
        .run(QueryRequest {
            org_id: scope.incident.org_id.clone(),
            language: QueryLanguage::Sql,
            statement,
            time_range: range,
            stream: Some(StreamHint {
                name: stream.to_owned(),
                stream_type: StreamType::LOGS,
            }),
            limit: Some(MOST_NAMES),
            step: None,
            federation_clusters: Vec::new(),
        })
        .await?;

    Ok(result
        .rows
        .iter()
        .filter_map(|row| {
            let name = row.first()?.as_str()?.trim();
            (!name.is_empty()).then(|| (name.to_owned(), row.get(1).map_or(0, count)))
        })
        .collect())
}
