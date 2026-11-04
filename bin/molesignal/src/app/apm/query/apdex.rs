// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The Apdex threshold T each service is read with: the one people set in
//! its definition, otherwise the organization's default. A definition holds
//! in every environment of its service, so T does too.

use std::collections::HashMap;

use super::{ApmQueryService, aggregate::DEFAULT_APDEX_THRESHOLD_MICROS};
use crate::{
    domain::apm::ServiceIdentity,
    shared::{Result, ids::Id},
};

/// The thresholds people set, by service namespace, then name, and the one
/// of a service that sets none.
#[derive(Debug, Clone)]
pub(super) struct ApdexThresholds {
    set: HashMap<String, HashMap<String, u64>>,
    default: u64,
}

impl Default for ApdexThresholds {
    fn default() -> Self {
        Self {
            set: HashMap::new(),
            default: DEFAULT_APDEX_THRESHOLD_MICROS,
        }
    }
}

impl ApdexThresholds {
    fn with_default(self, default: u64) -> Self {
        Self { default, ..self }
    }

    /// The threshold of a service that sets none.
    pub(super) fn default_micros(&self) -> u64 {
        self.default
    }

    pub(super) fn of(&self, service: &ServiceIdentity) -> u64 {
        self.named(Some(&service.namespace), &service.name)
    }

    /// The threshold of a service named without its namespace too: the one
    /// set for that name when only one namespace sets one.
    pub(super) fn named(&self, namespace: Option<&str>, name: &str) -> u64 {
        let set = match namespace {
            Some(namespace) => self
                .set
                .get(namespace)
                .and_then(|names| names.get(name))
                .copied(),
            None => {
                let mut found = self.set.values().filter_map(|names| names.get(name));
                found.next().filter(|_| found.next().is_none()).copied()
            }
        };

        set.unwrap_or(self.default)
    }
}

impl FromIterator<(String, String, u64)> for ApdexThresholds {
    fn from_iter<I: IntoIterator<Item = (String, String, u64)>>(entries: I) -> Self {
        let mut thresholds = HashMap::<String, HashMap<String, u64>>::new();

        for (namespace, name, micros) in entries {
            thresholds
                .entry(namespace)
                .or_default()
                .insert(name, micros);
        }

        Self {
            set: thresholds,
            ..Self::default()
        }
    }
}

impl ApmQueryService {
    /// The thresholds the organization's services set, none without a
    /// catalog, and the organization's default.
    pub(super) async fn apdex_thresholds(&self, org_id: &Id) -> Result<ApdexThresholds> {
        let default = self.org_settings(org_id).await.apdex_threshold;
        let Some(catalog) = &self.catalog else {
            return Ok(ApdexThresholds::default().with_default(default));
        };

        Ok(catalog
            .list_definitions(org_id)
            .await?
            .into_iter()
            .filter_map(|definition| {
                definition
                    .apdex_threshold
                    .map(|micros| (definition.namespace, definition.name, micros))
            })
            .collect::<ApdexThresholds>()
            .with_default(default))
    }
}

#[cfg(test)]
mod tests {
    use super::{super::ranking::transaction_summaries, *};
    use crate::{
        app::apm::ApmQueryContext,
        domain::apm::{
            BucketDimension, BucketMeasurements, HistogramSchema, LatencyHistogram, MergedBucket,
            QueryResolution, SortDirection, TransactionIdentity, TransactionKind,
        },
        shared::time::{TimeRange, TimestampMicros},
    };

    #[test]
    fn a_service_is_read_with_its_own_threshold_or_the_default() {
        let thresholds = [
            ("shop".to_owned(), "checkout".to_owned(), 300_000),
            ("shop".to_owned(), "search".to_owned(), 100_000),
            ("ops".to_owned(), "search".to_owned(), 2_000_000),
        ]
        .into_iter()
        .collect::<ApdexThresholds>();
        let checkout = ServiceIdentity {
            namespace: "shop".into(),
            name: "checkout".into(),
            environment: "prod".into(),
        };

        assert_eq!(thresholds.of(&checkout), 300_000);
        assert_eq!(thresholds.named(None, "checkout"), 300_000);
        assert_eq!(thresholds.named(Some("ops"), "search"), 2_000_000);
        // Two namespaces set one for `search`: without the namespace, neither is meant.
        assert_eq!(
            thresholds.named(None, "search"),
            DEFAULT_APDEX_THRESHOLD_MICROS
        );
        assert_eq!(
            thresholds.named(Some("shop"), "cart"),
            DEFAULT_APDEX_THRESHOLD_MICROS
        );

        // The organization's default stands for a service that sets none.
        let thresholds = thresholds.with_default(1_000_000);
        assert_eq!(thresholds.named(Some("shop"), "cart"), 1_000_000);
        assert_eq!(thresholds.of(&checkout), 300_000);
    }

    #[test]
    fn endpoints_are_read_with_the_threshold_of_their_service() {
        let schema = HistogramSchema::v1();
        let mut latency = LatencyHistogram::empty(&schema);
        latency.observe(&schema, 400_000).expect("observe");
        let bucket = |name: &str| MergedBucket {
            bucket_at: TimestampMicros(0),
            dimension: BucketDimension::Transaction {
                service: ServiceIdentity::new(Some("shop"), Some(name), Some("prod"), None),
                version: None,
                transaction: TransactionIdentity {
                    name: "GET /".into(),
                    kind: TransactionKind::Http,
                },
            },
            measurements: BucketMeasurements {
                request_count: 1,
                error_count: 0,
                overflow_count: 0,
                latency: latency.clone(),
                exemplars: Vec::new(),
                status_codes: Default::default(),
                timeout_count: 0,
                retry_count: 0,
            },
        };
        let context = ApmQueryContext {
            org_id: Id::from_string("org-a"),
            range: TimeRange::new(TimestampMicros(0), TimestampMicros(60_000_000)),
            namespace: None,
            service_name: None,
            environment: None,
            version: None,
            resolution: QueryResolution::Minute,
            sort: "request_count".into(),
            direction: SortDirection::Desc,
            limit: 50,
            cursor: None,
        };
        let thresholds = [("shop".to_owned(), "checkout".to_owned(), 300_000)]
            .into_iter()
            .collect::<ApdexThresholds>();

        let summaries = transaction_summaries(
            &context,
            &[bucket("checkout"), bucket("search")],
            &schema,
            &thresholds,
        )
        .expect("summaries");
        let read = |name: &str| {
            summaries
                .iter()
                .find(|summary| summary.service.name == name)
                .map(|summary| (summary.apdex_threshold, summary.red.apdex))
        };

        // 400 ms is past checkout's 300 ms (tolerated) and within the default 500 ms.
        assert_eq!(read("checkout"), Some((300_000, Some(0.5))));
        assert_eq!(
            read("search"),
            Some((DEFAULT_APDEX_THRESHOLD_MICROS, Some(1.0)))
        );
    }
}
