// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! File-level time bounds and zone maps.

use arrow::{
    array::{Array, RecordBatch},
    compute::{max as array_max, min as array_min},
    datatypes::{DataType, TimeUnit},
};

use crate::{
    domain::stream::{StreamDefinition, StreamIndexType},
    infra::storage::arrow_schema::TS_COL,
    shared::{
        Error, Result,
        trace::summary::{
            TRACE_SUMMARY_DURATION_NS_FIELD, TRACE_SUMMARY_ERROR_COUNT_FIELD,
            TRACE_SUMMARY_SPAN_COUNT_FIELD, TRACE_SUMMARY_START_NS_FIELD,
        },
    },
};

pub(super) fn timestamp_range(batch: &RecordBatch) -> Result<(i64, i64)> {
    let index = batch
        .schema()
        .index_of(TS_COL)
        .map_err(|_| Error::internal(format!("batch missing column {TS_COL}")))?;
    let column = batch.column(index);
    let DataType::Timestamp(TimeUnit::Microsecond, _) = column.data_type() else {
        return Err(Error::internal(format!(
            "{TS_COL} must be Timestamp(Microsecond), got {:?}",
            column.data_type()
        )));
    };
    let timestamps = column
        .as_any()
        .downcast_ref::<arrow::array::TimestampMicrosecondArray>()
        .ok_or_else(|| Error::internal("downcast _timestamp"))?;
    if timestamps.is_empty() {
        return Err(Error::internal("empty _timestamp column"));
    }
    let mut minimum = i64::MAX;
    let mut maximum = i64::MIN;
    for timestamp in timestamps.iter().flatten() {
        minimum = minimum.min(timestamp);
        maximum = maximum.max(timestamp);
    }
    Ok((minimum, maximum))
}

pub(super) fn zone_maps(
    stream: &StreamDefinition,
    batch: &RecordBatch,
) -> (
    serde_json::Map<String, serde_json::Value>,
    serde_json::Map<String, serde_json::Value>,
) {
    let mut minimums = serde_json::Map::new();
    let mut maximums = serde_json::Map::new();
    // Sort fields need zone maps even when users did not explicitly index them. The dedicated
    // Trace scanner uses these bounds to prove that later files cannot enter its retained Top-K.
    let sort_fields = [
        TRACE_SUMMARY_START_NS_FIELD,
        TRACE_SUMMARY_DURATION_NS_FIELD,
        TRACE_SUMMARY_SPAN_COUNT_FIELD,
        TRACE_SUMMARY_ERROR_COUNT_FIELD,
    ];
    for field in stream.schema.fields.iter().filter(|field| {
        field.effective_index_type() != StreamIndexType::None
            || sort_fields.contains(&field.name.as_str())
    }) {
        let Ok(index) = batch.schema().index_of(&field.name) else {
            continue;
        };
        let column = batch.column(index);
        if let Some(value) = scalar_min(column) {
            minimums.insert(field.name.clone(), value);
        }
        if let Some(value) = scalar_max(column) {
            maximums.insert(field.name.clone(), value);
        }
    }
    (minimums, maximums)
}

fn scalar_min(array: &dyn Array) -> Option<serde_json::Value> {
    use arrow::array::{
        BooleanArray, Float64Array, Int64Array, StringArray, TimestampMicrosecondArray,
    };

    if let Some(values) = array.as_any().downcast_ref::<Int64Array>() {
        return array_min(values).map(serde_json::Value::from);
    }
    if let Some(values) = array.as_any().downcast_ref::<Float64Array>() {
        return finite_f64_extreme(values.iter().flatten(), f64::min);
    }
    if let Some(values) = array.as_any().downcast_ref::<BooleanArray>() {
        return values.iter().flatten().min().map(serde_json::Value::from);
    }
    if let Some(values) = array.as_any().downcast_ref::<TimestampMicrosecondArray>() {
        return values.iter().flatten().min().map(serde_json::Value::from);
    }
    array
        .as_any()
        .downcast_ref::<StringArray>()
        .and_then(|values| {
            values
                .iter()
                .flatten()
                .min()
                .map(|value| serde_json::Value::from(value.to_string()))
        })
}

fn scalar_max(array: &dyn Array) -> Option<serde_json::Value> {
    use arrow::array::{
        BooleanArray, Float64Array, Int64Array, StringArray, TimestampMicrosecondArray,
    };

    if let Some(values) = array.as_any().downcast_ref::<Int64Array>() {
        return array_max(values).map(serde_json::Value::from);
    }
    if let Some(values) = array.as_any().downcast_ref::<Float64Array>() {
        return finite_f64_extreme(values.iter().flatten(), f64::max);
    }
    if let Some(values) = array.as_any().downcast_ref::<BooleanArray>() {
        return values.iter().flatten().max().map(serde_json::Value::from);
    }
    if let Some(values) = array.as_any().downcast_ref::<TimestampMicrosecondArray>() {
        return values.iter().flatten().max().map(serde_json::Value::from);
    }
    array
        .as_any()
        .downcast_ref::<StringArray>()
        .and_then(|values| {
            values
                .iter()
                .flatten()
                .max()
                .map(|value| serde_json::Value::from(value.to_string()))
        })
}

fn finite_f64_extreme(
    values: impl Iterator<Item = f64>,
    select: impl Fn(f64, f64) -> f64,
) -> Option<serde_json::Value> {
    let mut extreme: Option<f64> = None;
    for value in values {
        if !value.is_finite() {
            return None;
        }
        extreme = Some(extreme.map_or(value, |current| select(current, value)));
    }
    extreme
        .and_then(serde_json::Number::from_f64)
        .map(serde_json::Value::Number)
}

#[cfg(test)]
mod tests {
    use arrow::array::{
        BooleanArray, Float64Array, Int64Array, StringArray, TimestampMicrosecondArray,
    };
    use serde_json::json;

    use super::*;

    #[test]
    fn zone_map_scalars_cover_every_supported_field_representation() {
        let int = Int64Array::from(vec![Some(3), None, Some(-2)]);
        assert_eq!(scalar_min(&int), Some(json!(-2)));
        assert_eq!(scalar_max(&int), Some(json!(3)));

        let float = Float64Array::from(vec![Some(3.5), None, Some(-2.25)]);
        assert_eq!(scalar_min(&float), Some(json!(-2.25)));
        assert_eq!(scalar_max(&float), Some(json!(3.5)));

        let boolean = BooleanArray::from(vec![Some(true), None, Some(false)]);
        assert_eq!(scalar_min(&boolean), Some(json!(false)));
        assert_eq!(scalar_max(&boolean), Some(json!(true)));

        let timestamp = TimestampMicrosecondArray::from(vec![Some(20), None, Some(10)]);
        assert_eq!(scalar_min(&timestamp), Some(json!(10)));
        assert_eq!(scalar_max(&timestamp), Some(json!(20)));

        let string = StringArray::from(vec![Some("web"), None, Some("api")]);
        assert_eq!(scalar_min(&string), Some(json!("api")));
        assert_eq!(scalar_max(&string), Some(json!("web")));
    }

    #[test]
    fn non_finite_float_disables_zone_map_instead_of_writing_unsound_bounds() {
        let values = Float64Array::from(vec![1.0, f64::NAN]);
        assert_eq!(scalar_min(&values), None);
        assert_eq!(scalar_max(&values), None);
    }
}
