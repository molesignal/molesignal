// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 查询能不能解析：SQL 用 query-language 的 sqlparser 解析，PromQL 用 PromQL 解析器。

use crate::agent::scoring::rules::QuerySyntax;

pub struct ParserSyntax;

impl QuerySyntax for ParserSyntax {
    fn sql_parses(&self, sql: &str) -> bool {
        query_language::extract_referenced_tables(sql).is_ok()
    }

    fn promql_parses(&self, promql: &str) -> bool {
        promql_parser::parser::parse(promql).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_the_engines_would() {
        assert!(ParserSyntax.sql_parses("SELECT count(*) FROM nginx WHERE status >= 500"));
        assert!(!ParserSyntax.sql_parses("SELEC count(*) FROM"));
        assert!(ParserSyntax.promql_parses("sum(rate(http_requests_total[5m])) by (job)"));
        assert!(!ParserSyntax.promql_parses("rate(http_requests_total[5m)"));
    }
}
