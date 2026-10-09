// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `_timestamp` 与整数字面量比较的类型修正。
//!
//! 产品内所有时间都以 Unix 微秒整数表达（`time_range`、`CAST(_timestamp AS BIGINT)`、
//! `search_around` 的指针），而 `_timestamp` 列是 `Timestamp(µs, "UTC")`。DataFusion 的类型强制
//! 不会让整数与时间戳比较，`WHERE _timestamp <= 1790000000000000` 会在执行期以
//! `Cannot infer common argument type` 失败。本规则把这类整数字面量按微秒解释，改写成
//! 显式时间戳转换，需要先于内建 `TypeCoercion` 运行。

use arrow::datatypes::{DataType, TimeUnit};
use datafusion::{
    common::{
        config::ConfigOptions,
        tree_node::{Transformed, TreeNode},
    },
    error::Result as DfResult,
    logical_expr::{
        Cast, Expr, LogicalPlan, Operator,
        expr::{Between, BinaryExpr},
    },
    optimizer::AnalyzerRule,
    scalar::ScalarValue,
};

/// 所有流共用的隐式时间列。
const TIMESTAMP_COLUMN: &str = "_timestamp";

#[derive(Debug, Default)]
pub struct TimestampIntegerLiteral;

impl AnalyzerRule for TimestampIntegerLiteral {
    fn name(&self) -> &str {
        "timestamp_integer_literal"
    }

    fn analyze(&self, plan: LogicalPlan, _config: &ConfigOptions) -> DfResult<LogicalPlan> {
        plan.transform_up_with_subqueries(|node| {
            node.map_expressions(|expr| expr.transform_up(rewrite_comparison))
        })
        .map(|transformed| transformed.data)
    }
}

fn rewrite_comparison(expr: Expr) -> DfResult<Transformed<Expr>> {
    Ok(match expr {
        Expr::BinaryExpr(BinaryExpr { left, op, right }) if is_comparison(op) => {
            if is_timestamp_column(&left) && is_integer_literal(&right) {
                Transformed::yes(Expr::BinaryExpr(BinaryExpr {
                    left,
                    op,
                    right: as_micros(right),
                }))
            } else if is_integer_literal(&left) && is_timestamp_column(&right) {
                Transformed::yes(Expr::BinaryExpr(BinaryExpr {
                    left: as_micros(left),
                    op,
                    right,
                }))
            } else {
                Transformed::no(Expr::BinaryExpr(BinaryExpr { left, op, right }))
            }
        }
        Expr::Between(Between {
            expr,
            negated,
            low,
            high,
        }) if is_timestamp_column(&expr)
            && (is_integer_literal(&low) || is_integer_literal(&high)) =>
        {
            let low = if is_integer_literal(&low) {
                as_micros(low)
            } else {
                low
            };
            let high = if is_integer_literal(&high) {
                as_micros(high)
            } else {
                high
            };
            Transformed::yes(Expr::Between(Between {
                expr,
                negated,
                low,
                high,
            }))
        }
        other => Transformed::no(other),
    })
}

fn is_comparison(op: Operator) -> bool {
    matches!(
        op,
        Operator::Eq
            | Operator::NotEq
            | Operator::Lt
            | Operator::LtEq
            | Operator::Gt
            | Operator::GtEq
    )
}

fn is_timestamp_column(expr: &Expr) -> bool {
    matches!(expr, Expr::Column(column) if column.name == TIMESTAMP_COLUMN)
}

fn is_integer_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Literal(
            ScalarValue::Int64(Some(_)) | ScalarValue::UInt64(Some(_)),
            _
        )
    )
}

/// Arrow 把整数转成时间戳时按目标单位解释，所以这里得到的是「N 微秒」。
fn as_micros(literal: Box<Expr>) -> Box<Expr> {
    Box::new(Expr::Cast(Cast::new(
        literal,
        DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
    )))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{
        array::{ArrayRef, Int64Array, RecordBatch, TimestampMicrosecondArray},
        datatypes::{DataType, Field, Schema, TimeUnit},
    };
    use datafusion::{datasource::MemTable, prelude::SessionContext};

    use crate::infra::query::analyzer::session_context;

    /// `_timestamp` 取 1000 / 2000 / 3000 微秒，`v` 取 1 / 2 / 3。
    async fn ctx() -> SessionContext {
        let ctx = session_context(0);
        let ts: ArrayRef = Arc::new(
            TimestampMicrosecondArray::from(vec![1_000, 2_000, 3_000]).with_timezone("UTC"),
        );
        let v: ArrayRef = Arc::new(Int64Array::from(vec![1, 2, 3]));
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "_timestamp",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("v", DataType::Int64, false),
        ]));
        let batch = RecordBatch::try_new(schema.clone(), vec![ts, v]).unwrap();
        let table = MemTable::try_new(schema, vec![vec![batch]]).unwrap();
        ctx.register_table("t", Arc::new(table)).unwrap();
        ctx
    }

    async fn count(ctx: &SessionContext, predicate: &str) -> i64 {
        let sql = format!("SELECT count(*) AS n FROM t WHERE {predicate}");
        let batches = ctx.sql(&sql).await.unwrap().collect().await.unwrap();
        batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0)
    }

    #[tokio::test]
    async fn integer_literals_compare_as_microseconds() {
        let ctx = ctx().await;
        assert_eq!(count(&ctx, "_timestamp <= 2000").await, 2);
        assert_eq!(count(&ctx, "_timestamp > 1000").await, 2);
        assert_eq!(count(&ctx, "_timestamp = 3000").await, 1);
        assert_eq!(count(&ctx, "_timestamp != 3000").await, 2);
        assert_eq!(count(&ctx, "3000 >= _timestamp").await, 3);
        assert_eq!(count(&ctx, "1500 < _timestamp").await, 2);
    }

    #[tokio::test]
    async fn between_accepts_integer_bounds() {
        let ctx = ctx().await;
        assert_eq!(count(&ctx, "_timestamp BETWEEN 1500 AND 2500").await, 1);
        assert_eq!(count(&ctx, "_timestamp NOT BETWEEN 1500 AND 2500").await, 2);
    }

    #[tokio::test]
    async fn existing_forms_and_other_columns_are_untouched() {
        let ctx = ctx().await;
        assert_eq!(
            count(&ctx, "_timestamp <= to_timestamp_micros(2000)").await,
            2
        );
        assert_eq!(count(&ctx, "CAST(_timestamp AS BIGINT) <= 2000").await, 2);
        assert_eq!(count(&ctx, "v <= 2").await, 2);
    }
}
