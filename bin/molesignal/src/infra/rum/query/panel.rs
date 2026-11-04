// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements of the dashboard panels that draw what the explorer draws.
//!
//! The explorer asks for its numbers over a range; a dashboard runs one statement over whatever
//! range its reader picks, and the engine applies that range to the scan. So a panel's statement
//! names no time: it reads the newest revision of a view or a session through the current-version
//! views (`rum_views_latest`), and a chart over time cuts the range into bars of the interval the
//! dashboard says. The conditions are the explorer's, the sessions no person ran left out as there.
//!
//! The aggregate charts are written by [`super::Plan`]; the distribution is written here.

use serde::Serialize;

use super::{
    filter::Condition,
    member::Column,
    sql::{ident, literal},
};
use crate::{
    domain::{rum::event::RumStream, stream::FieldType},
    infra::query::current_views::CURRENT_VIEWS,
    shared::{Error, Result},
};

/// How many bars a distribution is drawn in, as the explorer draws it.
const DISTRIBUTION_BARS: usize = 40;

/// The names a dashboard reads as the time of a row (`TIME_FIELD_NAMES` of the web's data frames):
/// no other column may be called one of them.
const TIME_NAMES: [&str; 5] = ["_timestamp", "timestamp", "time", "ts", "__time__"];

/// The longest name a column of a panel is given.
const MAX_NAME_CHARS: usize = 64;

/// What a column of a panel's answer holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelRole {
    /// Where the bar of a chart over time begins.
    Time,
    /// What the rows are grouped by, or the stretch of values a bar of a distribution is of.
    Group,
    /// A number.
    Measure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PanelColumn {
    pub name: String,
    pub role: PanelRole,
    /// The unit a number is in once it is shown: a duration is in milliseconds, not in the
    /// nanoseconds the SDK sent. `none` for a time or a group.
    pub unit: &'static str,
}

/// A statement a panel runs, and the columns it answers with, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PanelStatement {
    pub sql: String,
    pub columns: Vec<PanelColumn>,
}

/// The interval a chart over time is cut into: a variable of the dashboard, as `${interval}`, or
/// a length such as `15 minutes`. Nothing else reaches the statement.
pub fn interval(text: &str) -> Result<&str> {
    let text = text.trim();

    let variable = text
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
        .is_some_and(|name| {
            name.chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
                && name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        });

    let length = text.split_once(' ').is_some_and(|(count, unit)| {
        (1..=6).contains(&count.len())
            && count.chars().all(|digit| digit.is_ascii_digit())
            && !count.starts_with('0')
            && ["second", "minute", "hour", "day", "week"]
                .iter()
                .any(|known| unit == *known || unit.strip_suffix('s') == Some(*known))
    });

    if variable || length {
        Ok(text)
    } else {
        Err(Error::invalid(
            "interval is a dashboard variable such as ${interval} or a length such as 15 minutes",
        ))
    }
}

/// What a statement of a panel reads: the newest revisions of a versioned stream, else the stream.
pub(super) fn relation(stream: RumStream) -> String {
    let name = CURRENT_VIEWS
        .iter()
        .find(|view| view.stream == stream)
        .map_or(stream.name(), |view| view.name);

    ident(name)
}

/// The conditions as a `WHERE` clause, with its leading space; nothing when there are none.
pub(super) fn where_clause(conditions: &[Condition], extra: &[String]) -> String {
    let all = conditions
        .iter()
        .map(|condition| condition.sql.clone())
        .chain(extra.iter().cloned())
        .collect::<Vec<_>>();

    if all.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", all.join(" AND "))
    }
}

/// A number as it is shown, and the unit it is then in: a duration in milliseconds.
pub(super) fn shown(sql: &str, unit: &'static str) -> (String, &'static str) {
    if unit == "ns" {
        (format!("({sql}) / 1e6"), "ms")
    } else {
        (sql.to_string(), unit)
    }
}

/// The names the columns of a panel are given: the one asked for, else the default, each told
/// apart from the others and from the names a dashboard reads as a time.
#[derive(Debug, Default)]
pub(super) struct Names {
    taken: Vec<String>,
}

impl Names {
    pub(super) fn name(&mut self, asked: Option<&str>, default: &str) -> String {
        let wanted = asked
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(default);

        let base = wanted.chars().take(MAX_NAME_CHARS).collect::<String>();

        let clashes = |name: &str, taken: &[String]| {
            TIME_NAMES
                .iter()
                .any(|time| time.eq_ignore_ascii_case(name))
                || taken.iter().any(|other| other.eq_ignore_ascii_case(name))
        };

        let mut name = base.clone();
        let mut count = 1;

        while clashes(&name, &self.taken) {
            count += 1;
            name = format!("{base} ({count})");
        }

        self.taken.push(name.clone());

        name
    }
}

/// The statement of a distribution of `column`: bars of one width from the smallest value to the
/// 99th percentile, the last counting what lies past it, as the explorer draws one, but in a
/// single statement over the range the dashboard looks at. A bar is named by where it begins, in
/// the unit the value is shown in; a whole number is cut in whole steps.
pub fn distribution_sql(
    stream: RumStream,
    conditions: &[Condition],
    column: &Column,
    (bar_name, count_name): (Option<&str>, Option<&str>),
) -> Result<PanelStatement> {
    if !column.is_numeric() {
        return Err(Error::invalid(format!(
            "`{}` is not a number",
            column.name()
        )));
    }

    let value = column.number();
    let mut names = Names::default();
    let bar = names.name(bar_name, column.name());
    let count = names.name(count_name, "count");
    let (start, unit) = shown("\"__low\" + \"__bar\" * \"__width\"", column.unit());
    let last = DISTRIBUTION_BARS - 1;

    let width = if column.data_type() == FieldType::Int64 {
        format!("GREATEST(CEIL((\"__high\" - \"__low\") / {DISTRIBUTION_BARS}.0), 1.0)")
    } else {
        format!(
            "CASE WHEN \"__high\" > \"__low\" THEN (\"__high\" - \"__low\") / {DISTRIBUTION_BARS}.0 ELSE 1.0 END"
        )
    };

    // The width a bar has in the unit it is shown in decides how many decimals tell bars apart.
    let (shown_width, _) = shown("\"__width\"", column.unit());

    let label = format!(
        "CASE WHEN {shown_width} >= 10 THEN CAST(CAST(ROUND({start}) AS BIGINT) AS VARCHAR) \
         WHEN {shown_width} >= 1 THEN CAST(ROUND({start}, 1) AS VARCHAR) \
         WHEN {shown_width} >= 0.1 THEN CAST(ROUND({start}, 2) AS VARCHAR) \
         WHEN {shown_width} >= 0.01 THEN CAST(ROUND({start}, 3) AS VARCHAR) \
         ELSE CAST(ROUND({start}, 5) AS VARCHAR) END{}",
        suffix(unit)
    );

    let sql = format!(
        "WITH \"__values\" AS (SELECT CAST({value} AS DOUBLE) AS \"__v\" FROM {}{}), \
         \"__bounds\" AS (SELECT MIN(\"__v\") AS \"__low\", \
         approx_percentile_cont(0.99) WITHIN GROUP (ORDER BY \"__v\") AS \"__high\" FROM \"__values\"), \
         \"__plan\" AS (SELECT \"__low\", {width} AS \"__width\" FROM \"__bounds\"), \
         \"__bars\" AS (SELECT LEAST({last}, GREATEST(0, CAST(FLOOR((\"__v\" - \"__low\") / \"__width\") AS BIGINT))) \
         AS \"__bar\", \"__low\", \"__width\" FROM \"__values\" CROSS JOIN \"__plan\") \
         SELECT {label} AS {}, COUNT(*) AS {} FROM \"__bars\" \
         GROUP BY \"__bar\", \"__low\", \"__width\" ORDER BY \"__bar\" ASC",
        relation(stream),
        where_clause(conditions, &[format!("{value} IS NOT NULL")]),
        ident(&bar),
        ident(&count),
    );

    Ok(PanelStatement {
        sql,
        columns: vec![
            PanelColumn {
                name: bar,
                role: PanelRole::Group,
                unit: "none",
            },
            PanelColumn {
                name: count,
                role: PanelRole::Measure,
                unit: "count",
            },
        ],
    })
}

/// What follows the number that begins a bar, so the bar says what it is of.
fn suffix(unit: &str) -> String {
    let said = match unit {
        "ms" => " ms",
        "bytes" => " B",
        "percent" => "%",
        "px" => " px",
        "per_second" => "/s",
        _ => return String::new(),
    };

    format!(" || {}", literal(said))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::rum::query::member::resolved;

    #[test]
    fn an_interval_is_a_variable_or_a_length() {
        assert_eq!(interval("${interval}").unwrap(), "${interval}");
        assert_eq!(interval(" 15 minutes ").unwrap(), "15 minutes");
        assert_eq!(interval("1 hour").unwrap(), "1 hour");
        assert_eq!(interval("7 days").unwrap(), "7 days");

        for refused in [
            "",
            "15",
            "minutes",
            "015 minutes",
            "1 fortnight",
            "${}",
            "${a b}",
            "${1x}",
            "1 hour'; DROP TABLE x; --",
            "1 hour' || '",
        ] {
            assert!(interval(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_versioned_stream_is_read_through_its_newest_revisions() {
        assert_eq!(relation(RumStream::Views), "\"rum_views_latest\"");
        assert_eq!(relation(RumStream::Sessions), "\"rum_sessions_latest\"");
        assert_eq!(relation(RumStream::Errors), "\"rum_errors\"");
    }

    #[test]
    fn names_are_told_apart_from_each_other_and_from_a_time() {
        let mut names = Names::default();

        assert_eq!(names.name(Some("Time"), "count"), "Time (2)");
        assert_eq!(names.name(None, "count"), "count");
        assert_eq!(names.name(Some(" count "), "x"), "count (2)");
        assert_eq!(names.name(Some(""), "browser.name"), "browser.name");
        assert_eq!(names.name(Some("ts"), "x"), "ts (2)");

        let long = "x".repeat(80);

        assert_eq!(names.name(Some(&long), "x").chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn a_duration_is_shown_in_milliseconds() {
        assert_eq!(
            shown("MAX(\"view.loading_time\")", "ns"),
            ("(MAX(\"view.loading_time\")) / 1e6".to_string(), "ms")
        );
        assert_eq!(
            shown("COUNT(*)", "count"),
            ("COUNT(*)".to_string(), "count")
        );
    }

    #[test]
    fn a_distribution_is_one_statement_that_names_no_time() {
        let column = resolved(RumStream::Views, "view.loading_time").unwrap();
        let statement = distribution_sql(
            RumStream::Views,
            &[],
            &column,
            (Some("加载时间"), Some("视图")),
        )
        .unwrap();

        assert!(
            statement.sql.contains("FROM \"rum_views_latest\" WHERE"),
            "{}",
            statement.sql
        );
        assert!(!statement.sql.contains("_timestamp"), "{}", statement.sql);
        assert!(statement.sql.contains("|| ' ms'"), "{}", statement.sql);
        assert_eq!(
            statement
                .columns
                .iter()
                .map(|column| (column.name.as_str(), column.role))
                .collect::<Vec<_>>(),
            [("加载时间", PanelRole::Group), ("视图", PanelRole::Measure)]
        );

        let text = resolved(RumStream::Views, "view.url_path").unwrap();

        assert!(distribution_sql(RumStream::Views, &[], &text, (None, None)).is_err());
    }
}
