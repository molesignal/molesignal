// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The APM figures alert rules watch, read by the evaluator in place of a
//! query. APM is built after the evaluator, so the evaluator holds a slot
//! the bootstrap fills once APM's query service exists.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use serde_json::json;

use crate::{
    domain::{
        apm::ApmAlertQuery,
        query::{QueryResult, StreamHint},
        stream::StreamType,
    },
    shared::{Error, Result, ids::Id, time::TimeRange},
};

/// A figure read for a rule, and what it is about.
#[derive(Debug, Clone, PartialEq)]
pub struct ApmAlertReading {
    pub value: f64,
    pub service: String,
    /// Sample spans of the window, the slowest first; they lead an incident to its traces.
    pub trace_ids: Vec<String>,
}

#[async_trait]
pub trait ApmAlertSource: Send + Sync {
    /// The rule's figure over `range`; `None` when the service served no
    /// request in it, which no threshold matches.
    async fn read(
        &self,
        org_id: &Id,
        query: &ApmAlertQuery,
        range: TimeRange,
    ) -> Result<Option<ApmAlertReading>>;
}

/// Where the evaluator finds APM once it runs.
#[derive(Default)]
pub struct ApmAlertSlot(OnceLock<Arc<dyn ApmAlertSource>>);

impl ApmAlertSlot {
    pub fn fill(&self, source: Arc<dyn ApmAlertSource>) {
        let _ = self.0.set(source);
    }

    pub fn source(&self) -> Result<&Arc<dyn ApmAlertSource>> {
        self.0
            .get()
            .ok_or_else(|| Error::unavailable("APM is not running on this node"))
    }
}

/// The stream an APM rule stands for where the evaluator asks for one.
pub fn apm_stream_hint() -> StreamHint {
    StreamHint {
        name: "apm".into(),
        stream_type: StreamType::TRACES,
    }
}

/// A reading as the rest of the evaluator reads a query result: the value
/// first, then the service and a sample trace an incident is told by.
pub fn reading_result(reading: Option<ApmAlertReading>, took_ms: u64) -> QueryResult {
    let rows = reading
        .map(|reading| {
            vec![vec![
                json!(reading.value),
                json!(reading.service),
                json!(reading.trace_ids.first()),
            ]]
        })
        .unwrap_or_default();

    QueryResult {
        columns: vec!["value".into(), "service".into(), "trace_id".into()],
        scanned_rows: rows.len() as u64,
        rows,
        took_ms,
        federation: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_is_a_one_row_result_with_its_service_and_trace() {
        let result = reading_result(
            Some(ApmAlertReading {
                value: 4.5,
                service: "checkout".into(),
                trace_ids: vec!["abc".into()],
            }),
            3,
        );

        assert_eq!(result.columns, ["value", "service", "trace_id"]);
        assert_eq!(
            result.rows,
            vec![vec![json!(4.5), json!("checkout"), json!("abc")]]
        );
        assert!(reading_result(None, 0).rows.is_empty());
        assert!(ApmAlertSlot::default().source().is_err());
    }
}
