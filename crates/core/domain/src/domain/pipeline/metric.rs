// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A metric a realtime pipeline makes of the events a step picks (`generate_metric`), written into
//! a metrics stream the way every metric is stored there: `metric_name` and the labels as columns,
//! so PromQL, dashboards and alert rules read it like any other.
//!
//! - **Count:** one per event, or the number in a field of it.
//! - **Gauge:** the number in a field of the latest event of an interval.
//! - **Histogram:** how the number in a field spreads over upper bounds, as Prometheus stores a
//!   histogram (`<name>_bucket` with `le`, `<name>_count`, `<name>_sum`); `histogram_quantile`
//!   reads it.
//!
//! Each interval of a series is one sample at the interval's end, counted by the time the events
//! carry, not by when they arrive. Counts and histograms are deltas (`metric_temporality` is
//! `delta`), so `increase` and `rate` add up the samples of a window, also those that two nodes,
//! or a late event, wrote for the same interval.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    condition::{field_value, number_of},
    protected::{PROTECTED_METRIC_LABELS, PROTECTED_METRIC_VALUES},
    realtime::writable_stream,
};
use crate::{
    domain::metrics::is_metric_label_name,
    shared::{Error, Result},
};

/// The lengths of the intervals a metric may count over.
pub const INTERVALS_SECS: &[u32] = &[10, 30, 60, 300];
pub const DEFAULT_INTERVAL_SECS: u32 = 60;
pub const MAX_GROUP_BY: usize = 10;
pub const MAX_BUCKETS: usize = 30;
/// At most this many series a step keeps apart in one interval on one node; the events of the
/// others go into one series whose labels all read [`OTHER_SERIES`].
pub const MAX_SERIES: usize = 1_000;
pub const OTHER_SERIES: &str = "__other__";
/// A label value is cut to this many bytes.
pub const MAX_LABEL_VALUE_LENGTH: usize = 256;
const MAX_NAME_LENGTH: usize = 200;
const MAX_FIELD_LENGTH: usize = 255;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MetricType {
    Count,
    Gauge,
    Histogram,
}

/// What a `generate_metric` step makes.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricSpec {
    /// The metric's name, as PromQL selects it. A histogram's series add `_bucket`, `_count` and
    /// `_sum` to it.
    pub name: String,
    #[serde(rename = "type")]
    pub kind: MetricType,
    /// The field whose number a gauge or a histogram reads. A count adds it up when one is named,
    /// and counts events otherwise.
    #[serde(default)]
    pub field: String,
    /// Fields whose values become labels: one series per combination.
    #[serde(default)]
    pub group_by: Vec<String>,
    /// A histogram's upper bounds, ascending; `+Inf` is always there.
    #[serde(default)]
    pub buckets: Vec<f64>,
    /// The metrics stream the samples are written into; made when it does not exist.
    pub stream: String,
    #[serde(default = "default_interval")]
    pub interval_secs: u32,
}

fn default_interval() -> u32 {
    DEFAULT_INTERVAL_SECS
}

impl MetricSpec {
    pub fn validate(&self) -> Result<()> {
        if !is_metric_name(&self.name) {
            return Err(Error::invalid(format!(
                "`{}` is not a metric name: letters, digits, `_` and `:`, not starting with a digit",
                self.name
            )));
        }

        let field = self.field.trim();

        if field.len() > MAX_FIELD_LENGTH {
            return Err(Error::invalid("the field of a metric is too long"));
        }

        if field.is_empty() && self.kind != MetricType::Count {
            return Err(Error::invalid(format!(
                "a {} reads the number in a field: name the field",
                self.kind.name()
            )));
        }

        if self.group_by.len() > MAX_GROUP_BY {
            return Err(Error::invalid(format!(
                "a metric groups by at most {MAX_GROUP_BY} fields"
            )));
        }

        for (index, label) in self.group_by.iter().enumerate() {
            if !is_group_label(label) {
                return Err(Error::invalid(format!(
                    "`{label}` cannot be a label: the metrics stream stores it as part of every \
                     sample"
                )));
            }

            if self.group_by[..index].contains(label) {
                return Err(Error::invalid(format!("`{label}` is grouped by twice")));
            }
        }

        self.check_buckets()?;

        if !INTERVALS_SECS.contains(&self.interval_secs) {
            return Err(Error::invalid(
                "a metric counts over 10, 30, 60 or 300 seconds",
            ));
        }

        writable_stream(&self.stream)
    }

    fn check_buckets(&self) -> Result<()> {
        if self.kind != MetricType::Histogram {
            return if self.buckets.is_empty() {
                Ok(())
            } else {
                Err(Error::invalid("only a histogram has buckets"))
            };
        }

        if self.buckets.is_empty() || self.buckets.len() > MAX_BUCKETS {
            return Err(Error::invalid(format!(
                "a histogram has 1 to {MAX_BUCKETS} upper bounds"
            )));
        }

        let ascending = self.buckets.iter().all(|bound| bound.is_finite())
            && self.buckets.windows(2).all(|pair| pair[0] < pair[1]);

        if !ascending {
            return Err(Error::invalid(
                "the upper bounds of a histogram go up, each a finite number",
            ));
        }

        Ok(())
    }

    /// What one event adds to its series: 1 for a count without a field, else the number in the
    /// field. `None`: the event has no number there and adds nothing.
    pub fn measure(&self, event: &Value) -> Option<f64> {
        let field = self.field.trim();

        if field.is_empty() {
            return Some(1.0);
        }

        field_value(event, field).and_then(number_of)
    }

    /// The labels of the event's series, in the order of `group_by`. A field the event does not
    /// have gives no label.
    pub fn labels(&self, event: &Value) -> Vec<(String, String)> {
        self.group_by
            .iter()
            .filter_map(|label| {
                let value = field_value(event, label).and_then(label_value)?;
                Some((label.clone(), value))
            })
            .collect()
    }

    /// The labels of the series that takes the events of every series past [`MAX_SERIES`].
    pub fn other_labels(&self) -> Vec<(String, String)> {
        self.group_by
            .iter()
            .map(|label| (label.clone(), OTHER_SERIES.to_owned()))
            .collect()
    }

    /// Whether the samples only go up: a count of events, and a histogram's buckets and count. A
    /// count of a field's numbers can go down when the numbers are negative.
    pub fn counts_events(&self) -> bool {
        self.kind == MetricType::Count && self.field.trim().is_empty()
    }
}

impl MetricType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Gauge => "gauge",
            Self::Histogram => "histogram",
        }
    }
}

/// A label value made of a field's value: text as it is, a number or a boolean as written, an
/// object or a list as JSON. Cut to [`MAX_LABEL_VALUE_LENGTH`] bytes; `null` gives none.
pub fn label_value(value: &Value) -> Option<String> {
    let text = match value {
        Value::Null => return None,
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };

    Some(cut(text, MAX_LABEL_VALUE_LENGTH))
}

/// `text` cut to at most `max` bytes, at a character boundary.
pub fn cut(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut end = max;

        while !text.is_char_boundary(end) {
            end -= 1;
        }

        text.truncate(end);
    }

    text
}

/// A name PromQL selects without quotes.
fn is_metric_name(name: &str) -> bool {
    let mut chars = name.chars();

    name.len() <= MAX_NAME_LENGTH
        && chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || matches!(first, '_' | ':'))
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':'))
}

/// A field a sample can carry as a label: not one of the columns every sample has, not one that
/// says which bucket or quantile it is, and not one Prometheus keeps for itself (`__…`).
fn is_group_label(label: &str) -> bool {
    !label.trim().is_empty()
        && label.trim() == label
        && label.len() <= MAX_FIELD_LENGTH
        && !label.starts_with("__")
        && is_metric_label_name(label)
        && !PROTECTED_METRIC_LABELS.contains(&label)
        && !PROTECTED_METRIC_VALUES.contains(&label)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn spec(value: Value) -> Result<MetricSpec> {
        let spec: MetricSpec =
            serde_json::from_value(value).map_err(|e| Error::invalid(e.to_string()))?;
        spec.validate().map(|()| spec)
    }

    #[test]
    fn a_count_needs_only_a_name_and_a_stream() {
        let count =
            spec(json!({ "name": "app_errors", "type": "count", "stream": "app" })).unwrap();

        assert_eq!(count.interval_secs, DEFAULT_INTERVAL_SECS);
        assert_eq!(count.measure(&json!({})), Some(1.0));
        assert!(count.counts_events());

        let bytes = spec(json!({
            "name": "app_bytes", "type": "count", "field": "bytes", "stream": "app",
        }))
        .unwrap();

        assert_eq!(bytes.measure(&json!({ "bytes": "512" })), Some(512.0));
        assert_eq!(bytes.measure(&json!({ "bytes": "lots" })), None);
        assert!(!bytes.counts_events());
    }

    #[test]
    fn a_gauge_or_a_histogram_reads_a_field() {
        assert!(spec(json!({ "name": "queue", "type": "gauge", "stream": "app" })).is_err());
        assert!(
            spec(json!({
                "name": "latency_ms", "type": "histogram", "field": "duration_ms", "stream": "app",
            }))
            .is_err(),
            "a histogram needs its upper bounds"
        );

        let latency = spec(json!({
            "name": "latency_ms", "type": "histogram", "field": "duration_ms",
            "buckets": [10, 50, 250], "stream": "app", "intervalSecs": 10,
        }))
        .unwrap();

        assert_eq!(latency.measure(&json!({ "duration_ms": 42 })), Some(42.0));

        for buckets in [json!([50, 10]), json!([10, 10]), json!([])] {
            assert!(
                spec(json!({
                    "name": "latency_ms", "type": "histogram", "field": "d",
                    "buckets": buckets, "stream": "app",
                }))
                .is_err()
            );
        }

        assert!(
            spec(json!({ "name": "c", "type": "count", "buckets": [1], "stream": "app" })).is_err()
        );
    }

    #[test]
    fn names_labels_streams_and_intervals_are_checked() {
        for wrong in [
            json!({ "name": "5xx", "type": "count", "stream": "app" }),
            json!({ "name": "http.errors", "type": "count", "stream": "app" }),
            json!({ "name": "c", "type": "count", "stream": "_molesignal" }),
            json!({ "name": "c", "type": "count", "stream": "app", "intervalSecs": 15 }),
            json!({ "name": "c", "type": "count", "stream": "app", "groupBy": ["le"] }),
            json!({ "name": "c", "type": "count", "stream": "app", "groupBy": ["value"] }),
            json!({ "name": "c", "type": "count", "stream": "app", "groupBy": ["__name__"] }),
            json!({ "name": "c", "type": "count", "stream": "app", "groupBy": ["env", "env"] }),
            json!({ "name": "c", "type": "summary", "stream": "app" }),
        ] {
            assert!(spec(wrong.clone()).is_err(), "{wrong}");
        }

        let grouped = spec(json!({
            "name": "http:requests", "type": "count", "stream": "app",
            "groupBy": ["service.name", "status"],
        }))
        .unwrap();

        assert_eq!(
            grouped.labels(&json!({ "service": { "name": "cart" }, "status": 503, "x": 1 })),
            vec![
                ("service.name".to_owned(), "cart".to_owned()),
                ("status".to_owned(), "503".to_owned())
            ]
        );
        assert_eq!(
            grouped.labels(&json!({ "status": null })),
            Vec::<(String, String)>::new()
        );
        assert_eq!(grouped.other_labels()[1].1, OTHER_SERIES);
    }

    #[test]
    fn label_values_are_text_cut_at_a_character() {
        assert_eq!(label_value(&json!(true)).as_deref(), Some("true"));
        assert_eq!(
            label_value(&json!({ "a": 1 })).as_deref(),
            Some("{\"a\":1}")
        );
        assert_eq!(label_value(&json!(null)), None);

        let long = "é".repeat(200);
        let value = label_value(&json!(long)).unwrap();

        assert!(value.len() <= MAX_LABEL_VALUE_LENGTH);
        assert!(value.chars().all(|c| c == 'é'));
    }
}
