// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! domain Stream Schema → Arrow Schema 的映射。
//!
//! 隐式列：`_timestamp` 始终作为第一列（`Timestamp(Microsecond, UTC)`），
//! 由 ingester 从事件的 `timestamp` 字段填入。

use std::sync::Arc;

use arrow::{
    array::{ArrayRef, RecordBatch, new_null_array},
    datatypes::{DataType, Field, Schema as ArrowSchema, TimeUnit},
    error::ArrowError,
};

use crate::domain::stream::{FieldType, Schema as DomainSchema};

pub const TS_COL: &str = "_timestamp";

/// 把 domain Schema 转成 Arrow Schema，并在头部加 `_timestamp` 列。
pub fn to_arrow(schema: &DomainSchema) -> Arc<ArrowSchema> {
    let mut fields: Vec<Field> = Vec::with_capacity(schema.fields.len() + 1);
    fields.push(Field::new(
        TS_COL,
        DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
        false,
    ));
    for f in &schema.fields {
        // 加密字段以密文（Utf8）落盘，列类型固定 Utf8，与 ingester 写时一致；
        // 查询端 decrypt(col) 还原明文。
        let dt = if f.encrypted {
            DataType::Utf8
        } else {
            map_type(f.data_type)
        };
        fields.push(Field::new(&f.name, dt, f.nullable));
    }
    Arc::new(ArrowSchema::new(fields))
}

fn map_type(t: FieldType) -> DataType {
    match t {
        FieldType::Bool => DataType::Boolean,
        FieldType::Int64 => DataType::Int64,
        FieldType::Float64 => DataType::Float64,
        FieldType::Utf8 => DataType::Utf8,
        FieldType::Timestamp => DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
        FieldType::Json => DataType::Utf8, // 内嵌 JSON 序列化为字符串
    }
}

/// 把一个 RecordBatch 投影/扩展到 `target` schema。
///
/// 应对 stream schema 演化：老 parquet 文件可能只有 schema 子集，老 parquet_file_meta + 新
/// parquet_file_meta 在同一次查询里被拼到一个 MemTable 时会撞 "Mismatch between schema and
/// batches"。统一以 [`to_arrow`] 的输出作为权威 schema，本函数：
///
/// - 按 target 字段顺序取列；batch 里同名列存在 → 直接 reuse
/// - batch 里缺这列 → 用 `new_null_array` 占位（必须是 nullable 字段；target 来自
///   `to_arrow` 时除 `_timestamp` 外都是 nullable，符合预期）
/// - 同名列 data_type 不一致 → 返错（不做隐式 cast，避免静默丢精度）
pub fn align_batch_to_schema(
    batch: &RecordBatch,
    target: &Arc<ArrowSchema>,
) -> Result<RecordBatch, ArrowError> {
    let rows = batch.num_rows();
    let src = batch.schema();
    let mut cols: Vec<ArrayRef> = Vec::with_capacity(target.fields().len());
    for tf in target.fields() {
        match src.fields().iter().position(|f| f.name() == tf.name()) {
            Some(idx) => {
                let sf = src.field(idx);
                if sf.data_type() != tf.data_type() {
                    return Err(ArrowError::SchemaError(format!(
                        "column `{}` data_type mismatch: source={:?} target={:?}",
                        tf.name(),
                        sf.data_type(),
                        tf.data_type()
                    )));
                }
                cols.push(batch.column(idx).clone());
            }
            None => {
                cols.push(new_null_array(tf.data_type(), rows));
            }
        }
    }
    RecordBatch::try_new(target.clone(), cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::stream::FieldDef;

    #[test]
    fn ts_is_prepended() {
        let s = DomainSchema {
            fields: vec![FieldDef {
                name: "msg".into(),
                data_type: FieldType::Utf8,
                nullable: true,
                indexed: true,
                encrypted: false,
                exact: false,
            }],
        };
        let a = to_arrow(&s);
        assert_eq!(a.fields().len(), 2);
        assert_eq!(a.field(0).name(), TS_COL);
        assert_eq!(a.field(1).name(), "msg");
    }
}
