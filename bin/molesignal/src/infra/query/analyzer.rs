// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 自定义 DataFusion AnalyzerRule（Search 差异化）。
//!
//! `ResultRowGuard`：给**无 LIMIT 且非聚合**的查询注入一个结果行安全上限，防跑飞的
//! `SELECT *`（无界）把海量行物化到内存。这不是重写 DataFusion 内建优化（TopK / 聚合化简
//! 等已内建），而是一条**增量的资源保护规则**——与 admission（并发）/ sampling（扫描）互补。
//!
//! 安全性：已有 LIMIT（结果已有界，尊重用户）或含 Aggregate（注入 LIMIT 会截断分组，
//! 语义不安全）时**不注入**。默认 `max_rows = 0` → 关闭（OSS 零行为变化）。

use std::sync::Arc;

use datafusion::{
    common::{config::ConfigOptions, tree_node::TreeNode},
    error::Result as DfResult,
    execution::SessionStateBuilder,
    logical_expr::{LogicalPlan, LogicalPlanBuilder},
    optimizer::AnalyzerRule,
    prelude::SessionContext,
};

#[derive(Debug)]
pub struct ResultRowGuard {
    max_rows: usize,
}

impl ResultRowGuard {
    pub fn new(max_rows: usize) -> Self {
        Self { max_rows }
    }
}

impl AnalyzerRule for ResultRowGuard {
    fn name(&self) -> &str {
        "result_row_guard"
    }

    fn analyze(&self, plan: LogicalPlan, _config: &ConfigOptions) -> DfResult<LogicalPlan> {
        if self.max_rows == 0 {
            return Ok(plan);
        }
        // 已有 LIMIT（结果有界）或含聚合（注入会截断分组）→ 不注入。
        let bounded_or_agg = plan.exists(|n| {
            Ok(matches!(
                n,
                LogicalPlan::Limit(_) | LogicalPlan::Aggregate(_)
            ))
        })?;
        if bounded_or_agg {
            return Ok(plan);
        }
        LogicalPlanBuilder::from(plan)
            .limit(0, Some(self.max_rows))?
            .build()
    }
}

/// 构造一个 `SessionContext`：`max_rows > 0` 时注册 [`ResultRowGuard`]，否则用默认 ctx
/// （零行为变化）。engine 与单测共用，确保规则注册路径被真实覆盖。
pub fn session_context_with_guard(max_rows: usize) -> SessionContext {
    if max_rows == 0 {
        return SessionContext::new();
    }
    let state = SessionStateBuilder::new()
        .with_default_features()
        .with_analyzer_rule(Arc::new(ResultRowGuard::new(max_rows)))
        .build();
    SessionContext::new_with_state(state)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{ArrayRef, Int64Array},
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
    };
    use datafusion::datasource::MemTable;

    use super::*;

    async fn ctx_with_table(max_rows: usize) -> SessionContext {
        let ctx = session_context_with_guard(max_rows);
        let schema = Arc::new(Schema::new(vec![Field::new("v", DataType::Int64, true)]));
        let arr: ArrayRef = Arc::new(Int64Array::from(vec![1, 2, 3]));
        let batch = RecordBatch::try_new(schema.clone(), vec![arr]).unwrap();
        let mem = MemTable::try_new(schema, vec![vec![batch]]).unwrap();
        ctx.register_table("t", Arc::new(mem)).unwrap();
        ctx
    }

    async fn optimized_plan(ctx: &SessionContext, sql: &str) -> String {
        let plan = ctx.sql(sql).await.unwrap().into_optimized_plan().unwrap();
        format!("{plan}")
    }

    #[tokio::test]
    async fn injects_limit_on_unbounded_select() {
        let ctx = ctx_with_table(100).await;
        let plan = optimized_plan(&ctx, "SELECT * FROM t").await;
        assert!(
            plan.contains("Limit"),
            "expected injected Limit, got:\n{plan}"
        );
    }

    #[tokio::test]
    async fn respects_existing_limit() {
        let ctx = ctx_with_table(100).await;
        // 用户写了 LIMIT 2 → 不应被 100 覆盖。
        let plan = optimized_plan(&ctx, "SELECT * FROM t LIMIT 2").await;
        assert!(
            plan.contains("fetch=2") || plan.contains("Limit: skip=0, fetch=2"),
            "got:\n{plan}"
        );
    }

    #[tokio::test]
    async fn skips_aggregate_queries() {
        let ctx = ctx_with_table(100).await;
        let plan = optimized_plan(&ctx, "SELECT count(*) FROM t").await;
        assert!(
            !plan.contains("Limit"),
            "aggregate must not get a Limit, got:\n{plan}"
        );
    }

    #[tokio::test]
    async fn disabled_when_zero() {
        let ctx = ctx_with_table(0).await;
        let plan = optimized_plan(&ctx, "SELECT * FROM t").await;
        assert!(
            !plan.contains("Limit"),
            "guard off → no Limit, got:\n{plan}"
        );
    }
}
