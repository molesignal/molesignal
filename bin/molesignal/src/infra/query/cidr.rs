// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `cidr_match(value TEXT, network TEXT) -> BOOLEAN`：地址是否落在网段里。
//!
//! IPv4 映射进 IPv6 比较（见 [`crate::domain::stream::cidr_range`]），`10.0.0.0/8` 也匹配
//! `::ffff:10.1.2.3`。值不是地址时为 false，任一参数为 NULL 时为 NULL。网段写错时，常量网段报错，
//! 逐行给出的网段在那一行为 NULL。`ip` 索引的字段上，顶层合取的 `cidr_match` 还用文件的地址
//! 范围跳过不可能命中的文件，见 [`super::skip_pruner`]。

use std::sync::Arc;

use arrow::{
    array::{Array, ArrayRef, BooleanArray, StringArray},
    compute::cast,
};
use datafusion::{
    arrow::datatypes::DataType,
    common::ScalarValue,
    error::{DataFusionError, Result},
    logical_expr::{
        ColumnarValue, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature, Volatility,
    },
};

use crate::domain::stream::{address_number, cidr_range};

pub fn cidr_match_udf() -> ScalarUDF {
    ScalarUDF::from(CidrMatch {
        signature: Signature::uniform(2, vec![DataType::Utf8], Volatility::Immutable),
    })
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct CidrMatch {
    signature: Signature,
}

impl ScalarUDFImpl for CidrMatch {
    fn name(&self) -> &str {
        "cidr_match"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let [value, network] = args.args.as_slice() else {
            return Err(DataFusionError::Execution(
                "cidr_match takes a value and a network".into(),
            ));
        };

        // 网段通常是常量：只解析一次。
        if let ColumnarValue::Scalar(network) = network {
            let Some(text) = scalar_text(network) else {
                return Ok(ColumnarValue::Scalar(ScalarValue::Boolean(None)));
            };

            let range = cidr_range(&text).ok_or_else(|| {
                DataFusionError::Execution(format!("cidr_match: `{text}` is not a network"))
            })?;

            return Ok(match value {
                ColumnarValue::Scalar(value) => ColumnarValue::Scalar(ScalarValue::Boolean(
                    scalar_text(value).map(|text| within(&text, range)),
                )),
                ColumnarValue::Array(values) => {
                    let values = text_array(values)?;

                    ColumnarValue::Array(Arc::new(
                        strings(&values)
                            .iter()
                            .map(|value| value.map(|text| within(text, range)))
                            .collect::<BooleanArray>(),
                    ))
                }
            });
        }

        let rows = args.number_rows;
        let values = text_array(&value.to_array(rows)?)?;
        let networks = text_array(&network.to_array(rows)?)?;

        let matched = strings(&values)
            .iter()
            .zip(strings(&networks).iter())
            .map(|(value, network)| {
                let range = cidr_range((*network)?)?;
                value.map(|text| within(text, range))
            })
            .collect::<BooleanArray>();

        Ok(ColumnarValue::Array(Arc::new(matched)))
    }
}

fn within(text: &str, (low, high): (u128, u128)) -> bool {
    address_number(text).is_some_and(|address| (low..=high).contains(&address))
}

fn scalar_text(value: &ScalarValue) -> Option<String> {
    match value {
        ScalarValue::Utf8(text) | ScalarValue::LargeUtf8(text) | ScalarValue::Utf8View(text) => {
            text.clone()
        }
        _ => None,
    }
}

/// 参数转成 `Utf8` 数组：字符串列也可能是 `LargeUtf8` 或 `Utf8View`。
fn text_array(array: &ArrayRef) -> Result<ArrayRef> {
    match array.data_type() {
        DataType::Utf8 => Ok(array.clone()),
        _ => Ok(cast(array, &DataType::Utf8)?),
    }
}

fn strings(array: &ArrayRef) -> Vec<Option<&str>> {
    array
        .as_any()
        .downcast_ref::<StringArray>()
        .map(|strings| strings.iter().collect())
        .unwrap_or_else(|| vec![None; array.len()])
}

#[cfg(test)]
mod tests {
    use arrow::{array::RecordBatch, datatypes::Field};
    use datafusion::{datasource::MemTable, prelude::SessionContext};

    use super::*;

    async fn matches(network: &str) -> Vec<Option<bool>> {
        let schema = Arc::new(arrow::datatypes::Schema::new(vec![Field::new(
            "client_ip",
            DataType::Utf8,
            true,
        )]));

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(StringArray::from(vec![
                Some("10.1.2.3"),
                Some("192.168.0.1"),
                Some("::ffff:10.0.0.7"),
                Some("unknown"),
                None,
            ]))],
        )
        .unwrap();

        let ctx = SessionContext::new();
        ctx.register_udf(cidr_match_udf());
        ctx.register_table(
            "logs",
            Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap()),
        )
        .unwrap();

        let batches = ctx
            .sql(&format!(
                "SELECT cidr_match(client_ip, '{network}') FROM logs"
            ))
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();

        batches
            .iter()
            .flat_map(|batch| {
                let column = batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<BooleanArray>()
                    .unwrap();

                column.iter().collect::<Vec<_>>()
            })
            .collect()
    }

    #[tokio::test]
    async fn addresses_inside_the_network_match() {
        assert_eq!(
            matches("10.0.0.0/8").await,
            [Some(true), Some(false), Some(true), Some(false), None]
        );
        assert_eq!(
            matches("192.168.0.1").await,
            [Some(false), Some(true), Some(false), Some(false), None]
        );
    }

    #[tokio::test]
    async fn a_malformed_constant_network_is_an_error() {
        let ctx = SessionContext::new();
        ctx.register_udf(cidr_match_udf());

        let result = ctx
            .sql("SELECT cidr_match('10.0.0.1', '10.0.0.0/40')")
            .await
            .unwrap()
            .collect()
            .await;

        assert!(result.is_err());
    }
}
