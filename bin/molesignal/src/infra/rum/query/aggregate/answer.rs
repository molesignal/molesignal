// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an aggregate came to: the groups it is of, the numbers it has for each, and why it has none.

use serde::Serialize;
use serde_json::Value;

use super::super::histogram::Unavailable;

/// The values of a row's groups, text for what has one.
pub(super) fn group_of(positions: &[usize], row: &[Value]) -> Vec<Option<String>> {
    positions
        .iter()
        .map(|at| match row.get(*at) {
            Some(Value::String(text)) => Some(text.clone()),
            Some(Value::Null) | None => None,
            Some(other) => Some(other.to_string()),
        })
        .collect()
}

/// What a measure of the answer is.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct MeasureInfo {
    pub name: String,
    #[serde(rename = "fn")]
    pub function: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// `count`, or the unit of the field: `ns`, `ms`, `bytes`, `score` and so on.
    pub unit: &'static str,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// What the row is of, in the order of `group_by`; `None` where the rows have no value.
    pub group: Vec<Option<String>>,
    /// Where the bar of the row begins, in microseconds, in an answer over time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// In the order of the measures. A value the reader may not see is the text it is masked to.
    pub values: Vec<Value>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Aggregate {
    pub group_by: Vec<String>,
    pub measures: Vec<MeasureInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_time: Option<i64>,
    pub rows: Vec<Row>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<Unavailable>,
}
