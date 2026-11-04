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

/// Sort fields need zone maps even when users did not explicitly index them. The dedicated Trace
/// scanner uses these bounds to prove that later files cannot enter its retained Top-K.
pub(super) const SORT_FIELDS: [&str; 4] = [
    TRACE_SUMMARY_START_NS_FIELD,
    TRACE_SUMMARY_DURATION_NS_FIELD,
    TRACE_SUMMARY_SPAN_COUNT_FIELD,
    TRACE_SUMMARY_ERROR_COUNT_FIELD,
];

pub(super) fn zone_maps(
    stream: &StreamDefinition,
    batch: &RecordBatch,
) -> (
    serde_json::Map<String, serde_json::Value>,
    serde_json::Map<String, serde_json::Value>,
) {
    let mut minimums = serde_json::Map::new();
    let mut maximums = serde_json::Map::new();

    // Text searched by words or substrings never gets a range predicate: its bounds, two whole
    // log lines per file, would only weigh on the catalog.
    for field in stream.schema.fields.iter().filter(|field| {
        matches!(
            field.effective_index_type(),
            StreamIndexType::Exact | StreamIndexType::Bloom | StreamIndexType::Skip
        ) || SORT_FIELDS.contains(&field.name.as_str())
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

/// Folds the zone maps of one more batch into `minimums` and `maximums`, so the maps of a file
/// written batch by batch equal the maps of the whole file.
pub(super) fn merge_zone_maps(
    (minimums, maximums): (
        &mut serde_json::Map<String, serde_json::Value>,
        &mut serde_json::Map<String, serde_json::Value>,
    ),
    (batch_minimums, batch_maximums): (
        serde_json::Map<String, serde_json::Value>,
        serde_json::Map<String, serde_json::Value>,
    ),
) {
    for (name, value) in batch_minimums {
        match minimums.get(&name) {
            Some(current) if !precedes(&value, current) => {}
            _ => {
                minimums.insert(name, value);
            }
        }
    }

    for (name, value) in batch_maximums {
        match maximums.get(&name) {
            Some(current) if !precedes(current, &value) => {}
            _ => {
                maximums.insert(name, value);
            }
        }
    }
}

/// Longest string bound kept whole, in bytes.
const MAX_BOUND_BYTES: usize = 64;

/// Shortens string bounds longer than [`MAX_BOUND_BYTES`] so that they still bound every value:
/// a minimum becomes its prefix, which is not greater than it; a maximum becomes its prefix with the
/// last character that can be raised raised by one, which is greater than every string starting
/// with that prefix. A maximum with no such character is dropped.
pub(super) fn shorten_bounds(
    minimums: &mut serde_json::Map<String, serde_json::Value>,
    maximums: &mut serde_json::Map<String, serde_json::Value>,
) {
    for value in minimums.values_mut() {
        if let serde_json::Value::String(text) = value
            && text.len() > MAX_BOUND_BYTES
        {
            text.truncate(prefix_end(text));
        }
    }

    maximums.retain(|_, value| {
        let serde_json::Value::String(text) = value else {
            return true;
        };

        if text.len() <= MAX_BOUND_BYTES {
            return true;
        }

        match raised_prefix(text) {
            Some(raised) => {
                *text = raised;
                true
            }
            None => false,
        }
    });
}

/// The end of the longest prefix of `text` within [`MAX_BOUND_BYTES`] that ends on a character.
fn prefix_end(text: &str) -> usize {
    (0..=MAX_BOUND_BYTES)
        .rev()
        .find(|end| text.is_char_boundary(*end))
        .unwrap_or(0)
}

/// The shortest string after every string that starts with the prefix of `text`: the prefix with
/// its last raisable character raised, the characters after it dropped.
fn raised_prefix(text: &str) -> Option<String> {
    let mut prefix = text[..prefix_end(text)].chars().collect::<Vec<_>>();

    while let Some(last) = prefix.pop() {
        let raised = (u32::from(last) + 1..=u32::from(char::MAX)).find_map(char::from_u32);

        if let Some(raised) = raised {
            prefix.push(raised);
            return Some(prefix.into_iter().collect());
        }
    }

    None
}

/// `left < right` for two zone-map scalars of one column. Scalars of different kinds never
/// precede each other, so the bound already recorded is kept.
fn precedes(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    use serde_json::Value;

    match (left, right) {
        (Value::Number(left), Value::Number(right)) => match (left.as_i64(), right.as_i64()) {
            (Some(left), Some(right)) => left < right,
            _ => match (left.as_f64(), right.as_f64()) {
                (Some(left), Some(right)) => left < right,
                _ => false,
            },
        },
        (Value::String(left), Value::String(right)) => left < right,
        (Value::Bool(left), Value::Bool(right)) => !left & right,
        _ => false,
    }
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
    fn long_string_bounds_are_shortened_and_still_bound_every_value() {
        let low = format!("{}é tail", "a".repeat(MAX_BOUND_BYTES - 1));
        let high = format!("{}z tail", "y".repeat(MAX_BOUND_BYTES));
        let mut minimums = serde_json::Map::from_iter([("message".to_string(), json!(low))]);
        let mut maximums = serde_json::Map::from_iter([("message".to_string(), json!(high))]);

        shorten_bounds(&mut minimums, &mut maximums);

        let minimum = minimums["message"].as_str().unwrap();
        let maximum = maximums["message"].as_str().unwrap();

        assert_eq!(minimum, "a".repeat(MAX_BOUND_BYTES - 1));
        assert!(minimum <= low.as_str());
        assert_eq!(maximum, format!("{}z", "y".repeat(MAX_BOUND_BYTES - 1)));
        assert!(maximum > high.as_str());

        let mut unbounded = serde_json::Map::from_iter([(
            "message".to_string(),
            json!(char::MAX.to_string().repeat(MAX_BOUND_BYTES)),
        )]);

        shorten_bounds(&mut serde_json::Map::new(), &mut unbounded);
        assert!(unbounded.is_empty());
    }

    #[test]
    fn non_finite_float_disables_zone_map_instead_of_writing_unsound_bounds() {
        let values = Float64Array::from(vec![1.0, f64::NAN]);
        assert_eq!(scalar_min(&values), None);
        assert_eq!(scalar_max(&values), None);
    }
}
