// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The pieces every RUM statement is made of: quoting, and the relation a statement reads from.
//!
//! The rows of a view or a session are written again as the entity changes, so a statement over
//! them must first take the latest revision of each. The relation does that, once, in a common
//! table expression; every statement of the module reads from it.

use std::collections::BTreeSet;

use super::filter::Condition;
use crate::domain::rum::event::{RumStream, Versioned};

/// The column that holds the time of a row as microseconds, in the relation of a versioned stream.
pub(super) const TS: &str = "__ts";

/// The column that says the share of such rows that was kept, in percent.
pub const WEIGHT_COLUMN: &str = "retention.rate";

/// How many rows a row stands for when totals are worked out of the rows that were kept: a row
/// kept with a share of 25 stands for four. A row that says no share, as the rows written before
/// shares were said, stands for one.
pub const WEIGHT: &str = "COALESCE(100.0 / NULLIF(\"retention.rate\", 0.0), 1.0)";

/// A quoted SQL identifier.
pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// A quoted SQL string literal.
pub fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// What a statement is about: a stream, a range of time, and the conditions rows must meet.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub stream: RumStream,
    /// Microseconds; a row counts when `from <= _timestamp < to`.
    pub from: i64,
    pub to: i64,
    pub conditions: &'a [Condition],
}

/// Where a statement reads from, and how it refers to the time and the identity of a row.
#[derive(Debug)]
pub(super) struct Relation {
    /// The `WITH` clause, with its trailing space, for a versioned stream.
    pub with: String,
    pub from: String,
    pub conditions: Vec<String>,
    /// The time of a row as microseconds.
    pub ts: String,
    /// What orders rows of the same time: the entity key, or the event id.
    pub tie: String,
    /// The same two as they stand in an `ORDER BY`.
    pub order_ts: String,
    pub order_tie: String,
}

impl Relation {
    pub fn of(scope: &Scope<'_>, needed: &[&str]) -> Self {
        let time = [
            format!("CAST(_timestamp AS BIGINT) >= {}", scope.from),
            format!("CAST(_timestamp AS BIGINT) < {}", scope.to),
        ];

        let Some(versioned) = scope.stream.versioned() else {
            return Self {
                with: String::new(),
                from: ident(scope.stream.name()),
                conditions: time
                    .into_iter()
                    .chain(
                        scope
                            .conditions
                            .iter()
                            .map(|condition| condition.sql.clone()),
                    )
                    .collect(),
                ts: "CAST(_timestamp AS BIGINT)".into(),
                tie: "COALESCE(\"_event_id\", '')".into(),
                order_ts: "_timestamp".into(),
                order_tie: ident("_event_id"),
            };
        };

        Self::latest(scope, needed, versioned, time)
    }

    fn latest(scope: &Scope<'_>, needed: &[&str], versioned: Versioned, time: [String; 2]) -> Self {
        let mut columns = needed
            .iter()
            .map(|name| (*name).to_string())
            .collect::<BTreeSet<_>>();

        for condition in scope.conditions {
            columns.extend(condition.columns.iter().cloned());
        }

        columns.insert(versioned.key.to_string());
        columns.insert(versioned.revision.to_string());

        let selected = columns
            .iter()
            .map(|name| ident(name))
            .collect::<Vec<_>>()
            .join(", ");

        // What identifies the entity cannot change between its revisions, so it may be asked
        // before the latest is taken, and an index can answer it. Anything else must be asked of
        // the latest alone.
        let pushed = scope
            .conditions
            .iter()
            .filter(|condition| condition.identity);

        let inner = time
            .iter()
            .cloned()
            .chain(pushed.map(|condition| condition.sql.clone()))
            .collect::<Vec<_>>()
            .join(" AND ");

        let key = ident(versioned.key);
        let revision = ident(versioned.revision);

        let with = format!(
            "WITH \"latest\" AS (SELECT {selected}, CAST(_timestamp AS BIGINT) AS \"{TS}\", \
             ROW_NUMBER() OVER (PARTITION BY _timestamp, {key} \
             ORDER BY {revision} DESC NULLS LAST, \"_event_id\" DESC NULLS LAST) AS \"__rn\" \
             FROM {} WHERE {inner}) ",
            ident(scope.stream.name()),
        );

        let conditions = std::iter::once("\"__rn\" = 1".to_string())
            .chain(
                scope
                    .conditions
                    .iter()
                    .filter(|condition| !condition.identity)
                    .map(|condition| condition.sql.clone()),
            )
            .collect();

        Self {
            with,
            from: "\"latest\"".into(),
            conditions,
            ts: ident(TS),
            tie: key.clone(),
            order_ts: ident(TS),
            order_tie: key,
        }
    }

    pub fn where_clause(&self, extra: &[String]) -> String {
        self.conditions
            .iter()
            .chain(extra)
            .cloned()
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_and_literals_are_quoted() {
        assert_eq!(ident("view.id"), "\"view.id\"");
        assert_eq!(ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(literal("it's"), "'it''s'");
    }

    #[test]
    fn a_stream_of_events_is_read_as_it_is() {
        let condition = Condition::raw("\"env\" = 'prod'", &["env"], false);
        let scope = Scope {
            stream: RumStream::Resources,
            from: 10,
            to: 20,
            conditions: std::slice::from_ref(&condition),
        };

        let relation = Relation::of(&scope, &["resource.url"]);

        assert!(relation.with.is_empty());
        assert_eq!(relation.from, "\"rum_resources\"");
        assert_eq!(
            relation.where_clause(&[]),
            "CAST(_timestamp AS BIGINT) >= 10 AND CAST(_timestamp AS BIGINT) < 20 AND \"env\" = 'prod'"
        );
        assert_eq!(relation.order_ts, "_timestamp");
    }

    #[test]
    fn a_versioned_stream_is_read_through_its_latest_revisions() {
        let by_id = Condition::raw("\"view.id\" = 'v-1'", &["view.id"], true);
        let by_state = Condition::raw("\"view.is_active\" = TRUE", &["view.is_active"], false);
        let conditions = [by_id, by_state];

        let scope = Scope {
            stream: RumStream::Views,
            from: 10,
            to: 20,
            conditions: &conditions,
        };

        let relation = Relation::of(&scope, &["view.url"]);

        assert!(relation.with.starts_with("WITH \"latest\" AS (SELECT "));
        assert!(relation.with.contains("\"view.url\""));
        assert!(relation.with.contains("\"view.is_active\""));
        assert!(relation.with.contains("\"view.revision\""));
        assert!(relation.with.contains(
            "ROW_NUMBER() OVER (PARTITION BY _timestamp, \"view.id\" ORDER BY \"view.revision\" DESC NULLS LAST"
        ));

        // The identity is asked before the latest is taken; the state after it.
        let inner = relation
            .with
            .split(" FROM \"rum_views\" WHERE ")
            .nth(1)
            .unwrap();

        assert!(inner.contains("\"view.id\" = 'v-1'"));
        assert!(!inner.contains("is_active\" = TRUE"));
        assert_eq!(
            relation.where_clause(&[]),
            "\"__rn\" = 1 AND \"view.is_active\" = TRUE"
        );
        assert_eq!(relation.from, "\"latest\"");
        assert_eq!(relation.order_tie, "\"view.id\"");
    }
}
