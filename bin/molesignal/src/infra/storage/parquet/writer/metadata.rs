// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! File-level time bounds, zone maps, and canonical object keys.

use arrow::{
    array::{Array, RecordBatch},
    compute::{max as array_max, min as array_min},
    datatypes::{DataType, TimeUnit},
};

use crate::{
    domain::{
        storage::{PhysicalDatasetKind, hour_partition_path},
        stream::{StreamDefinition, StreamType},
    },
    infra::storage::arrow_schema::TS_COL,
    shared::{
        Error, Result,
        ids::Id,
        time::TimestampMicros,
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
    for field in stream
        .schema
        .fields
        .iter()
        .filter(|field| field.indexed || sort_fields.contains(&field.name.as_str()))
    {
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
    use arrow::array::{Int64Array, StringArray};

    if let Some(values) = array.as_any().downcast_ref::<Int64Array>() {
        return array_min(values).map(serde_json::Value::from);
    }
    array
        .as_any()
        .downcast_ref::<StringArray>()?
        .iter()
        .flatten()
        .min()
        .map(|value| serde_json::Value::from(value.to_string()))
}

fn scalar_max(array: &dyn Array) -> Option<serde_json::Value> {
    use arrow::array::{Int64Array, StringArray};

    if let Some(values) = array.as_any().downcast_ref::<Int64Array>() {
        return array_max(values).map(serde_json::Value::from);
    }
    array
        .as_any()
        .downcast_ref::<StringArray>()?
        .iter()
        .flatten()
        .max()
        .map(|value| serde_json::Value::from(value.to_string()))
}

/// `{org}/{stream_type}/{dataset_kind}/{stream}/{YYYY}/{MM}/{DD}/{HH}/{id}.parquet`.
pub(super) fn object_key(
    org_id: &Id,
    stream: &str,
    stream_type: StreamType,
    dataset_kind: PhysicalDatasetKind,
    start_micros: i64,
) -> String {
    let partition = hour_partition_path(TimestampMicros(start_micros));
    format!(
        "{}/{}/{}/{}/{}/{}.parquet",
        org_id.0,
        stream_type_directory(stream_type),
        dataset_kind.as_str(),
        stream,
        partition,
        Id::new().0
    )
}

fn stream_type_directory(stream_type: StreamType) -> &'static str {
    match stream_type {
        StreamType::Logs => "logs",
        StreamType::Metrics => "metrics",
        StreamType::Traces => "traces",
        StreamType::Profiles => "profiles",
        StreamType::Extend => "extend",
    }
}
