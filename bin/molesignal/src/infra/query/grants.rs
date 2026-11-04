// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the query in progress may read beyond the organization's ordinary streams: the audit
//! trail (`_audit`) only for a caller with `audit.read`.
//!
//! The grants travel with the task that runs a query, not with the query: a request from a caller
//! carries the caller's ([`scope`], set by the HTTP layer and Flight SQL), a query forwarded to a
//! querier carries them in a header ([`HEADER`]), and a scheduled pipeline that reads `_audit` —
//! saved only by someone with `audit.read` — runs with them. Anything else reads with none: a
//! background task that does not set them cannot read `_audit`, whoever started it.
//! [`super::planner::ensure_stream_in_org`] refuses the read.

use std::future::Future;

use crate::domain::{query::QueryRequest, stream::AUDIT_STREAM};

/// The permission that lets a caller read the audit trail.
pub const AUDIT_READ: &str = "audit.read";

/// The gRPC metadata a query forwarded to a querier carries its grants in.
pub const HEADER: &str = "x-molesignal-read-grants";

tokio::task_local! {
    static GRANTS: ReadGrants;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadGrants {
    /// The audit trail.
    pub audit: bool,
}

impl ReadGrants {
    /// The grants of a caller who holds `audit.read` when `audit` says so.
    pub fn of_caller(audit: bool) -> Self {
        Self { audit }
    }

    /// The grants a forwarded query's [`HEADER`] names.
    pub fn from_header(value: Option<&str>) -> Self {
        Self {
            audit: value.is_some_and(|value| value.split(',').any(|grant| grant.trim() == "audit")),
        }
    }

    /// The [`HEADER`] value that carries these grants; `None` when there are none.
    pub fn header_value(self) -> Option<&'static str> {
        self.audit.then_some("audit")
    }

    /// A key for results cached for a caller of `role`: callers who read the audit trail and
    /// callers who do not never share one.
    pub fn cache_role(self, role: &str) -> String {
        if self.audit {
            format!("{role}+audit")
        } else {
            role.to_string()
        }
    }
}

/// Runs `future` with these grants: the queries it makes read what they allow.
pub async fn scope<F: Future>(grants: ReadGrants, future: F) -> F::Output {
    GRANTS.scope(grants, future).await
}

/// The grants of the query in progress; none outside a [`scope`].
pub fn current() -> ReadGrants {
    GRANTS.try_with(|grants| *grants).unwrap_or_default()
}

/// Whether the query in progress may read `stream`.
pub fn may_read(stream: &str) -> bool {
    stream != AUDIT_STREAM || current().audit
}

/// Whether `request` names the audit trail, as its stream or as a table of its SQL. A query run
/// later in the background has no caller's grants to read it with.
pub fn names_audit(request: &QueryRequest) -> bool {
    request
        .stream
        .as_ref()
        .is_some_and(|stream| stream.name == AUDIT_STREAM)
        || statement_names_audit(&request.statement)
}

/// Whether a SQL statement reads the audit trail. Anything else, PromQL too, names no table here.
pub fn statement_names_audit(statement: &str) -> bool {
    super::parser::statement_names_table(statement, AUDIT_STREAM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_audit_trail_is_read_only_with_the_grant() {
        assert!(!may_read(AUDIT_STREAM));
        assert!(may_read("app_logs"));

        scope(ReadGrants::of_caller(true), async {
            assert!(may_read(AUDIT_STREAM));
        })
        .await;

        scope(ReadGrants::of_caller(false), async {
            assert!(!may_read(AUDIT_STREAM));
        })
        .await;

        // A task spawned inside a scope does not inherit it.
        scope(ReadGrants::of_caller(true), async {
            let spawned = tokio::spawn(async { may_read(AUDIT_STREAM) });
            assert!(!spawned.await.unwrap());
        })
        .await;
    }

    #[test]
    fn a_query_names_the_audit_trail_by_its_stream_or_its_sql() {
        use crate::{
            domain::{
                query::{QueryLanguage, StreamHint},
                stream::StreamType,
            },
            shared::{
                ids::Id,
                time::{TimeRange, TimestampMicros},
            },
        };

        let query = |stream: &str, statement: &str| QueryRequest {
            org_id: Id::from_string("org"),
            language: QueryLanguage::Sql,
            statement: statement.into(),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            stream: Some(StreamHint {
                name: stream.into(),
                stream_type: StreamType::LOGS,
            }),
            limit: None,
            step: None,
            federation_clusters: vec![],
        };

        assert!(names_audit(&query("_audit", "SELECT * FROM \"_audit\"")));
        assert!(names_audit(&query(
            "app_logs",
            "SELECT * FROM app_logs JOIN _audit ON app_logs.user = _audit.actor_id"
        )));
        assert!(!names_audit(&query("app_logs", "SELECT * FROM app_logs")));
    }

    #[test]
    fn grants_cross_to_a_querier_in_a_header() {
        let granted = ReadGrants::of_caller(true);

        assert_eq!(ReadGrants::from_header(granted.header_value()), granted);
        assert_eq!(ReadGrants::from_header(None), ReadGrants::default());
        assert_eq!(
            ReadGrants::from_header(Some("other")),
            ReadGrants::default()
        );
        assert_eq!(granted.cache_role("admin"), "admin+audit");
        assert_eq!(ReadGrants::default().cache_role("admin"), "admin");
    }
}
