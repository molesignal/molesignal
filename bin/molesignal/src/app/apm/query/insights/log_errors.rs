// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Services whose error-level log lines rose: each service's lines a minute over the range,
//! judged by the MAD detector of anomaly rules against the lines a minute of each part of the
//! stretch before and of the same stretch a week before. Logs name a service only by its name,
//! so the lines go to the service of that name in view with the most requests.
//!
//! Best effort: the logs are read beside the spans' buckets, and a range they cannot be read in
//! within [`LOOK_WAIT`] goes without these insights.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::Duration,
};

use domain::{
    alerting::anomaly::{AnomalyDetector, MadDetector},
    apm::ServiceIdentity,
    query::{QueryLanguage, QueryRequest, StreamHint},
    stream::{MOLESIGNAL_SYSTEM_STREAM, StreamRepository, StreamType},
};
use kernel::{
    Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};

use super::{
    super::RedSummary,
    WEEK_MICROS,
    model::{Insight, InsightKind, InsightSeverity},
};
use crate::app::{
    apm::{
        OrgNames,
        correlation::{LOG_ERROR_LEVELS, LOG_LEVEL_FIELDS, LOG_SERVICE_FIELDS},
    },
    query::{
        QueryService,
        statement::{count, first_field, ident},
    },
};

/// Where error-level log lines are read from.
#[derive(Clone)]
pub struct LogSources {
    pub query: Arc<QueryService>,
    pub streams: Arc<dyn StreamRepository>,
}

/// Each stretch is told in this many equal parts.
const PARTS: i64 = 12;
const MOST_STREAMS: usize = 4;
const MOST_ROWS: usize = 5_000;
/// Fewer lines than this in the range, or more than the baseline by fewer, is not a rise.
const MIN_LINES: f64 = 20.0;
/// How many robust deviations from the baseline a rise is, as an anomaly rule's default.
const DEVIATIONS: f64 = 3.0;
/// The rise is at least this many times the busier stretch before.
const MIN_RATIO: f64 = 2.0;
/// From this many times the busier stretch before, a rise is critical.
const CRITICAL_RATIO: f64 = 10.0;
const LOOK_WAIT: Duration = Duration::from_secs(3);

/// A service's error lines, a part of the stretch at a time; a stretch the logs did not reach
/// is absent.
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct Lines {
    pub now: Vec<f64>,
    pub previous: Option<Vec<f64>>,
    pub last_week: Option<Vec<f64>>,
}

/// What a rise is said with: lines a minute now, before and a week before, and when it began.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Rise {
    pub now: f64,
    pub previous: Option<f64>,
    pub last_week: Option<f64>,
    pub since: TimestampMicros,
    pub critical: bool,
}

/// The rise of `lines` over `range`, if they rose.
pub(super) fn rise(lines: &Lines, range: TimeRange) -> Option<Rise> {
    let part_minutes = range.duration_micros().max(1) as f64 / PARTS as f64 / 60_000_000.0;
    let total: f64 = lines.now.iter().sum();

    if total < MIN_LINES {
        return None;
    }

    let history = lines
        .previous
        .iter()
        .chain(lines.last_week.iter())
        .flatten()
        .map(|part| part / part_minutes)
        .collect::<Vec<_>>();

    if history.is_empty() {
        return None;
    }

    let now = total / (PARTS as f64 * part_minutes);
    let rate = |parts: &Option<Vec<f64>>| {
        parts
            .as_ref()
            .map(|parts| parts.iter().sum::<f64>() / (PARTS as f64 * part_minutes))
    };
    let (previous, last_week) = (rate(&lines.previous), rate(&lines.last_week));
    let busier = previous.unwrap_or(0.0).max(last_week.unwrap_or(0.0));
    let decision = MadDetector::new(DEVIATIONS).detect(now, &history);

    let risen = decision.firing
        && now > decision.baseline
        && now >= busier * MIN_RATIO
        && (now - busier) * PARTS as f64 * part_minutes >= MIN_LINES;

    if !risen {
        return None;
    }

    // It began in the first part past halfway between the baseline and now.
    let halfway = (decision.baseline + now) / 2.0;
    let first = lines
        .now
        .iter()
        .position(|part| part / part_minutes > halfway)
        .unwrap_or(0);
    let part_micros = range.duration_micros() / PARTS;

    Some(Rise {
        now,
        previous,
        last_week,
        since: TimestampMicros(range.start.0 + first as i64 * part_micros),
        critical: now >= busier.max(f64::EPSILON) * CRITICAL_RATIO,
    })
}

/// The insights of the services whose error lines rose; `in_view` are the services with their
/// request counts.
pub(super) fn insights(
    lines: BTreeMap<String, Lines>,
    in_view: &HashMap<ServiceIdentity, RedSummary>,
    range: TimeRange,
) -> Vec<Insight> {
    let mut found = lines
        .into_iter()
        .filter_map(|(name, lines)| {
            let service = in_view
                .iter()
                .filter(|(service, _)| service.name == name)
                .max_by_key(|(service, red)| {
                    (
                        red.request_count,
                        service.environment.clone(),
                        service.namespace.clone(),
                    )
                })
                .map(|(service, _)| service.clone())?;
            let rise = rise(&lines, range)?;

            let severity = if rise.critical {
                InsightSeverity::Critical
            } else {
                InsightSeverity::Warning
            };
            let mut insight = Insight::new(InsightKind::LogErrors, severity, service, rise.now);

            insight.metric = Some("log_errors");
            insight.previous = rise.previous;
            insight.last_week = rise.last_week;
            insight.since = Some(rise.since);
            Some(insight)
        })
        .collect::<Vec<_>>();

    found.sort_by(|left, right| right.value.total_cmp(&left.value));
    found
}

impl LogSources {
    /// Each service's error lines over `range`, the stretch before and the same stretch a week
    /// before, by the names `names` give them; nothing when they cannot be read in time.
    pub(super) async fn lines(
        &self,
        org_id: &Id,
        names: &OrgNames,
        range: TimeRange,
    ) -> BTreeMap<String, Lines> {
        match tokio::time::timeout(LOOK_WAIT, self.read(org_id, names, range)).await {
            Ok(Ok(lines)) => lines,
            Ok(Err(error)) => {
                tracing::debug!(%error, "error log lines could not be read for insights");
                BTreeMap::new()
            }
            Err(_) => {
                tracing::debug!("error log lines took too long to read for insights");
                BTreeMap::new()
            }
        }
    }

    async fn read(
        &self,
        org_id: &Id,
        names: &OrgNames,
        range: TimeRange,
    ) -> Result<BTreeMap<String, Lines>> {
        let mut chosen = self
            .streams
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

        let length = range.duration_micros().max(PARTS);
        let stretches = [
            range,
            TimeRange::new(
                TimestampMicros(range.start.0 - length),
                TimestampMicros(range.end.0 - length),
            ),
            TimeRange::new(
                TimestampMicros(range.start.0 - WEEK_MICROS),
                TimestampMicros(range.end.0 - WEEK_MICROS),
            ),
        ];
        let mut lines = BTreeMap::<String, Lines>::new();

        for (stream, service, level) in &chosen {
            let (now, previous, week) = tokio::join!(
                self.parts(org_id, stream, service, level, stretches[0]),
                self.parts(org_id, stream, service, level, stretches[1]),
                self.parts(org_id, stream, service, level, stretches[2]),
            );

            // A stream that cannot be read leaves the others to tell.
            let (Ok(now), Ok(previous), Ok(week)) = (now, previous, week) else {
                tracing::debug!(stream = %stream, "error log lines could not be read");
                continue;
            };

            // A stretch the stream has no error line of at all may be before it was written to.
            let reached = |parts: &HashMap<String, Vec<f64>>| !parts.is_empty();
            let (previous_reached, week_reached) = (reached(&previous), reached(&week));

            for (raw, parts) in now {
                let name = names.service(&raw);
                let entry = lines.entry(name.clone()).or_insert_with(|| Lines {
                    now: vec![0.0; PARTS as usize],
                    ..Lines::default()
                });

                add(&mut entry.now, &parts);

                for (reached, stretch, kept) in [
                    (previous_reached, &previous, &mut entry.previous),
                    (week_reached, &week, &mut entry.last_week),
                ] {
                    if reached {
                        let kept = kept.get_or_insert_with(|| vec![0.0; PARTS as usize]);

                        if let Some(parts) = stretch.get(&raw) {
                            add(kept, parts);
                        }
                    }
                }
            }
        }

        Ok(lines)
    }

    /// The error lines of each service of `stream` over `range`, a part at a time.
    async fn parts(
        &self,
        org_id: &Id,
        stream: &str,
        service: &str,
        level: &str,
        range: TimeRange,
    ) -> Result<HashMap<String, Vec<f64>>> {
        let part = (range.duration_micros() / PARTS).max(1);
        let start = range.start.0;
        let statement = format!(
            "SELECT \"{service}\" AS svc, (CAST(_timestamp AS BIGINT) - {start}) / {part} AS part, \
             count(*) AS lines FROM \"{from}\" \
             WHERE lower(CAST(\"{level}\" AS VARCHAR)) IN ({LOG_ERROR_LEVELS}) \
             GROUP BY \"{service}\", (CAST(_timestamp AS BIGINT) - {start}) / {part} \
             LIMIT {MOST_ROWS}",
            service = ident(service),
            from = ident(stream),
            level = ident(level),
        );

        let result = self
            .query
            .run(QueryRequest {
                org_id: org_id.clone(),
                language: QueryLanguage::Sql,
                statement,
                time_range: range,
                stream: Some(StreamHint {
                    name: stream.to_owned(),
                    stream_type: StreamType::LOGS,
                }),
                limit: Some(MOST_ROWS),
                step: None,
                federation_clusters: Vec::new(),
            })
            .await?;

        let mut parts = HashMap::<String, Vec<f64>>::new();

        for row in &result.rows {
            let Some(name) = row.first().and_then(|value| value.as_str()).map(str::trim) else {
                continue;
            };

            if name.is_empty() {
                continue;
            }

            let index = row.get(1).map_or(0, count).min(PARTS as u64 - 1) as usize;
            let entry = parts
                .entry(name.to_owned())
                .or_insert_with(|| vec![0.0; PARTS as usize]);

            entry[index] += row.get(2).map_or(0, count) as f64;
        }

        Ok(parts)
    }
}

impl super::super::ApmQueryService {
    /// The error lines of the services in view over `context`'s range, when logs are read.
    pub(super) async fn error_log_lines(
        &self,
        context: &super::super::ApmQueryContext,
    ) -> Result<BTreeMap<String, Lines>> {
        let Some(logs) = &self.logs else {
            return Ok(BTreeMap::new());
        };

        let names = OrgNames::of(self.org_settings(&context.org_id).await.as_ref());
        Ok(logs.lines(&context.org_id, &names, context.range).await)
    }
}

fn add(into: &mut [f64], parts: &[f64]) {
    for (kept, part) in into.iter_mut().zip(parts) {
        *kept += part;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000_000;

    fn range() -> TimeRange {
        TimeRange::new(TimestampMicros(100 * HOUR), TimestampMicros(101 * HOUR))
    }

    fn flat(value: f64) -> Vec<f64> {
        vec![value; PARTS as usize]
    }

    #[test]
    fn lines_that_jump_from_a_steady_baseline_rise_from_where_they_jumped() {
        let mut now = flat(2.0);
        now[8..].fill(120.0);

        let lines = Lines {
            now,
            previous: Some(flat(2.0)),
            last_week: Some(flat(3.0)),
        };

        let rise = rise(&lines, range()).expect("a rise");

        assert!(rise.now > 8.0, "{rise:?}");
        assert_eq!(rise.previous, Some(0.4));
        assert_eq!(rise.since, TimestampMicros(100 * HOUR + 8 * HOUR / PARTS));
        assert!(rise.critical);
    }

    #[test]
    fn lines_like_the_week_before_or_too_few_are_not_a_rise() {
        let busy_week = Lines {
            now: flat(40.0),
            previous: Some(flat(5.0)),
            last_week: Some(flat(40.0)),
        };
        assert_eq!(rise(&busy_week, range()), None);

        let few = Lines {
            now: vec![1.0; PARTS as usize],
            previous: Some(flat(0.0)),
            last_week: None,
        };
        assert_eq!(rise(&few, range()), None);

        // Nothing to compare with: a stream that did not reach back.
        let unknown = Lines {
            now: flat(50.0),
            previous: None,
            last_week: None,
        };
        assert_eq!(rise(&unknown, range()), None);
    }

    #[test]
    fn the_lines_go_to_the_busiest_service_of_that_name() {
        let service = |environment: &str| ServiceIdentity {
            namespace: "shop".into(),
            name: "checkout".into(),
            environment: environment.into(),
        };
        let red = |requests: u64| RedSummary {
            request_count: requests,
            ..RedSummary::default()
        };
        let in_view = HashMap::from([(service("prod"), red(500)), (service("staging"), red(5))]);
        let mut now = flat(1.0);
        now[6..].fill(30.0);

        let found = insights(
            BTreeMap::from([
                (
                    "checkout".to_owned(),
                    Lines {
                        now,
                        previous: Some(flat(1.0)),
                        last_week: None,
                    },
                ),
                ("unknown".to_owned(), Lines::default()),
            ]),
            &in_view,
            range(),
        );

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, InsightKind::LogErrors);
        assert_eq!(found[0].metric, Some("log_errors"));
        assert_eq!(found[0].service.environment, "prod");
        assert!(found[0].since.is_some());
    }
}
