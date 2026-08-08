// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Query engines（infra adapter 层）。
//!
//! - [`promql`]：PromQL 引擎。
//! - [`tantivy_pruner`]：基于 tantivy 倒排索引的 parquet_file_meta 候选裁剪。
//! - [`distributed`]：分布式 SQL 引擎，用 Arrow Flight 调远端 querier do_get。
//! - [`planner`]：多租户 planner rewrite；当前由 `ensure_stream_in_org` 校验越权。
//! - [`parser`]：基于 sqlparser AST 的 base table 引用提取（change `sqlparser-join-planner`）。
//! - [`rewrite`]：query rewrite 框架（org_id 注入等）；当前 passthrough。

pub mod analyzer;
pub mod distributed;
pub mod federated;
pub mod federation_cancel;
pub mod parquet_table;
pub mod parser;
pub mod planner;
pub mod promql;
pub mod rewrite;
pub mod sql_functions;
pub mod tantivy_pruner;
pub mod udafs;
pub mod udfs;

/// 转义一个 SQL 标识符，供双引号包裹后嵌入 SQL。调用方负责加外层引号：
/// `format!("FROM \"{}\"", escape_sql_ident(name))`。
///
/// 任何把流名拼进 SQL 的地方都必须过这里。流名可以含点，而 `default` 这类 SQL
/// 保留字只有被引号包裹才能出现在 `FROM` 后面；测试/内部仓库也可能构造更宽的名字。
pub fn escape_sql_ident(ident: &str) -> String {
    ident.replace('"', "\"\"")
}

#[cfg(test)]
mod ident_tests {
    use super::escape_sql_ident;

    #[test]
    fn escapes_embedded_double_quotes() {
        assert_eq!(escape_sql_ident(r#"we"ird"#), r#"we""ird"#);
        assert_eq!(escape_sql_ident("plain"), "plain");
        // 保留字与含点的名字本身不需要转义，靠调用方的外层引号生效。
        assert_eq!(escape_sql_ident("default"), "default");
        assert_eq!(escape_sql_ident("a.b"), "a.b");
    }
}
