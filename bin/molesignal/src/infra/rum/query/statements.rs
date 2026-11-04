// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The statements the explorer and the detail pages run.

use super::{
    columns::trouble,
    member::Column,
    sql::{Relation, Scope, ident, literal},
};
use crate::{
    domain::{
        rum::event::{RumStream, Versioned},
        stream::FieldType,
    },
    shared::{
        Error, Result,
        cursor::{CursorDirection, CursorSortDirection, CursorValue, lexicographic_seek},
    },
};

/// The result columns that carry what the page needs to order and cut a list: the time of the row
/// in microseconds, and the value that tells rows of the same time apart.
pub const TS_COLUMN: &str = "__ts";
pub const ID_COLUMN: &str = "__id";

pub const FACET_VALUE: &str = "__value";
pub const FACET_COUNT: &str = "__count";

pub const KEY_COLUMN: &str = "__key";
pub const KEY_COUNT: &str = "__sessions";

pub const BUCKET_COLUMN: &str = "__histogram_bucket";
pub const TOTAL_COLUMN: &str = "__histogram_total";
pub const TROUBLE_COLUMN: &str = "__histogram_errors";

/// Where the next page of a list begins.
#[derive(Debug, Clone)]
pub struct Seek {
    pub direction: CursorDirection,
    pub ts: i64,
    pub tie: String,
}

pub struct ListRequest<'a> {
    pub scope: Scope<'a>,
    pub columns: &'a [&'a str],
    pub limit: usize,
    pub seek: Option<Seek>,
    /// A timeline reads from the start; an explorer from the newest row.
    pub oldest_first: bool,
}

/// One page of rows, newest first unless asked otherwise, with `__ts` and `__id` to cut the next
/// page at. A page that precedes the cursor is read in the other direction and turned round by the
/// caller.
pub fn list_sql(request: &ListRequest<'_>) -> String {
    let relation = Relation::of(&request.scope, request.columns);

    let sort = if request.oldest_first {
        CursorSortDirection::Asc
    } else {
        CursorSortDirection::Desc
    };

    let seek = request.seek.as_ref().map(|seek| {
        lexicographic_seek(
            &[
                (relation.ts.as_str(), CursorValue::Integer(seek.ts), sort),
                (
                    relation.tie.as_str(),
                    CursorValue::Text(seek.tie.clone()),
                    sort,
                ),
            ],
            seek.direction,
            literal,
        )
    });

    let reversed = request
        .seek
        .as_ref()
        .is_some_and(|seek| seek.direction == CursorDirection::Before);

    let order = if request.oldest_first != reversed {
        "ASC"
    } else {
        "DESC"
    };

    let mut selected = request
        .columns
        .iter()
        .map(|name| ident(name))
        .collect::<Vec<_>>();

    selected.push(format!("{} AS \"{TS_COLUMN}\"", relation.ts));
    selected.push(format!("{} AS \"{ID_COLUMN}\"", relation.tie));

    let extra = seek.into_iter().collect::<Vec<_>>();

    format!(
        "{}SELECT {} FROM {} WHERE {} ORDER BY {} {order} NULLS LAST, {} {order} NULLS LAST LIMIT {}",
        relation.with,
        selected.join(", "),
        relation.from,
        relation.where_clause(&extra),
        relation.order_ts,
        relation.order_tie,
        request.limit,
    )
}

/// The most frequent values of a column and how many rows have each.
pub fn facet_sql(scope: &Scope<'_>, column: &Column, limit: usize) -> Result<String> {
    if column.data_type() == FieldType::Json {
        return Err(Error::invalid(format!(
            "`{}` has no values to count",
            column.name()
        )));
    }

    let relation = Relation::of(scope, &[column.stored_name()]);
    let name = column.value();

    let mut extra = vec![format!("{name} IS NOT NULL")];

    if column.data_type() == FieldType::Utf8 {
        extra.push(format!("{name} <> ''"));
    }

    Ok(format!(
        "{}SELECT CAST({name} AS VARCHAR) AS \"{FACET_VALUE}\", COUNT(*) AS \"{FACET_COUNT}\" \
         FROM {} WHERE {} GROUP BY 1 ORDER BY 2 DESC, 1 ASC LIMIT {limit}",
        relation.with,
        relation.from,
        relation.where_clause(&extra),
    ))
}

/// The keys found in a JSON column and how many sessions had each, most first.
pub fn keys_sql(scope: &Scope<'_>, column: &str, limit: usize) -> String {
    let relation = Relation::of(scope, &[column, "session.id"]);
    let name = ident(column);

    let extra = [format!("{name} IS NOT NULL")];

    format!(
        "{}SELECT \"{KEY_COLUMN}\", COUNT(DISTINCT \"session.id\") AS \"{KEY_COUNT}\" FROM (\
         SELECT unnest(json_keys({name})) AS \"{KEY_COLUMN}\", \"session.id\" FROM {} WHERE {}) \
         GROUP BY 1 ORDER BY 2 DESC, 1 ASC LIMIT {limit}",
        relation.with,
        relation.from,
        relation.where_clause(&extra),
    )
}

/// How many rows fall in each bar of the histogram, and how many of those are the troubled ones
/// (see [`trouble`]). A bar is `bucket` microseconds long and the first begins at `origin`.
pub fn histogram_sql(scope: &Scope<'_>, origin: i64, bucket: i64) -> String {
    let apart = trouble(scope.stream);
    let needed = apart.map_or(&[][..], |(_, columns)| columns);
    let relation = Relation::of(scope, needed);

    let counted = apart
        .map(|(condition, _)| {
            format!(", SUM(CASE WHEN {condition} THEN 1 ELSE 0 END) AS \"{TROUBLE_COLUMN}\"")
        })
        .unwrap_or_default();

    format!(
        "{}SELECT ({} - {origin}) / {bucket} AS \"{BUCKET_COLUMN}\", COUNT(*) AS \"{TOTAL_COLUMN}\"{counted} \
         FROM {} WHERE {} GROUP BY 1",
        relation.with,
        relation.ts,
        relation.from,
        relation.where_clause(&[]),
    )
}

/// The rows with the largest values of a column that has one, largest first, with the time of
/// each as `__ts`: what the worst of a figure looks like.
pub fn ranked_sql(scope: &Scope<'_>, columns: &[&str], rank_by: &str, limit: usize) -> String {
    let needed = columns
        .iter()
        .copied()
        .chain(std::iter::once(rank_by))
        .collect::<Vec<_>>();
    let relation = Relation::of(scope, &needed);
    let ranked = ident(rank_by);

    let mut selected = columns.iter().map(|name| ident(name)).collect::<Vec<_>>();

    selected.push(format!("{} AS \"{TS_COLUMN}\"", relation.ts));

    let extra = [format!("{ranked} IS NOT NULL")];

    format!(
        "{}SELECT {} FROM {} WHERE {} ORDER BY {ranked} DESC NULLS LAST, {} DESC LIMIT {limit}",
        relation.with,
        selected.join(", "),
        relation.from,
        relation.where_clause(&extra),
        relation.order_tie,
    )
}

/// The newest rows written for one session, whatever their revision, for the merger to rebuild a
/// session it has no memory of. The rows of a session are dated at its start.
pub fn session_rows_sql(session_id: &str, from: i64, to: i64, limit: usize) -> String {
    let versioned = RumStream::Sessions.versioned().unwrap_or(Versioned {
        key: "session.id",
        revision: "session.revision",
    });

    format!(
        "SELECT * FROM {} WHERE CAST(_timestamp AS BIGINT) >= {from} \
         AND CAST(_timestamp AS BIGINT) < {to} AND {} = {} \
         ORDER BY {} DESC NULLS LAST LIMIT {limit}",
        ident(RumStream::Sessions.name()),
        ident(versioned.key),
        literal(session_id),
        ident(versioned.revision),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::rum::event::field,
        infra::rum::query::{columns::all_columns, filter::Condition},
    };

    fn scope<'a>(stream: RumStream, conditions: &'a [Condition]) -> Scope<'a> {
        Scope {
            stream,
            from: 100,
            to: 200,
            conditions,
        }
    }

    #[test]
    fn a_list_of_events_is_ordered_by_time_and_event_id() {
        let conditions = [Condition::equals("env", "prod")];

        let sql = list_sql(&ListRequest {
            scope: scope(RumStream::Errors, &conditions),
            columns: &["error.message", "session.id"],
            limit: 51,
            seek: None,
            oldest_first: false,
        });

        assert_eq!(
            sql,
            "SELECT \"error.message\", \"session.id\", CAST(_timestamp AS BIGINT) AS \"__ts\", \
             COALESCE(\"_event_id\", '') AS \"__id\" FROM \"rum_errors\" WHERE \
             CAST(_timestamp AS BIGINT) >= 100 AND CAST(_timestamp AS BIGINT) < 200 AND \"env\" = 'prod' \
             ORDER BY _timestamp DESC NULLS LAST, \"_event_id\" DESC NULLS LAST LIMIT 51"
        );
    }

    #[test]
    fn the_next_page_of_a_list_begins_after_the_cursor() {
        let sql = list_sql(&ListRequest {
            scope: scope(RumStream::Resources, &[]),
            columns: &["resource.url"],
            limit: 11,
            seek: Some(Seek {
                direction: CursorDirection::After,
                ts: 150,
                tie: "b:2".into(),
            }),
            oldest_first: false,
        });

        assert!(sql.contains(
            "((CAST(_timestamp AS BIGINT) < 150) OR (CAST(_timestamp AS BIGINT) = 150 AND COALESCE(\"_event_id\", '') < 'b:2'))"
        ));
        assert!(sql.ends_with("DESC NULLS LAST LIMIT 11"));
    }

    #[test]
    fn the_page_before_a_cursor_is_read_the_other_way_round() {
        let sql = list_sql(&ListRequest {
            scope: scope(RumStream::Resources, &[]),
            columns: &[],
            limit: 11,
            seek: Some(Seek {
                direction: CursorDirection::Before,
                ts: 150,
                tie: "b:2".into(),
            }),
            oldest_first: false,
        });

        assert!(sql.contains("CAST(_timestamp AS BIGINT) > 150"));
        assert!(sql.contains("ORDER BY _timestamp ASC NULLS LAST, \"_event_id\" ASC NULLS LAST"));
    }

    #[test]
    fn a_timeline_reads_from_the_start() {
        let conditions = [Condition::equals("session.id", "s-1")];

        let sql = list_sql(&ListRequest {
            scope: scope(RumStream::Actions, &conditions),
            columns: &["action.type"],
            limit: 500,
            seek: None,
            oldest_first: true,
        });

        assert!(sql.contains("ORDER BY _timestamp ASC NULLS LAST"));
        assert!(sql.contains("\"session.id\" = 's-1'"));
    }

    #[test]
    fn a_list_of_views_is_read_from_their_latest_revisions_and_cut_by_view_id() {
        let conditions = [
            Condition::equals("session.id", "s-1"),
            Condition::raw("\"view.error.count\" > 0", &["view.error.count"], false),
        ];

        let sql = list_sql(&ListRequest {
            scope: scope(RumStream::Views, &conditions),
            columns: &["view.url_path"],
            limit: 21,
            seek: Some(Seek {
                direction: CursorDirection::After,
                ts: 150,
                tie: "v-9".into(),
            }),
            oldest_first: false,
        });

        assert!(sql.starts_with("WITH \"latest\" AS (SELECT "));
        assert!(sql.contains(
            "SELECT \"view.url_path\", \"__ts\" AS \"__ts\", \"view.id\" AS \"__id\" FROM \"latest\" WHERE \"__rn\" = 1 AND \"view.error.count\" > 0 AND ((\"__ts\" < 150) OR (\"__ts\" = 150 AND \"view.id\" < 'v-9'))"
        ));
        assert!(
            sql.ends_with(
                "ORDER BY \"__ts\" DESC NULLS LAST, \"view.id\" DESC NULLS LAST LIMIT 21"
            )
        );
    }

    #[test]
    fn a_facet_counts_the_values_that_are_there() {
        let column = Column::Stored(field(RumStream::Resources, "resource.type").unwrap());
        let sql = facet_sql(&scope(RumStream::Resources, &[]), &column, 10).unwrap();

        assert!(sql.starts_with("SELECT CAST(\"resource.type\" AS VARCHAR) AS \"__value\", COUNT(*) AS \"__count\" FROM \"rum_resources\" WHERE "));
        assert!(sql.contains("\"resource.type\" IS NOT NULL AND \"resource.type\" <> ''"));
        assert!(sql.ends_with("GROUP BY 1 ORDER BY 2 DESC, 1 ASC LIMIT 10"));

        let json = Column::Stored(field(RumStream::Views, "context").unwrap());

        assert!(facet_sql(&scope(RumStream::Views, &[]), &json, 10).is_err());
    }

    #[test]
    fn the_histogram_counts_the_troubled_apart() {
        let sql = histogram_sql(&scope(RumStream::Resources, &[]), 100, 50);

        assert!(sql.starts_with(
            "SELECT (CAST(_timestamp AS BIGINT) - 100) / 50 AS \"__histogram_bucket\", COUNT(*) AS \"__histogram_total\", SUM(CASE WHEN (\"resource.status_code\" >= 400 OR \"resource.status_code\" = 0) THEN 1 ELSE 0 END) AS \"__histogram_errors\" FROM"
        ));
        assert!(sql.ends_with("GROUP BY 1"));

        let plain = histogram_sql(&scope(RumStream::Errors, &[]), 100, 50);

        assert!(!plain.contains("__histogram_errors"));
    }

    #[test]
    fn the_histogram_of_sessions_counts_each_session_once() {
        let sql = histogram_sql(&scope(RumStream::Sessions, &[]), 0, 1000);

        assert!(sql.starts_with("WITH \"latest\" AS ("));
        assert!(sql.contains("(\"__ts\" - 0) / 1000 AS \"__histogram_bucket\""));
        assert!(sql.contains("COALESCE(\"session.error.count\", 0) > 0"));
        assert!(sql.contains("\"session.error.count\""));
    }

    #[test]
    fn an_entity_is_a_list_of_one_with_every_column() {
        let conditions = [Condition::equals("view.id", "v-1")];

        let sql = list_sql(&ListRequest {
            scope: scope(RumStream::Views, &conditions),
            columns: &all_columns(RumStream::Views),
            limit: 1,
            seek: None,
            oldest_first: false,
        });

        // Every column of the stream is read from the latest revision, not just the key.
        assert!(sql.contains("\"view.performance.lcp.timestamp\""));
        assert!(sql.contains("\"context\""));
        assert!(sql.contains("FROM \"latest\" WHERE \"__rn\" = 1"));
        assert!(sql.ends_with("LIMIT 1"));
    }

    #[test]
    fn the_rows_of_a_session_are_read_as_they_were_written() {
        assert_eq!(
            session_rows_sql("s-1'", 100, 200, 8),
            "SELECT * FROM \"rum_sessions\" WHERE CAST(_timestamp AS BIGINT) >= 100 \
             AND CAST(_timestamp AS BIGINT) < 200 AND \"session.id\" = 's-1''' \
             ORDER BY \"session.revision\" DESC NULLS LAST LIMIT 8"
        );
    }
}
