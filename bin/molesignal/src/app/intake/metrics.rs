// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Metric samples at intake. A metrics stream holds the metrics of one source: every sample names
//! its metric in `metric_name`, and the metric directory learns which metrics each stream holds.

use std::{
    collections::{BTreeSet, HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use moka::sync::Cache;
use serde_json::Value;

use crate::{
    domain::{
        intake::RawEvent,
        metrics::{
            METRIC_KIND_FIELD, METRIC_NAME_FIELD, MetricDirectory, MetricObservation,
            is_metric_label_name,
        },
        stream::StreamDefinition,
    },
    shared::time::TimestampMicros,
};

const HOUR_MICROS: i64 = 3_600 * 1_000_000;

/// Metric hours one process remembers having recorded. Past it the oldest are recorded again,
/// which costs one more upsert each.
const MAX_RECORDED_HOURS: u64 = 200_000;

/// Gives every sample a metric: one without a `metric_name`, or with an empty one, belongs to
/// the metric named after its stream, and a name that is not text is stored as its JSON text.
pub(super) fn name_samples(stream: &str, events: &mut [RawEvent]) {
    for event in events {
        let name = match event.fields.get(METRIC_NAME_FIELD) {
            Some(Value::String(name)) if !name.is_empty() => continue,
            None | Some(Value::Null) | Some(Value::String(_)) => stream.to_string(),
            Some(other) => other.to_string(),
        };

        event
            .fields
            .insert(METRIC_NAME_FIELD.into(), Value::String(name));
    }
}

/// Records in the metric directory the metrics that intake writes, each metric at most once an
/// hour per stream and process.
pub struct MetricRecorder {
    directory: Arc<dyn MetricDirectory>,
    /// Hashes of the (organization, stream, metric, hour) already recorded.
    recorded: Cache<u64, ()>,
}

impl MetricRecorder {
    pub fn new(directory: Arc<dyn MetricDirectory>) -> Self {
        Self {
            directory,
            recorded: Cache::builder()
                .max_capacity(MAX_RECORDED_HOURS)
                .time_to_live(Duration::from_secs(2 * 3_600))
                .build(),
        }
    }

    /// What `events` add to the directory: the metrics and hours this process has not recorded
    /// for `stream` yet. Label names are collected only for those.
    pub(super) fn pending<'a>(
        &'a self,
        stream: &'a StreamDefinition,
        events: &[RawEvent],
    ) -> Option<PendingMetrics<'a>> {
        let mut hours = HashMap::<&str, BTreeSet<i64>>::new();

        for event in events {
            if let Some(name) = event.fields.get(METRIC_NAME_FIELD).and_then(Value::as_str) {
                let hour = event.timestamp.0.div_euclid(HOUR_MICROS) * HOUR_MICROS;
                hours.entry(name).or_default().insert(hour);
            }
        }

        let mut keys = Vec::new();
        let mut unrecorded = HashMap::<&str, BTreeSet<i64>>::new();

        for (name, hours) in hours {
            for hour in hours {
                let key = hour_key(stream, name, hour);

                if !self.recorded.contains_key(&key) {
                    keys.push(key);
                    unrecorded.entry(name).or_default().insert(hour);
                }
            }
        }

        if unrecorded.is_empty() {
            return None;
        }

        let observations = observe(&unrecorded, events);

        Some(PendingMetrics {
            recorder: self,
            stream,
            observations,
            keys,
        })
    }
}

/// Metrics and hours to record once the batch is written.
pub(super) struct PendingMetrics<'a> {
    recorder: &'a MetricRecorder,
    stream: &'a StreamDefinition,
    observations: Vec<MetricObservation>,
    keys: Vec<u64>,
}

impl PendingMetrics<'_> {
    /// Writes the observations. A failure is logged, and the hours are tried again with the
    /// next batch that carries them.
    pub(super) async fn record(self) {
        match self
            .recorder
            .directory
            .record(self.stream, &self.observations)
            .await
        {
            Ok(()) => {
                for key in self.keys {
                    self.recorder.recorded.insert(key, ());
                }
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    stream = %self.stream.name,
                    "metric directory record failed; retrying with the next batch"
                );
            }
        }
    }
}

/// One observation per metric in `unrecorded`, covering its unrecorded hours, with the kind and
/// label names its samples in `events` carry.
fn observe(
    unrecorded: &HashMap<&str, BTreeSet<i64>>,
    events: &[RawEvent],
) -> Vec<MetricObservation> {
    let mut seen = HashMap::<&str, (Option<&str>, BTreeSet<&str>)>::new();

    for event in events {
        let Some(name) = event.fields.get(METRIC_NAME_FIELD).and_then(Value::as_str) else {
            continue;
        };

        if !unrecorded.contains_key(name) {
            continue;
        }

        let (kind, labels) = seen.entry(name).or_default();

        if kind.is_none() {
            *kind = event.fields.get(METRIC_KIND_FIELD).and_then(Value::as_str);
        }

        labels.extend(
            event
                .fields
                .iter()
                .filter(|(field, value)| value.is_string() && is_metric_label_name(field))
                .map(|(field, _)| field.as_str()),
        );
    }

    let mut observations = unrecorded
        .iter()
        .map(|(name, hours)| {
            let (kind, labels) = seen.remove(name).unwrap_or_default();
            let first = hours.first().copied().unwrap_or_default();
            let last = hours.last().copied().unwrap_or_default();

            MetricObservation {
                metric_name: (*name).to_string(),
                metric_kind: kind.map(str::to_string),
                labels: labels.into_iter().map(str::to_string).collect(),
                first_seen: TimestampMicros(first),
                last_seen: TimestampMicros(last.saturating_add(HOUR_MICROS)),
            }
        })
        .collect::<Vec<_>>();

    observations.sort_by(|left, right| left.metric_name.cmp(&right.metric_name));
    observations
}

fn hour_key(stream: &StreamDefinition, metric: &str, hour: i64) -> u64 {
    let mut hasher = DefaultHasher::new();
    stream.org_id.0.hash(&mut hasher);
    stream.id.0.hash(&mut hasher);
    metric.hash(&mut hasher);
    hour.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        domain::{
            metrics::MemoryMetricDirectory,
            stream::{Schema, StreamType},
        },
        shared::ids::Id,
    };

    fn stream() -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string("stream-prod"),
            org_id: Id::from_string("org-a"),
            name: "prod".into(),
            stream_type: StreamType::METRICS,
            schema: Schema { fields: Vec::new() },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn sample(hour: i64, fields: serde_json::Value) -> RawEvent {
        RawEvent {
            timestamp: TimestampMicros(hour * HOUR_MICROS + 1),
            fields: fields.as_object().unwrap().clone(),
        }
    }

    #[test]
    fn unnamed_samples_take_the_stream_name() {
        let mut events = vec![
            sample(0, json!({ "value": 1 })),
            sample(0, json!({ "metric_name": "", "value": 1 })),
            sample(0, json!({ "metric_name": 7, "value": 1 })),
            sample(0, json!({ "metric_name": "up", "value": 1 })),
        ];

        name_samples("prod", &mut events);

        let names = events
            .iter()
            .map(|event| event.fields[METRIC_NAME_FIELD].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(names, ["prod", "prod", "7", "up"]);
    }

    #[tokio::test]
    async fn each_metric_hour_is_recorded_once() {
        let directory = Arc::new(MemoryMetricDirectory::default());
        let recorder = MetricRecorder::new(directory.clone());
        let stream = stream();

        let events = vec![
            sample(4, json!({ "metric_name": "up", "job": "api", "value": 1 })),
            sample(
                5,
                json!({ "metric_name": "up", "instance": "a", "value": 1 }),
            ),
            sample(
                5,
                json!({ "metric_name": "cpu", "metric_kind": "gauge", "value": 0.5 }),
            ),
        ];

        let pending = recorder.pending(&stream, &events).unwrap();
        assert_eq!(pending.observations.len(), 2);
        pending.record().await;

        assert!(recorder.pending(&stream, &events).is_none());

        let entries = directory.list(&stream.org_id).await.unwrap();
        let up = entries
            .iter()
            .find(|entry| entry.metric_name == "up")
            .unwrap();
        assert_eq!(up.labels, ["instance", "job"]);
        assert_eq!(up.first_seen, TimestampMicros(4 * HOUR_MICROS));
        assert_eq!(up.last_seen, TimestampMicros(6 * HOUR_MICROS));

        let cpu = entries
            .iter()
            .find(|entry| entry.metric_name == "cpu")
            .unwrap();
        assert_eq!(cpu.metric_kind.as_deref(), Some("gauge"));

        let later = vec![sample(6, json!({ "metric_name": "up", "value": 1 }))];
        let pending = recorder.pending(&stream, &later).unwrap();
        assert_eq!(
            pending.observations[0].first_seen,
            TimestampMicros(6 * HOUR_MICROS)
        );
    }
}
