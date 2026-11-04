// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The reads a RUM figure is made of.
//!
//! A figure is not one number: a share is two counts, a percentile is the percentile and how many
//! views had a value. The numbers of one stream are asked together, so the figures that come from
//! the same stream and the same scope are one read, which every rule and preview of them shares.
//!
//! What a figure says is an estimate of all the sessions, when the application keeps a share of
//! them. How much there is to say it by is the rows that are stored: a rule that waits for enough
//! rows waits for those, not for what they stand for.

use std::collections::HashMap;

use crate::{
    domain::rum::{
        RumAlertMetric, RumAlertQuery, RumAlertReading, RumAlertSearch, RumAlertStatistic,
        event::RumStream,
    },
    infra::rum::query::{Aggregate, Filter, MeasureSpec},
};

/// The SDKs send durations in nanoseconds; rules and pages say milliseconds.
const NANOSECONDS_PER_MILLISECOND: f64 = 1_000_000.0;

pub(super) const VIEWS: &str = "views";
/// The rows behind `views`, `sessions` and `requests`: what there is to go by.
pub(super) const VIEWS_STORED: &str = "views_stored";
pub(super) const SESSIONS_STORED: &str = "sessions_stored";
pub(super) const REQUESTS_STORED: &str = "requests_stored";
pub(super) const ERRORED: &str = "errored";
pub(super) const LCP: &str = "lcp";
pub(super) const LCP_N: &str = "lcp_n";
pub(super) const INP: &str = "inp";
pub(super) const INP_N: &str = "inp_n";
pub(super) const CLS: &str = "cls";
pub(super) const CLS_N: &str = "cls_n";
pub(super) const TTFB: &str = "ttfb";
pub(super) const TTFB_N: &str = "ttfb_n";
pub(super) const SESSIONS: &str = "sessions";
pub(super) const CRASHED: &str = "crashed";
pub(super) const FRUSTRATED: &str = "frustrated";
pub(super) const REQUESTS: &str = "requests";
pub(super) const NO_ANSWER: &str = "no_answer";
pub(super) const SERVER_ERROR: &str = "server_error";
pub(super) const ERRORS: &str = "errors";
pub(super) const ISSUES: &str = "issues";
/// A figure the application measures itself, and how many rows it is made of.
pub(super) const CUSTOM: &str = "custom";
pub(super) const CUSTOM_N: &str = "custom_n";

/// The sessions the worst issue touched, and what its errors say which issue they are of.
pub(super) const IMPACT: &str = "impact";
pub(super) const ISSUE_FIELD: &str = "error.issue_id";

/// What a read is of: a stream, or the issues kept in the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Group {
    Views,
    Sessions,
    Resources,
    Errors,
    NewIssues,
    RegressedIssues,
    CustomTiming,
    CustomVital,
    IssueImpact,
    /// The rows of the type an explorer query is of.
    Query(RumStream),
}

impl Group {
    /// The read a rule's figure is made of. A query of a type there is none of reads the views,
    /// which the rule's check refused before.
    pub(super) fn of(query: &RumAlertQuery) -> Self {
        if let Some(search) = &query.search {
            return Self::Query(search.stream().unwrap_or(RumStream::Views));
        }

        Self::of_metric(query.metric)
    }

    fn of_metric(metric: RumAlertMetric) -> Self {
        match metric {
            RumAlertMetric::ViewCount
            | RumAlertMetric::ErrorRate
            | RumAlertMetric::LcpP75
            | RumAlertMetric::InpP75
            | RumAlertMetric::ClsP75
            | RumAlertMetric::TtfbP75 => Self::Views,
            RumAlertMetric::SessionCount
            | RumAlertMetric::CrashFreeSessions
            | RumAlertMetric::FrustrationRate => Self::Sessions,
            RumAlertMetric::FailedRequestRate => Self::Resources,
            RumAlertMetric::ErrorCount => Self::Errors,
            RumAlertMetric::NewIssues => Self::NewIssues,
            RumAlertMetric::RegressedIssues => Self::RegressedIssues,
            RumAlertMetric::CustomTiming => Self::CustomTiming,
            RumAlertMetric::CustomVital => Self::CustomVital,
            RumAlertMetric::IssueImpact => Self::IssueImpact,
            RumAlertMetric::Query => Self::Query(RumStream::Views),
        }
    }

    /// The stream the read is made over; none for the issues.
    pub(super) fn stream(self) -> Option<RumStream> {
        match self {
            Self::Views | Self::CustomTiming => Some(RumStream::Views),
            Self::CustomVital => Some(RumStream::Vitals),
            Self::Sessions => Some(RumStream::Sessions),
            Self::Resources => Some(RumStream::Resources),
            Self::Errors | Self::IssueImpact => Some(RumStream::Errors),
            Self::Query(stream) => Some(stream),
            Self::NewIssues | Self::RegressedIssues => None,
        }
    }

    /// The numbers taken from the stream together.
    pub(super) fn measures(self) -> Vec<MeasureSpec> {
        match self {
            Self::Views => vec![
                count(VIEWS, None, Vec::new()),
                stored(VIEWS_STORED, None, Vec::new()),
                count(ERRORED, None, vec![filter("view.error.count", ">", "0")]),
                percentile(LCP, "view.performance.lcp.timestamp"),
                stored(LCP_N, Some("view.performance.lcp.timestamp"), Vec::new()),
                percentile(INP, "view.performance.inp.duration"),
                stored(INP_N, Some("view.performance.inp.duration"), Vec::new()),
                percentile(CLS, "view.performance.cls.score"),
                stored(CLS_N, Some("view.performance.cls.score"), Vec::new()),
                percentile(TTFB, "view.first_byte"),
                stored(TTFB_N, Some("view.first_byte"), Vec::new()),
            ],
            Self::Sessions => vec![
                count(SESSIONS, None, Vec::new()),
                stored(SESSIONS_STORED, None, Vec::new()),
                count(CRASHED, None, vec![filter("session.crash.count", ">", "0")]),
                count(
                    FRUSTRATED,
                    None,
                    vec![filter("session.frustration.count", ">", "0")],
                ),
            ],
            // The requests a page makes of its own back end, told apart from the files it loads.
            Self::Resources => vec![
                count(REQUESTS, None, vec![calls()]),
                stored(REQUESTS_STORED, None, vec![calls()]),
                count(
                    NO_ANSWER,
                    None,
                    vec![calls(), filter("resource.status_code", "=", "0")],
                ),
                count(
                    SERVER_ERROR,
                    None,
                    vec![calls(), filter("resource.status_code", ">=", "500")],
                ),
            ],
            Self::Errors => vec![count(ERRORS, None, Vec::new())],
            // By issue, with the one that ranks first answering: errors of no issue are not one.
            Self::IssueImpact => vec![measure(
                "count_distinct",
                IMPACT,
                Some("session.id"),
                vec![filter(ISSUE_FIELD, "exists", "")],
            )],
            Self::NewIssues
            | Self::RegressedIssues
            | Self::CustomTiming
            | Self::CustomVital
            | Self::Query(_) => Vec::new(),
        }
    }
}

/// The numbers taken for a rule: those of its group, of the figure the application measures, or
/// of the query.
pub(super) fn measures_of(query: &RumAlertQuery) -> Vec<MeasureSpec> {
    if let Some(search) = &query.search {
        return searched(search);
    }

    let timing;

    let field = match (query.metric, query.custom_name.as_deref()) {
        (RumAlertMetric::CustomVital, _) => "vital.duration",
        (RumAlertMetric::CustomTiming, Some(name)) => {
            timing = format!("view.custom_timings.{name}");
            timing.as_str()
        }
        (metric, _) => return Group::of_metric(metric).measures(),
    };

    vec![
        measure(
            query.effective_statistic().function(),
            CUSTOM,
            Some(field),
            Vec::new(),
        ),
        stored(CUSTOM_N, Some(field), Vec::new()),
    ]
}

/// The number of a query, and how many rows it is made of: of the rows that have the field, or of
/// every row the query matches.
fn searched(search: &RumAlertSearch) -> Vec<MeasureSpec> {
    let field = search.measure.field.as_deref();

    vec![
        measure(&search.measure.function, CUSTOM, field, Vec::new()),
        stored(CUSTOM_N, field, Vec::new()),
    ]
}

/// The figure a rule watches, out of the numbers of its read.
pub(super) fn figure_of(query: &RumAlertQuery, numbers: &Numbers) -> Option<RumAlertReading> {
    if let Some(search) = &query.search {
        let divisor = if search.in_nanoseconds() {
            NANOSECONDS_PER_MILLISECOND
        } else {
            1.0
        };

        // A count or a sum is nought where nothing happened; the rest need rows to say anything.
        if search.adds_up() {
            return Some(RumAlertReading {
                value: numbers.get(CUSTOM).unwrap_or(0.0) / divisor,
                samples: numbers.count(CUSTOM_N),
            });
        }

        return percentile_of(
            numbers,
            (CUSTOM, CUSTOM_N),
            divisor,
            query.effective_min_samples(),
        );
    }

    if !query.metric.is_custom() {
        return figure(query.metric, numbers, query.effective_min_samples());
    }

    if query.effective_statistic() == RumAlertStatistic::Count {
        return Some(counted(numbers.count(CUSTOM)));
    }

    percentile_of(
        numbers,
        (CUSTOM, CUSTOM_N),
        NANOSECONDS_PER_MILLISECOND,
        query.effective_min_samples(),
    )
}

/// The filter that keeps the requests of `fetch` and `xhr`.
pub(super) fn calls() -> Filter {
    filter("resource.type", "in", r#"["fetch","xhr"]"#)
}

fn filter(field: &str, op: &str, value: &str) -> Filter {
    Filter {
        field: field.into(),
        op: op.into(),
        value: value.into(),
        quoted: false,
    }
}

fn count(name: &str, field: Option<&str>, filters: Vec<Filter>) -> MeasureSpec {
    measure("count", name, field, filters)
}

/// The rows that are stored, whatever they stand for.
fn stored(name: &str, field: Option<&str>, filters: Vec<Filter>) -> MeasureSpec {
    measure("stored", name, field, filters)
}

fn percentile(name: &str, field: &str) -> MeasureSpec {
    measure("p75", name, Some(field), Vec::new())
}

fn measure(function: &str, name: &str, field: Option<&str>, filters: Vec<Filter>) -> MeasureSpec {
    MeasureSpec {
        function: function.into(),
        field: field.map(str::to_string),
        fallback: None,
        filters,
        name: Some(name.into()),
    }
}

/// The numbers one read came to, by the names of its measures. A number the stream has none of,
/// or that the reader may not see, is not here.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Numbers(HashMap<String, f64>);

impl Numbers {
    /// The numbers of the first row of an answer.
    pub(super) fn of(aggregate: &Aggregate) -> Self {
        Self::of_row(aggregate, 0)
    }

    pub(super) fn of_row(aggregate: &Aggregate, row: usize) -> Self {
        let Some(row) = aggregate.rows.get(row) else {
            return Self::default();
        };

        Self(
            aggregate
                .measures
                .iter()
                .zip(&row.values)
                .filter_map(|(info, value)| Some((info.name.clone(), value.as_f64()?)))
                .collect(),
        )
    }

    pub(super) fn only(name: &str, value: f64) -> Self {
        Self(HashMap::from([(name.to_string(), value)]))
    }

    pub(super) fn get(&self, name: &str) -> Option<f64> {
        self.0.get(name).copied()
    }

    /// A count; zero where there is none.
    pub(super) fn count(&self, name: &str) -> u64 {
        self.get(name)
            .map_or(0, |value| value.max(0.0).round() as u64)
    }
}

fn counted(count: u64) -> RumAlertReading {
    RumAlertReading {
        value: count as f64,
        samples: count,
    }
}

/// `part` of `whole` in percent, said by `rows` that are stored; none without enough of them.
fn share(part: u64, whole: u64, rows: u64, min_samples: u32) -> Option<RumAlertReading> {
    if whole == 0 || rows == 0 || rows < u64::from(min_samples) {
        return None;
    }

    Some(RumAlertReading {
        value: part.min(whole) as f64 * 100.0 / whole as f64,
        samples: rows,
    })
}

/// A percentile of `value` over the `rows` that had one, in the unit of the metric.
fn percentile_of(
    numbers: &Numbers,
    (value, rows): (&str, &str),
    divisor: f64,
    min_samples: u32,
) -> Option<RumAlertReading> {
    let samples = numbers.count(rows);

    if samples == 0 || samples < u64::from(min_samples) {
        return None;
    }

    Some(RumAlertReading {
        value: numbers.get(value)? / divisor,
        samples,
    })
}

/// The figure of a metric out of the numbers of its read. A count is a figure even when it is zero,
/// which is how a rule can watch traffic stop; the rest have none without the rows they are made
/// of, and fewer rows than `min_samples` are too few to say.
pub(super) fn figure(
    metric: RumAlertMetric,
    numbers: &Numbers,
    min_samples: u32,
) -> Option<RumAlertReading> {
    match metric {
        RumAlertMetric::SessionCount => Some(counted(numbers.count(SESSIONS))),
        RumAlertMetric::ViewCount => Some(counted(numbers.count(VIEWS))),
        RumAlertMetric::ErrorCount => Some(counted(numbers.count(ERRORS))),
        RumAlertMetric::NewIssues | RumAlertMetric::RegressedIssues => {
            Some(counted(numbers.count(ISSUES)))
        }
        RumAlertMetric::IssueImpact => Some(counted(numbers.count(IMPACT))),
        RumAlertMetric::ErrorRate => share(
            numbers.count(ERRORED),
            numbers.count(VIEWS),
            numbers.count(VIEWS_STORED),
            min_samples,
        ),
        RumAlertMetric::CrashFreeSessions => {
            let sessions = numbers.count(SESSIONS);

            share(
                sessions.saturating_sub(numbers.count(CRASHED)),
                sessions,
                numbers.count(SESSIONS_STORED),
                min_samples,
            )
        }
        RumAlertMetric::FrustrationRate => share(
            numbers.count(FRUSTRATED),
            numbers.count(SESSIONS),
            numbers.count(SESSIONS_STORED),
            min_samples,
        ),
        RumAlertMetric::FailedRequestRate => share(
            numbers.count(NO_ANSWER) + numbers.count(SERVER_ERROR),
            numbers.count(REQUESTS),
            numbers.count(REQUESTS_STORED),
            min_samples,
        ),
        RumAlertMetric::LcpP75 => percentile_of(
            numbers,
            (LCP, LCP_N),
            NANOSECONDS_PER_MILLISECOND,
            min_samples,
        ),
        RumAlertMetric::InpP75 => percentile_of(
            numbers,
            (INP, INP_N),
            NANOSECONDS_PER_MILLISECOND,
            min_samples,
        ),
        RumAlertMetric::ClsP75 => percentile_of(numbers, (CLS, CLS_N), 1.0, min_samples),
        RumAlertMetric::TtfbP75 => percentile_of(
            numbers,
            (TTFB, TTFB_N),
            NANOSECONDS_PER_MILLISECOND,
            min_samples,
        ),
        RumAlertMetric::CustomTiming | RumAlertMetric::CustomVital => percentile_of(
            numbers,
            (CUSTOM, CUSTOM_N),
            NANOSECONDS_PER_MILLISECOND,
            min_samples,
        ),
        // A query's figure is its own measure (`figure_of`).
        RumAlertMetric::Query => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(pairs: &[(&str, f64)]) -> Numbers {
        Numbers(
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_string(), *value))
                .collect(),
        )
    }

    fn value(metric: RumAlertMetric, numbers: &Numbers, min: u32) -> Option<f64> {
        figure(metric, numbers, min).map(|reading| reading.value)
    }

    #[test]
    fn every_metric_reads_the_group_that_has_its_numbers() {
        for metric in RumAlertMetric::ALL {
            let group = Group::of_metric(metric);

            assert_eq!(group.stream().is_none(), metric.is_about_issues());
            assert_eq!(
                group.measures().is_empty(),
                metric.is_about_issues() || metric.is_custom() || metric == RumAlertMetric::Query
            );
        }

        // A read asks for no more than the aggregate allows in one statement.
        assert!(Group::Views.measures().len() <= 16);
    }

    #[test]
    fn a_count_is_a_figure_even_when_it_is_zero() {
        let none = Numbers::default();

        for metric in RumAlertMetric::ALL
            .into_iter()
            .filter(|metric| metric.counts_zero())
        {
            assert_eq!(value(metric, &none, 0), Some(0.0), "{metric:?}");
        }
    }

    #[test]
    fn shares_are_percent_of_the_rows_and_wait_for_enough_of_them() {
        let views = numbers(&[(VIEWS, 200.0), (VIEWS_STORED, 200.0), (ERRORED, 10.0)]);

        assert_eq!(value(RumAlertMetric::ErrorRate, &views, 100), Some(5.0));
        assert_eq!(value(RumAlertMetric::ErrorRate, &views, 300), None);
        assert_eq!(
            value(RumAlertMetric::ErrorRate, &Numbers::default(), 0),
            None
        );

        let sessions = numbers(&[
            (SESSIONS, 400.0),
            (SESSIONS_STORED, 400.0),
            (CRASHED, 2.0),
            (FRUSTRATED, 40.0),
        ]);

        assert_eq!(
            value(RumAlertMetric::CrashFreeSessions, &sessions, 100),
            Some(99.5)
        );
        assert_eq!(
            value(RumAlertMetric::FrustrationRate, &sessions, 100),
            Some(10.0)
        );

        let calls = numbers(&[
            (REQUESTS, 1_000.0),
            (REQUESTS_STORED, 1_000.0),
            (NO_ANSWER, 5.0),
            (SERVER_ERROR, 15.0),
        ]);

        assert_eq!(
            value(RumAlertMetric::FailedRequestRate, &calls, 200),
            Some(2.0)
        );
    }

    #[test]
    fn a_percentile_is_in_milliseconds_and_waits_for_enough_views_that_had_a_value() {
        let vitals = numbers(&[
            (LCP, 3_000_000_000.0),
            (LCP_N, 80.0),
            (INP, 250_000_000.0),
            (INP_N, 10.0),
            (CLS, 0.12),
            (CLS_N, 80.0),
        ]);

        assert_eq!(value(RumAlertMetric::LcpP75, &vitals, 50), Some(3_000.0));
        assert_eq!(value(RumAlertMetric::InpP75, &vitals, 50), None);
        assert_eq!(value(RumAlertMetric::ClsP75, &vitals, 50), Some(0.12));
        assert_eq!(value(RumAlertMetric::TtfbP75, &vitals, 0), None);

        let reading = figure(RumAlertMetric::LcpP75, &vitals, 50).unwrap();

        assert_eq!(reading.samples, 80);
    }

    #[test]
    fn a_crash_free_share_never_goes_below_zero() {
        let odd = numbers(&[(SESSIONS, 10.0), (SESSIONS_STORED, 10.0), (CRASHED, 12.0)]);

        assert_eq!(value(RumAlertMetric::CrashFreeSessions, &odd, 0), Some(0.0));
    }

    #[test]
    fn a_rule_waits_for_the_rows_that_are_stored_not_for_what_they_stand_for() {
        // Four sessions were kept of a hundred and sixty: they say what four can say.
        let few = numbers(&[
            (SESSIONS, 160.0),
            (SESSIONS_STORED, 4.0),
            (CRASHED, 40.0),
            (FRUSTRATED, 40.0),
        ]);

        assert_eq!(value(RumAlertMetric::CrashFreeSessions, &few, 100), None);
        assert_eq!(value(RumAlertMetric::FrustrationRate, &few, 100), None);
        assert_eq!(
            value(RumAlertMetric::CrashFreeSessions, &few, 4),
            Some(75.0)
        );

        let reading = figure(RumAlertMetric::FrustrationRate, &few, 4).unwrap();

        assert_eq!((reading.value, reading.samples), (25.0, 4));

        // A count is what the rows stand for.
        assert_eq!(value(RumAlertMetric::SessionCount, &few, 100), Some(160.0));
    }
}
