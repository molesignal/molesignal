// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `skip` 字段索引的文件级 min/max（zone map）裁剪。
//!
//! 只提取顶层 `WHERE` 合取链中的简单比较；`OR`、函数和无法证明的类型组合全部保守保留。
//! 索引只决定一个 Parquet 文件是否可能命中，最终行级语义仍由 DataFusion 执行。

use std::{cmp::Ordering, collections::HashMap};

use sqlparser::{
    ast::{BinaryOperator, Expr, SetExpr, Statement, UnaryOperator, Value as SqlValue},
    dialect::GenericDialect,
    parser::Parser,
};

use crate::domain::{
    storage::ParquetFileMeta,
    stream::{FieldType, Schema, StreamIndexType},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparison {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

impl Comparison {
    fn from_binary(operator: &BinaryOperator) -> Option<Self> {
        match operator {
            BinaryOperator::Eq => Some(Self::Eq),
            BinaryOperator::NotEq => Some(Self::NotEq),
            BinaryOperator::Lt => Some(Self::Lt),
            BinaryOperator::LtEq => Some(Self::LtEq),
            BinaryOperator::Gt => Some(Self::Gt),
            BinaryOperator::GtEq => Some(Self::GtEq),
            _ => None,
        }
    }

    fn mirrored(self) -> Self {
        match self {
            Self::Eq => Self::Eq,
            Self::NotEq => Self::NotEq,
            Self::Lt => Self::Gt,
            Self::LtEq => Self::GtEq,
            Self::Gt => Self::Lt,
            Self::GtEq => Self::LtEq,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Literal {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

impl Literal {
    fn from_expr(expression: &Expr) -> Option<Self> {
        match expression {
            Expr::Value(value) => match &value.value {
                SqlValue::SingleQuotedString(value) => Some(Self::String(value.clone())),
                SqlValue::Boolean(value) => Some(Self::Bool(*value)),
                SqlValue::Number(value, _) => value
                    .parse::<i64>()
                    .map(Self::Int)
                    .or_else(|_| value.parse::<f64>().map(Self::Float))
                    .ok(),
                _ => None,
            },
            Expr::UnaryOp {
                op: UnaryOperator::Minus,
                expr,
            } => match Self::from_expr(expr)? {
                Self::Int(value) => value.checked_neg().map(Self::Int),
                Self::Float(value) => Some(Self::Float(-value)),
                Self::Bool(_) | Self::String(_) => None,
            },
            Expr::UnaryOp {
                op: UnaryOperator::Plus,
                expr,
            }
            | Expr::Nested(expr) => Self::from_expr(expr),
            _ => None,
        }
    }

    fn supports(&self, data_type: FieldType) -> bool {
        matches!(
            (self, data_type),
            (Self::Bool(_), FieldType::Bool)
                | (Self::Int(_), FieldType::Int64 | FieldType::Timestamp)
                | (Self::Int(_) | Self::Float(_), FieldType::Float64)
                | (Self::String(_), FieldType::Utf8 | FieldType::Json)
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
struct SkipPredicate {
    field: String,
    data_type: FieldType,
    comparison: Comparison,
    literal: Literal,
}

impl SkipPredicate {
    fn file_may_match(&self, file: &ParquetFileMeta) -> bool {
        let Some(minimum) = file.min_values.get(&self.field) else {
            return true;
        };
        let Some(maximum) = file.max_values.get(&self.field) else {
            return true;
        };
        let Some(minimum_ordering) = compare_bound(minimum, &self.literal, self.data_type) else {
            return true;
        };
        let Some(maximum_ordering) = compare_bound(maximum, &self.literal, self.data_type) else {
            return true;
        };

        match self.comparison {
            Comparison::Eq => {
                minimum_ordering != Ordering::Greater && maximum_ordering != Ordering::Less
            }
            Comparison::NotEq => {
                !(minimum_ordering == Ordering::Equal && maximum_ordering == Ordering::Equal)
            }
            Comparison::Lt => minimum_ordering == Ordering::Less,
            Comparison::LtEq => minimum_ordering != Ordering::Greater,
            Comparison::Gt => maximum_ordering == Ordering::Greater,
            Comparison::GtEq => maximum_ordering != Ordering::Less,
        }
    }
}

/// 用 `skip` 字段的文件级 min/max 排除不可能命中的候选文件。
pub fn prune(
    files: Vec<ParquetFileMeta>,
    statement: &str,
    schema: &Schema,
) -> Vec<ParquetFileMeta> {
    let predicates = extract_predicates(statement, schema);
    if predicates.is_empty() {
        return files;
    }
    files
        .into_iter()
        .filter(|file| {
            predicates
                .iter()
                .all(|predicate| predicate.file_may_match(file))
        })
        .collect()
}

fn extract_predicates(statement: &str, schema: &Schema) -> Vec<SkipPredicate> {
    let skip_fields: HashMap<&str, FieldType> = schema
        .fields
        .iter()
        .filter(|field| !field.encrypted && field.effective_index_type() == StreamIndexType::Skip)
        .map(|field| (field.name.as_str(), field.data_type))
        .collect();
    if skip_fields.is_empty() {
        return Vec::new();
    }

    let Ok(statements) = Parser::parse_sql(&GenericDialect, statement) else {
        return Vec::new();
    };
    if statements.len() != 1 {
        return Vec::new();
    }
    let mut predicates = Vec::new();
    for statement in &statements {
        if let Statement::Query(query) = statement
            && let SetExpr::Select(select) = query.body.as_ref()
            && let Some(selection) = &select.selection
        {
            collect_conjuncts(selection, &skip_fields, &mut predicates);
        }
    }
    predicates
}

fn collect_conjuncts(
    expression: &Expr,
    fields: &HashMap<&str, FieldType>,
    predicates: &mut Vec<SkipPredicate>,
) {
    match expression {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            collect_conjuncts(left, fields, predicates);
            collect_conjuncts(right, fields, predicates);
        }
        Expr::BinaryOp { left, op, right } => {
            let Some(comparison) = Comparison::from_binary(op) else {
                return;
            };
            if let Some(predicate) = predicate(left, right, comparison, fields)
                .or_else(|| predicate(right, left, comparison.mirrored(), fields))
            {
                predicates.push(predicate);
            }
        }
        Expr::Nested(inner) => collect_conjuncts(inner, fields, predicates),
        _ => {}
    }
}

fn predicate(
    column: &Expr,
    value: &Expr,
    comparison: Comparison,
    fields: &HashMap<&str, FieldType>,
) -> Option<SkipPredicate> {
    let field = unqualified_column(column)?;
    let data_type = *fields.get(field)?;
    let literal = Literal::from_expr(value)?;
    if !literal.supports(data_type) {
        return None;
    }
    Some(SkipPredicate {
        field: field.to_string(),
        data_type,
        comparison,
        literal,
    })
}

/// 只接受无表限定的列，避免多表查询中拿另一张表的谓词裁主表。日志筛选器对时间字段生成
/// `CAST(field AS BIGINT)`，因此透明剥掉 cast/nested 包装。
fn unqualified_column(expression: &Expr) -> Option<&str> {
    match expression {
        Expr::Identifier(identifier) => Some(identifier.value.as_str()),
        Expr::Cast { expr, .. } | Expr::Nested(expr) => unqualified_column(expr),
        _ => None,
    }
}

fn compare_bound(
    bound: &serde_json::Value,
    literal: &Literal,
    data_type: FieldType,
) -> Option<Ordering> {
    match (data_type, literal) {
        (FieldType::Bool, Literal::Bool(value)) => bound.as_bool()?.partial_cmp(value),
        (FieldType::Int64 | FieldType::Timestamp, Literal::Int(value)) => {
            bound.as_i64()?.partial_cmp(value)
        }
        (FieldType::Float64, Literal::Int(value)) => bound.as_f64()?.partial_cmp(&(*value as f64)),
        (FieldType::Float64, Literal::Float(value)) if value.is_finite() => {
            bound.as_f64()?.partial_cmp(value)
        }
        (FieldType::Utf8 | FieldType::Json, Literal::String(value)) => {
            Some(bound.as_str()?.cmp(value.as_str()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::shared::{
        ids::Id,
        time::{TimeRange, TimestampMicros},
    };

    fn schema(index_type: StreamIndexType, data_type: FieldType) -> Schema {
        Schema {
            fields: vec![crate::domain::stream::FieldDef {
                name: "value".into(),
                data_type,
                nullable: true,
                index_type: Some(index_type),
                indexed: index_type != StreamIndexType::None,
                encrypted: false,
                exact: index_type == StreamIndexType::Exact,
            }],
        }
    }

    fn file(id: &str, minimum: serde_json::Value, maximum: serde_json::Value) -> ParquetFileMeta {
        ParquetFileMeta {
            id: Id::from_string(id),
            org_id: Id::from_string("org"),
            stream: "logs".into(),
            stream_type: crate::domain::stream::StreamType::Logs,
            dataset_kind: Default::default(),
            object_key: format!("{id}.parquet"),
            time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1)),
            rows: 1,
            size_bytes: 1,
            min_values: [("value".into(), minimum)].into_iter().collect(),
            max_values: [("value".into(), maximum)].into_iter().collect(),
            deleted: false,
        }
    }

    #[test]
    fn numeric_comparisons_prune_only_proven_misses() {
        let files = vec![
            file("low", json!(10), json!(20)),
            file("high", json!(30), json!(40)),
        ];
        let kept = prune(
            files.clone(),
            "SELECT * FROM logs WHERE value > 25",
            &schema(StreamIndexType::Skip, FieldType::Int64),
        );
        assert_eq!(
            kept.iter().map(|file| file.id.as_str()).collect::<Vec<_>>(),
            vec!["high"]
        );

        let kept = prune(
            files,
            "SELECT * FROM logs WHERE value = 20",
            &schema(StreamIndexType::Skip, FieldType::Int64),
        );
        assert_eq!(
            kept.iter().map(|file| file.id.as_str()).collect::<Vec<_>>(),
            vec!["low"]
        );
    }

    #[test]
    fn disjunction_and_non_skip_fields_are_never_pruned() {
        let files = vec![file("one", json!(10), json!(20))];
        assert_eq!(
            prune(
                files.clone(),
                "SELECT * FROM logs WHERE value > 100 OR other = 1",
                &schema(StreamIndexType::Skip, FieldType::Int64),
            )
            .len(),
            1
        );
        assert_eq!(
            prune(
                files,
                "SELECT * FROM logs WHERE value > 100",
                &schema(StreamIndexType::Bloom, FieldType::Int64),
            )
            .len(),
            1
        );
    }

    #[test]
    fn string_boolean_and_cast_timestamp_bounds_are_supported() {
        assert!(
            prune(
                vec![file("string", json!("api"), json!("web"))],
                "SELECT * FROM logs WHERE value = 'zzz'",
                &schema(StreamIndexType::Skip, FieldType::Utf8),
            )
            .is_empty()
        );
        assert!(
            prune(
                vec![file("bool", json!(false), json!(false))],
                "SELECT * FROM logs WHERE value = TRUE",
                &schema(StreamIndexType::Skip, FieldType::Bool),
            )
            .is_empty()
        );
        assert!(
            prune(
                vec![file("timestamp", json!(100), json!(200))],
                "SELECT * FROM logs WHERE CAST(value AS BIGINT) >= 300",
                &schema(StreamIndexType::Skip, FieldType::Timestamp),
            )
            .is_empty()
        );
    }

    #[test]
    fn missing_bounds_and_qualified_columns_degrade_to_keep() {
        let mut missing = file("missing", json!(1), json!(2));
        missing.max_values.clear();
        let schema = schema(StreamIndexType::Skip, FieldType::Int64);
        assert_eq!(
            prune(
                vec![missing],
                "SELECT * FROM logs WHERE value > 100",
                &schema,
            )
            .len(),
            1
        );
        assert_eq!(
            prune(
                vec![file("qualified", json!(1), json!(2))],
                "SELECT * FROM logs l WHERE l.value > 100",
                &schema,
            )
            .len(),
            1
        );
    }

    #[test]
    fn encrypted_fields_and_uncertain_predicates_are_never_used_for_skipping() {
        let files = vec![file("encrypted", json!(1), json!(2))];
        let mut encrypted = schema(StreamIndexType::Skip, FieldType::Int64);
        encrypted.fields[0].encrypted = true;
        assert_eq!(
            prune(
                files.clone(),
                "SELECT * FROM logs WHERE value > 100",
                &encrypted,
            )
            .len(),
            1
        );
        assert_eq!(
            prune(
                files,
                "SELECT * FROM logs WHERE value + 1 > 100",
                &schema(StreamIndexType::Skip, FieldType::Int64),
            )
            .len(),
            1
        );
    }
}
