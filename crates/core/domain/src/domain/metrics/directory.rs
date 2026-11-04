// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which metrics each metrics stream holds.
//!
//! A metrics stream holds the metrics of one source, told apart by `metric_name`. Intake records
//! every metric it sees in a stream, with the hours it was seen in, its kind and its label names,
//! so a query finds the streams holding a metric without opening their files.

use std::{collections::BTreeMap, sync::Mutex};

use async_trait::async_trait;

use crate::{
    domain::stream::StreamDefinition,
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// One metric of one stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricDirectoryEntry {
    pub stream_id: Id,
    pub stream_name: String,
    pub metric_name: String,
    /// `metric_kind` of the samples, when they carry one (OTLP and self telemetry).
    pub metric_kind: Option<String>,
    /// Label names seen on the metric's samples, sorted.
    pub labels: Vec<String>,
    /// Start of the first hour the metric was seen in.
    pub first_seen: TimestampMicros,
    /// End of the last hour the metric was seen in.
    pub last_seen: TimestampMicros,
}

impl MetricDirectoryEntry {
    /// Whether the metric was seen in an hour that `time_range` touches.
    pub fn overlaps(&self, time_range: TimeRange) -> bool {
        self.first_seen <= time_range.end && self.last_seen >= time_range.start
    }
}

/// What intake saw of one metric in one batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricObservation {
    pub metric_name: String,
    pub metric_kind: Option<String>,
    /// Sorted label names.
    pub labels: Vec<String>,
    pub first_seen: TimestampMicros,
    pub last_seen: TimestampMicros,
}

#[async_trait]
pub trait MetricDirectory: Send + Sync {
    /// Records what intake saw in `stream`. A known metric keeps the widest hours, the latest
    /// kind and every label it has carried.
    async fn record(
        &self,
        stream: &StreamDefinition,
        observations: &[MetricObservation],
    ) -> Result<()>;

    /// Every metric of the organization, ordered by name and then stream.
    async fn list(&self, org_id: &Id) -> Result<Vec<MetricDirectoryEntry>>;

    /// The streams holding `metric_name`, by stream name.
    async fn find(&self, org_id: &Id, metric_name: &str) -> Result<Vec<MetricDirectoryEntry>>;

    /// Forgets what retention removed from `stream`: a metric last seen before `cutoff` is
    /// dropped, and the others are taken to start no earlier than it. Returns the number dropped.
    async fn forget_before(
        &self,
        stream: &StreamDefinition,
        cutoff: TimestampMicros,
    ) -> Result<u64>;
}

/// The directory in process memory, for tests.
#[derive(Default)]
pub struct MemoryMetricDirectory {
    /// By organization, metric and stream name.
    entries: Mutex<BTreeMap<(String, String, String), MetricDirectoryEntry>>,
}

#[async_trait]
impl MetricDirectory for MemoryMetricDirectory {
    async fn record(
        &self,
        stream: &StreamDefinition,
        observations: &[MetricObservation],
    ) -> Result<()> {
        let mut entries = self.entries.lock().expect("metric directory lock");

        for observation in observations {
            let key = (
                stream.org_id.0.clone(),
                observation.metric_name.clone(),
                stream.name.clone(),
            );

            let entry = entries.entry(key).or_insert_with(|| MetricDirectoryEntry {
                stream_id: stream.id.clone(),
                stream_name: stream.name.clone(),
                metric_name: observation.metric_name.clone(),
                metric_kind: None,
                labels: Vec::new(),
                first_seen: observation.first_seen,
                last_seen: observation.last_seen,
            });

            if observation.metric_kind.is_some() {
                entry.metric_kind.clone_from(&observation.metric_kind);
            }

            entry.labels.extend(observation.labels.iter().cloned());
            entry.labels.sort();
            entry.labels.dedup();
            entry.first_seen = entry.first_seen.min(observation.first_seen);
            entry.last_seen = entry.last_seen.max(observation.last_seen);
        }

        Ok(())
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<MetricDirectoryEntry>> {
        let entries = self.entries.lock().expect("metric directory lock");

        Ok(entries
            .iter()
            .filter(|((org, _, _), _)| *org == org_id.0)
            .map(|(_, entry)| entry.clone())
            .collect())
    }

    async fn find(&self, org_id: &Id, metric_name: &str) -> Result<Vec<MetricDirectoryEntry>> {
        let entries = self.entries.lock().expect("metric directory lock");

        Ok(entries
            .iter()
            .filter(|((org, metric, _), _)| *org == org_id.0 && metric == metric_name)
            .map(|(_, entry)| entry.clone())
            .collect())
    }

    async fn forget_before(
        &self,
        stream: &StreamDefinition,
        cutoff: TimestampMicros,
    ) -> Result<u64> {
        let mut entries = self.entries.lock().expect("metric directory lock");
        let before = entries.len();

        entries.retain(|_, entry| entry.stream_id != stream.id || entry.last_seen > cutoff);

        for entry in entries
            .values_mut()
            .filter(|entry| entry.stream_id == stream.id)
        {
            entry.first_seen = entry.first_seen.max(cutoff);
        }

        Ok((before - entries.len()) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::stream::{Schema, StreamType};

    fn stream(name: &str) -> StreamDefinition {
        StreamDefinition {
            id: Id::from_string(format!("id-{name}")),
            org_id: Id::from_string("org-a"),
            name: name.into(),
            stream_type: StreamType::METRICS,
            schema: Schema { fields: Vec::new() },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn seen(metric: &str, labels: &[&str], hours: (i64, i64)) -> MetricObservation {
        MetricObservation {
            metric_name: metric.into(),
            metric_kind: None,
            labels: labels.iter().map(|label| label.to_string()).collect(),
            first_seen: TimestampMicros(hours.0 * 3_600_000_000),
            last_seen: TimestampMicros(hours.1 * 3_600_000_000),
        }
    }

    #[tokio::test]
    async fn a_known_metric_widens_its_hours_and_gains_labels() {
        let directory = MemoryMetricDirectory::default();
        let prod = stream("prod");

        directory
            .record(&prod, &[seen("up", &["job"], (5, 6))])
            .await
            .unwrap();

        directory
            .record(&prod, &[seen("up", &["instance", "job"], (2, 3))])
            .await
            .unwrap();

        let entries = directory.list(&prod.org_id).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].labels, ["instance", "job"]);
        assert_eq!(entries[0].first_seen, TimestampMicros(2 * 3_600_000_000));
        assert_eq!(entries[0].last_seen, TimestampMicros(6 * 3_600_000_000));
    }

    #[tokio::test]
    async fn find_returns_the_streams_holding_the_metric() {
        let directory = MemoryMetricDirectory::default();

        directory
            .record(&stream("prod"), &[seen("up", &[], (0, 2))])
            .await
            .unwrap();

        directory
            .record(&stream("staging"), &[seen("up", &[], (10, 11))])
            .await
            .unwrap();

        let hour = |hour: i64| TimestampMicros(hour * 3_600_000_000);
        let org = Id::from_string("org-a");

        let found = directory.find(&org, "up").await.unwrap();

        assert_eq!(
            found
                .iter()
                .map(|entry| entry.stream_name.as_str())
                .collect::<Vec<_>>(),
            ["prod", "staging"]
        );

        let early = TimeRange::new(hour(1), hour(3));

        assert_eq!(
            found
                .iter()
                .filter(|entry| entry.overlaps(early))
                .map(|entry| entry.stream_name.as_str())
                .collect::<Vec<_>>(),
            ["prod"]
        );

        assert!(directory.find(&org, "down").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn retention_drops_metrics_last_seen_before_the_cutoff() {
        let directory = MemoryMetricDirectory::default();
        let prod = stream("prod");
        let staging = stream("staging");

        directory
            .record(
                &prod,
                &[seen("gone", &[], (0, 2)), seen("kept", &[], (1, 30))],
            )
            .await
            .unwrap();

        directory
            .record(&staging, &[seen("gone", &[], (0, 2))])
            .await
            .unwrap();

        let cutoff = TimestampMicros(10 * 3_600_000_000);
        assert_eq!(directory.forget_before(&prod, cutoff).await.unwrap(), 1);

        let org = Id::from_string("org-a");
        let gone = directory.find(&org, "gone").await.unwrap();
        assert_eq!(gone.len(), 1, "another stream keeps its own entries");
        assert_eq!(gone[0].stream_name, "staging");

        let kept = directory.find(&org, "kept").await.unwrap();
        assert_eq!(kept[0].first_seen, cutoff);
    }
}
