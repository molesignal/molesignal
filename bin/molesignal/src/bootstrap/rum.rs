// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What RUM needs from the other runtimes.
//!
//! A node that meets a session it knows nothing of rebuilds it from the rows written for it. Those
//! are read back through the query service, which is built after the intake that merges sessions,
//! so the history reaches it through a slot that is filled once it exists.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use serde_json::{Map, Value};

use crate::{
    app::query::QueryService,
    domain::{
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        rum::event::RumStream,
        stream::StreamType,
    },
    infra::rum::{query::session_rows_sql, session_state::SessionHistory},
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// How many of the newest rows of a session are read back. The rows of one node may know less
/// than those of another, so more than one is read.
const HISTORY_ROWS: usize = 8;

/// Where the history finds the query service once it runs.
#[derive(Default)]
pub(super) struct QuerySlot(OnceLock<Arc<QueryService>>);

impl QuerySlot {
    pub(super) fn fill(&self, query: Arc<QueryService>) {
        let _ = self.0.set(query);
    }

    fn query(&self) -> Result<&Arc<QueryService>> {
        self.0
            .get()
            .ok_or_else(|| Error::unavailable("the query service is not running yet"))
    }
}

/// The rows written for a session, read as SQL from `rum_sessions`.
pub(super) struct SqlSessionHistory {
    query: Arc<QuerySlot>,
}

impl SqlSessionHistory {
    pub(super) fn new(query: Arc<QuerySlot>) -> Self {
        Self { query }
    }
}

#[async_trait]
impl SessionHistory for SqlSessionHistory {
    async fn rows(
        &self,
        org_id: &Id,
        session_id: &str,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<Map<String, Value>>> {
        let query = self.query.query()?;

        let from = from_ms.saturating_mul(1000);
        let to = to_ms.saturating_mul(1000);

        // The state is derived from what was written, so it is read as it was written: the
        // values of a masked field are not the reader's to hide here.
        let result = query
            .run_raw(QueryRequest {
                org_id: org_id.clone(),
                language: QueryLanguage::Sql,
                statement: session_rows_sql(session_id, from, to, HISTORY_ROWS),
                time_range: TimeRange::new(TimestampMicros(from), TimestampMicros(to)),
                stream: Some(StreamHint {
                    name: RumStream::Sessions.name().to_string(),
                    stream_type: StreamType::LOGS,
                }),
                limit: Some(HISTORY_ROWS),
                step: None,
                federation_clusters: Vec::new(),
            })
            .await;

        match result {
            Ok(result) => Ok(rows_of(result)),
            // Nothing was ever written to the stream.
            Err(Error::NotFound(_)) => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }
}

fn rows_of(result: QueryResult) -> Vec<Map<String, Value>> {
    let QueryResult { columns, rows, .. } = result;

    rows.into_iter()
        .map(|row| columns.iter().cloned().zip(row).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn rows_come_back_as_maps_of_their_columns() {
        let rows = rows_of(QueryResult {
            columns: vec!["session.id".into(), "session.revision".into()],
            rows: vec![vec![json!("s-1"), json!(7)], vec![json!("s-1"), json!(6)]],
            scanned_rows: 2,
            took_ms: 1,
            federation: None,
        });

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["session.revision"], 7);
        assert_eq!(rows[1]["session.id"], "s-1");
    }

    #[tokio::test]
    async fn history_is_unavailable_until_the_query_service_runs() {
        let history = SqlSessionHistory::new(Arc::new(QuerySlot::default()));

        assert!(history.rows(&Id::new(), "s-1", 0, 1).await.is_err());
    }
}
