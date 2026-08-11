// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Query rewrite 框架（spec query / change `sqlparser-join-planner`）。
//!
//! 当前实装：**passthrough**——参见 design.md D4。
//!
//! # ⚠️ 未接线，勿误信这是安全边界
//!
//! [`enforce_org_isolation`] **零生产调用者**（只在本模块测试里被调）。别因为它存在、
//! 名字像安全 API，就以为查询走了 SQL 层的 org 过滤——它没有。
//!
//! 当前的 org 隔离**由构造成立**：查询路径拿 file 候选唯一入口是
//! [`crate::domain::storage::ParquetFileMetaRepository::find`]`(org_id, ...)`，PG 侧
//! `WHERE org_id = $1` 天然只返回本 org 的文件，跨 org 的对象根本不进候选集。SQL 文本里
//! 没有、也不需要 `_org_id` 谓词。本函数是为「schema 加上 `_org_id` 列后改走 SQL 注入」
//! 预留的接口，在那之前它不承担任何隔离职责。
//!
//! 未来契约：当 stream schema 加上 `_org_id` 列后，本函数会把：
//!
//! ```sql
//! SELECT col FROM logs WHERE level = 'error'
//! ```
//!
//! 改写成：
//!
//! ```sql
//! SELECT col FROM (SELECT * FROM logs WHERE _org_id = '<org>') AS logs
//! WHERE level = 'error'
//! ```
//!
//! 在 SQL 字符串这一层做注入，而不是 DataFusion LogicalPlan rewriter，主要原因是
//! DataFusion 的 plan API 在 53 → 54 间反复调整（Subquery / TableScan filter
//! attach 方式），独立实现更稳定。
//!
//! 公开当前签名让 caller（datafusion_engine / api routes）即刻接入，未来切换实装
//! 不需要 API churn。

use crate::shared::{Result, ids::Id};

/// 在 SQL 中注入 `_org_id` 隔离过滤（**当前为透传**）。
///
/// 参数：
/// - `stmt`：用户原始 SQL；不要求 valid（实装后会先 `extract_referenced_tables`
///   失败则原样返回，让 DataFusion 报错更明确）。
/// - `org_id`：当前请求作用域的 org；实装后会作为常量字面量插入 WHERE。
///
/// 当前行为：直接返回 `Ok(stmt.to_string())`，仅消费参数以避免 dead-code 警告。
///
/// 行为契约（实装后）：
/// 1. 解析 `stmt`，对每个 base table 引用包成 `(SELECT * FROM <t> WHERE _org_id = '<org>') AS <t>`。
/// 2. 保留原 alias；保留 schema-qualified 仅当 dialect 真支持。
/// 3. CTE 内部 base table 同样改写；CTE 引用自身不改写。
/// 4. 无 base table 引用（如 `SELECT 1`）→ 原样返回。
/// 5. 解析失败 → 原样返回，让下游解析报错更精准。
pub fn enforce_org_isolation(stmt: &str, org_id: &Id) -> Result<String> {
    let _ = org_id; // future use
    Ok(stmt.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_preserves_sql() {
        let org = Id("orgA".into());
        let sql = "SELECT * FROM logs";
        assert_eq!(enforce_org_isolation(sql, &org).unwrap(), sql);
    }

    #[test]
    fn passthrough_handles_complex_sql() {
        let org = Id("orgB".into());
        let sql = "WITH x AS (SELECT * FROM logs) SELECT * FROM x JOIN traces t ON t.id = x.id";
        assert_eq!(enforce_org_isolation(sql, &org).unwrap(), sql);
    }
}
