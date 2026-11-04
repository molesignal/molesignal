// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The field query of the Logs and Traces pages, read on the server: a client
//! sends the text a person wrote and the reads take the same filters, words to
//! find and condition tree the pages would have sent. `contracts/field-query/`
//! holds the cases every reader of the text agrees on.

mod expression;
mod logs;
mod traces;

pub use expression::{FieldExpression, has_top_level_or, parse_field_expression, quoted_text};
pub use logs::{LogClause, LogCondition, LogMatch, read_log_query};
pub use traces::{MAX_TRACE_IN_VALUES, TraceClause, TraceCondition, TraceMatch, read_trace_query};

#[cfg(test)]
mod contract_tests {
    use serde_json::Value;

    use super::*;

    const CASES: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../contracts/field-query/cases.json"
    ));

    fn cases(reader: &str) -> Vec<Value> {
        let all: Value = serde_json::from_str(CASES).expect("cases.json");
        all[reader].as_array().expect("cases").clone()
    }

    fn check<T: serde::Serialize>(reader: &str, read: impl Fn(&str) -> crate::shared::Result<T>) {
        for case in cases(reader) {
            let query = case["query"].as_str().expect("query");

            match read(query) {
                Ok(_) if case["error"] == true => panic!("{reader}: `{query}` should be refused"),
                Ok(read) => assert_eq!(
                    serde_json::to_value(read).expect("json"),
                    case["read"],
                    "{reader}: `{query}`"
                ),
                Err(error) => assert!(
                    case["error"] == true,
                    "{reader}: `{query}` was refused: {error}"
                ),
            }
        }
    }

    #[test]
    fn log_queries_read_as_the_logs_page_reads_them() {
        check("logs", read_log_query);
    }

    #[test]
    fn trace_queries_read_as_the_traces_page_reads_them() {
        check("traces", read_trace_query);
    }

    #[test]
    fn a_refusal_names_the_part_it_cannot_read() {
        let error = read_log_query("level = 'error' AND (oops").unwrap_err();

        assert!(error.to_string().contains("(oops"), "{error}");
    }
}
