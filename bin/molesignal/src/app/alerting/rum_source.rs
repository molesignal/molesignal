// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The RUM figures alert rules watch, read by the evaluator in place of a query. The evaluator
//! holds a [`RumAlertSource`]; what it reads comes back as a query result so that the rest of the
//! evaluation, thresholds and baselines and incidents, needs not know where a number came from.

use serde_json::{Value, json};

use crate::domain::{
    query::{QueryResult, StreamHint},
    rum::{RumAlertReading, RumAlertSample},
    stream::StreamType,
};

/// The stream a RUM rule stands for where the evaluator asks for one.
pub fn rum_stream_hint() -> StreamHint {
    StreamHint {
        name: "rum".into(),
        stream_type: StreamType::LOGS,
    }
}

/// A reading as the rest of the evaluator reads a query result: the value first, then the
/// application. No reading is a result without rows, which no threshold matches.
pub fn reading_result(
    application_id: &str,
    reading: Option<RumAlertReading>,
    took_ms: u64,
) -> QueryResult {
    let scanned_rows = reading.as_ref().map_or(0, |reading| reading.samples);

    let rows = reading
        .map(|reading| vec![vec![json!(reading.value), json!(application_id)]])
        .unwrap_or_default();

    QueryResult {
        columns: vec!["value".into(), "application".into()],
        scanned_rows,
        rows,
        took_ms,
        federation: None,
    }
}

/// The first look a firing rule took, with the sessions an incident is told by added: one row for
/// each, carrying the figure again so that the first column stays the figure, and the session, the
/// time and what went with it as columns the incident keeps as the labels of its samples.
pub fn with_samples(result: QueryResult, samples: &[RumAlertSample]) -> QueryResult {
    let Some(first) = result.rows.first() else {
        return result;
    };

    if samples.is_empty() {
        return result;
    }

    let value = first.first().cloned().unwrap_or(Value::Null);
    let application = first.get(1).cloned().unwrap_or(Value::Null);

    let rows = samples
        .iter()
        .map(|sample| {
            vec![
                value.clone(),
                application.clone(),
                json!(sample.session_id),
                json!(sample.at.0),
                sample.value.map_or(Value::Null, |value| json!(value)),
                sample
                    .detail
                    .as_ref()
                    .map_or(Value::Null, |text| json!(text)),
            ]
        })
        .collect();

    QueryResult {
        columns: [
            "value",
            "application",
            "rum.session_id",
            "_timestamp",
            "rum.value",
            "rum.detail",
        ]
        .map(String::from)
        .to_vec(),
        rows,
        ..result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::time::TimestampMicros;

    #[test]
    fn a_reading_is_a_one_row_result_and_no_reading_has_no_rows() {
        let result = reading_result(
            "shop-web",
            Some(RumAlertReading {
                value: 4.5,
                samples: 120,
            }),
            3,
        );

        assert_eq!(result.columns, ["value", "application"]);
        assert_eq!(result.rows, vec![vec![json!(4.5), json!("shop-web")]]);
        assert_eq!(result.scanned_rows, 120);
        assert!(reading_result("shop-web", None, 0).rows.is_empty());
    }

    #[test]
    fn the_sessions_come_after_the_figure_and_keep_it_first() {
        let result = with_samples(
            reading_result(
                "shop-web",
                Some(RumAlertReading {
                    value: 4.3,
                    samples: 80,
                }),
                1,
            ),
            &[
                RumAlertSample {
                    session_id: "s-1".into(),
                    at: TimestampMicros(10),
                    value: Some(7_200.0),
                    detail: Some("img.hero".into()),
                },
                RumAlertSample {
                    session_id: "s-2".into(),
                    at: TimestampMicros(20),
                    value: None,
                    detail: None,
                },
            ],
        );

        assert_eq!(result.columns[0], "value");
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], json!(4.3));
        assert_eq!(result.rows[0][2], json!("s-1"));
        assert_eq!(result.rows[0][3], json!(10));
        assert_eq!(result.rows[0][5], json!("img.hero"));
        assert_eq!(result.rows[1][4], Value::Null);
    }

    #[test]
    fn no_figure_or_no_session_leaves_the_result_as_it_was() {
        let none = reading_result("shop-web", None, 0);

        assert!(with_samples(none, &[]).rows.is_empty());

        let some = reading_result(
            "shop-web",
            Some(RumAlertReading {
                value: 1.0,
                samples: 1,
            }),
            0,
        );

        assert_eq!(with_samples(some, &[]).columns, ["value", "application"]);
    }
}
