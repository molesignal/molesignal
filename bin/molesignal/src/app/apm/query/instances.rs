// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The instances of a service, each with what it says about itself (its
//! version, where and how it runs) and what it served within the range: its
//! requests, errors and latency, from the buckets kept per instance. During a
//! rollout the instances of each version sit side by side; past the instance
//! limit the rest are counted together in the overflow row.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use super::{
    ApmQueryContext, ApmQueryService, RedSparkPoint, RedSummary,
    aggregate::{red_from_buckets_at, sparkline_from_buckets},
};
use crate::{
    domain::apm::{
        BucketDimension, BucketKind, InstanceObservation, MergedBucket, ServiceIdentity,
    },
    shared::{Error, Result},
};

/// The most instances a service's list holds.
const MAX_INSTANCES: usize = 500;

/// An instance as its service's list shows it.
#[derive(Debug, Clone, Serialize)]
pub struct InstanceSummary {
    #[serde(flatten)]
    pub instance: InstanceObservation,
    /// What the instance served within the range, Apdex at its service's threshold.
    pub red: RedSummary,
    pub sparkline: Vec<RedSparkPoint>,
}

impl ApmQueryService {
    /// The instances of one service in one environment within the range, the
    /// most recently seen first, of the version in view when one is, each with
    /// what it served.
    pub async fn service_instances(
        &self,
        context: &ApmQueryContext,
    ) -> Result<Vec<InstanceSummary>> {
        let (Some(name), Some(_)) = (&context.service_name, &context.environment) else {
            return Err(Error::invalid(
                "instances are listed for one service in one environment",
            ));
        };

        let instance_query = self.bucket_query(context, BucketKind::Instance);
        let catalog_query = self.catalog_query(context);
        let (observations, buckets, thresholds) = tokio::try_join!(
            self.repository.list_instances(&catalog_query),
            self.repository.query_buckets(&instance_query),
            self.apdex_thresholds(&context.org_id),
        )?;

        let threshold = thresholds.named(context.namespace.as_deref(), name);
        let mut served = HashMap::<String, Vec<MergedBucket>>::new();

        for bucket in buckets {
            if let BucketDimension::Instance { instance_id, .. } = &bucket.dimension {
                served.entry(instance_id.clone()).or_default().push(bucket);
            }
        }

        let mut instances = observations
            .into_iter()
            .filter(|instance| {
                context
                    .version
                    .as_deref()
                    .is_none_or(|wanted| instance.version.as_deref() == Some(wanted))
            })
            .map(|instance| {
                let buckets = served.remove(&instance.instance_id).unwrap_or_default();
                (instance, buckets)
            })
            .collect::<Vec<_>>();

        // Instances that served in the range before anything was kept about them,
        // and the overflow row, are listed by what their buckets say.
        for (instance_id, buckets) in served {
            instances.push((seen_in(context, name, &instance_id, &buckets), buckets));
        }

        let mut summaries = instances
            .into_iter()
            .map(|(instance, buckets)| {
                Ok(InstanceSummary {
                    instance,
                    red: red_from_buckets_at(&buckets, &self.config.histogram, threshold)?,
                    sparkline: sparkline_from_buckets(&buckets, &self.config.histogram)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        summaries.sort_by(|left, right| {
            right
                .instance
                .last_seen_at
                .cmp(&left.instance.last_seen_at)
                .then_with(|| left.instance.instance_id.cmp(&right.instance.instance_id))
        });
        summaries.truncate(MAX_INSTANCES);

        Ok(summaries)
    }
}

/// An instance known only by the buckets it served: first and last seen in them.
fn seen_in(
    context: &ApmQueryContext,
    name: &str,
    instance_id: &str,
    buckets: &[MergedBucket],
) -> InstanceObservation {
    let times = buckets.iter().map(|bucket| bucket.bucket_at);
    let first = times.clone().min().unwrap_or(context.range.start);
    let last = times.max().unwrap_or(context.range.end);
    let service = buckets.first().map_or_else(
        || {
            ServiceIdentity::new(
                context.namespace.as_deref(),
                Some(name),
                context.environment.as_deref(),
                None,
            )
        },
        |bucket| bucket.dimension.service().clone(),
    );

    InstanceObservation {
        org_id: context.org_id.clone(),
        service,
        instance_id: instance_id.to_owned(),
        version: buckets
            .iter()
            .max_by_key(|bucket| bucket.bucket_at)
            .and_then(|bucket| bucket.dimension.version().map(str::to_owned)),
        first_seen_at: first,
        last_seen_at: last,
        attributes: BTreeMap::new(),
    }
}
