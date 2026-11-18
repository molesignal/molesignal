// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `contains_text(text, case_insensitive, needle, ...) -> BOOLEAN`: whether `text` holds one of
//! the needles, null where `text` is.
//!
//! Queries do not name it. [`ContainsLiteral`] rewrites `col LIKE '%x%'`, `col ILIKE '%x%'` with an
//! ASCII `x`, and `OR`s of such on one column, to calls of it. The Parquet scan then searches each
//! column buffer once for all the needles (see [`kernel`]) instead of once per row and per
//! pattern. Its answers are `LIKE`'s and `ILIKE`'s: byte for byte, and ASCII case folding for
//! `ILIKE`, as Arrow's kernels do for an ASCII pattern.

mod kernel;
mod rewrite;

use std::sync::Arc;

use arrow::{
    array::{Array, AsArray, BooleanArray},
    datatypes::DataType,
};
use datafusion::{
    common::ScalarValue,
    error::{DataFusionError, Result},
    logical_expr::{
        ColumnarValue, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature, Volatility,
    },
};
use kernel::Search;
pub(crate) use rewrite::ContainsLiteral;

/// The function's name.
pub const NAME: &str = "contains_text";

pub fn contains_text_udf() -> ScalarUDF {
    ScalarUDF::from(ContainsText {
        signature: Signature::variadic_any(Volatility::Immutable),
    })
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct ContainsText {
    signature: Signature,
}

impl ScalarUDFImpl for ContainsText {
    fn name(&self) -> &str {
        NAME
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let [text, fold, needles @ ..] = args.args.as_slice() else {
            return Err(failure("takes a text, a case flag and needles"));
        };

        let ColumnarValue::Scalar(ScalarValue::Boolean(Some(fold))) = fold else {
            return Err(failure("the case flag must be a boolean literal"));
        };

        let needles = needles
            .iter()
            .map(|needle| match needle {
                ColumnarValue::Scalar(needle) => {
                    scalar_text(needle).ok_or_else(|| failure("needles must be text literals"))
                }
                ColumnarValue::Array(_) => Err(failure("needles must be text literals")),
            })
            .collect::<Result<Vec<_>>>()?;

        let search = Search::new(&needles, *fold).map_err(|reason| failure(&reason))?;
        let mut scratch = Vec::new();

        let array = match text {
            ColumnarValue::Scalar(value) => {
                return Ok(ColumnarValue::Scalar(ScalarValue::Boolean(
                    scalar_text(value).map(|text| search.holds(&text, &mut scratch)),
                )));
            }
            ColumnarValue::Array(array) => array,
        };

        let found: BooleanArray = match array.data_type() {
            DataType::Utf8 => search.rows(array.as_string::<i32>(), &mut scratch),
            DataType::LargeUtf8 => search.rows(array.as_string::<i64>(), &mut scratch),
            DataType::Utf8View => array
                .as_string_view()
                .iter()
                .map(|text| text.map(|text| search.holds(text, &mut scratch)))
                .collect(),
            DataType::Null => BooleanArray::new_null(array.len()),
            other => return Err(failure(&format!("cannot search {other}"))),
        };

        Ok(ColumnarValue::Array(Arc::new(found)))
    }
}

fn scalar_text(value: &ScalarValue) -> Option<String> {
    match value {
        ScalarValue::Utf8(text) | ScalarValue::LargeUtf8(text) | ScalarValue::Utf8View(text) => {
            text.clone()
        }
        _ => None,
    }
}

fn failure(reason: &str) -> DataFusionError {
    DataFusionError::Execution(format!("{NAME}: {reason}"))
}

#[cfg(test)]
mod tests;
