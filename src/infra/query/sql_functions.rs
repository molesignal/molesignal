// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SQL 文本检索函数能力清单（暴露给 API 客户端）。
//!
//! 与 `promql::capabilities` 同模式：函数实现与补全元数据放在同一处，UI 从
//! `/query/sql/capabilities` 拉取，绝不宣传引擎不支持的函数。当前清单：
//! [`MATCH`](crate::infra::query::tantivy_pruner)、[`MATCH_TEXT`]。

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlFunctionCompletionKind {
    Function,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SqlFunctionCompletion {
    pub label: &'static str,
    /// Monaco snippet 插入文本；`${1:...}` 为 Tab 跳转占位。
    pub insert_text: &'static str,
    pub detail: &'static str,
    pub documentation: &'static str,
    pub kind: SqlFunctionCompletionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SqlQueryCapabilities {
    pub engine: &'static str,
    pub version: u8,
    pub functions: Vec<SqlFunctionCompletion>,
}

/// SQL 检索函数能力：与 `extract_match_predicates`（`tantivy_pruner`）支持的调用
/// 一一对应，前端据此生成补全（全大写规范形式，解析大小写不敏感）。
pub fn sql_query_capabilities() -> SqlQueryCapabilities {
    SqlQueryCapabilities {
        engine: "molesignal-sql",
        version: 1,
        functions: vec![
            SqlFunctionCompletion {
                label: "MATCH",
                insert_text: "MATCH(${1:field}, '${2:term}')",
                detail: "任意字段子串匹配，大小写不敏感",
                documentation: "无索引前提。term 中的 % / _ 按字面量处理；空 term 恒不匹配。",
                kind: SqlFunctionCompletionKind::Function,
            },
            SqlFunctionCompletion {
                label: "MATCH_TEXT",
                insert_text: "MATCH_TEXT(${1:field}, '${2:query}')",
                detail: "全文检索（多词 / 短语 / 通配符）",
                documentation: "仅限已配置 full_text 索引的 string 字段（indexed && !exact），未配置时报错。",
                kind: SqlFunctionCompletionKind::Function,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_query_capabilities_has_the_two_text_search_functions() {
        let caps = sql_query_capabilities();
        assert_eq!(caps.engine, "molesignal-sql");
        let labels: Vec<&str> = caps.functions.iter().map(|f| f.label).collect();
        assert_eq!(labels, vec!["MATCH", "MATCH_TEXT"]);
        // 插入文本是 snippet，带 Tab 占位。
        assert!(caps.functions.iter().all(|f| f.insert_text.contains("${")));
    }
}
