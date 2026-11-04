// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One call of the map opened: what it carried over time, what was asked
//! along it, and, of a service called, its error groups.

use serde::Serialize;

use super::{
    ApmQueryContext, ApmQueryService, ApmResponseMeta, ErrorSummary, RedSummary, RedTrendPoint,
    aggregate::{red_from_buckets, trend_from_buckets},
    common::overflow_dimensions,
    errors::error_summaries,
    map::{GraphCall, MapEdgeKind, category_name, consumed_topic},
    ranking::{dependency_summaries, transaction_summaries},
};
use crate::{
    domain::apm::{BucketDimension, BucketKind, MergedBucket, TransactionKind},
    shared::{Error, Result, time::TimestampMicros},
};

/// The most operations and errors a call lists.
const LISTED: usize = 10;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapEdgeDetail {
    pub meta: ApmResponseMeta,
    pub kind: MapEdgeKind,
    pub red: RedSummary,
    pub trend: Vec<RedTrendPoint>,
    /// What was asked along the call: a dependency's operations, the
    /// consuming endpoints of a topic, or the called service's endpoints.
    pub operations: Vec<MapOperation>,
    /// The operations are the called service's endpoints, as every caller asked them.
    pub every_caller: bool,
    /// The called service's error groups, the most frequent first.
    pub errors: Vec<ErrorSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapOperation {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<TransactionKind>,
    pub red: RedSummary,
}

/// A minute or an hour of a call between services, as the service graph has it.
#[derive(Debug, Clone)]
pub struct GraphPoint {
    pub bucket_at: TimestampMicros,
    pub call: GraphCall,
}

/// What a node id names.
enum End<'a> {
    Service(&'a str),
    Topic(&'a str),
    External(&'a str, &'a str),
}

fn end(id: &str) -> Result<End<'_>> {
    let (prefix, name) = id
        .split_once(':')
        .filter(|(_, name)| !name.is_empty())
        .ok_or_else(|| Error::invalid("a map node id is <kind>:<name>"))?;

    Ok(match prefix {
        "service" => End::Service(name),
        "topic" => End::Topic(name),
        category => End::External(category, name),
    })
}

impl ApmQueryService {
    /// The call from `source` to `target`; `graph` is the service graph's
    /// trend of it when both are services.
    pub async fn map_edge(
        &self,
        context: &ApmQueryContext,
        source: &str,
        target: &str,
        graph: &[GraphPoint],
    ) -> Result<MapEdgeDetail> {
        let of = |service: &str| {
            let mut scoped = context.clone();
            scoped.namespace = None;
            scoped.service_name = Some(service.to_owned());
            scoped
        };

        match (end(source)?, end(target)?) {
            (End::Service(caller), End::Service(callee)) => {
                self.service_call(context, &of(caller), &of(callee), callee, graph)
                    .await
            }
            (End::Service(caller), End::Topic(topic)) => {
                let buckets = self.called(&of(caller), "messaging", topic).await?;
                self.dependency_call(context, MapEdgeKind::Publish, buckets)
                    .await
            }
            (End::Service(caller), End::External(category, name)) => {
                let buckets = self.called(&of(caller), category, name).await?;
                self.dependency_call(context, MapEdgeKind::Call, buckets)
                    .await
            }
            (End::Topic(topic), End::Service(consumer)) => {
                self.consumption(context, &of(consumer), topic).await
            }
            _ => Err(Error::invalid("the map has no such call")),
        }
    }

    /// The caller's dependency buckets of one target.
    async fn called(
        &self,
        caller: &ApmQueryContext,
        category: &str,
        target: &str,
    ) -> Result<Vec<MergedBucket>> {
        let buckets = self
            .repository
            .query_buckets(&self.bucket_query(caller, BucketKind::Dependency))
            .await?;

        Ok(buckets
            .into_iter()
            .filter(|bucket| {
                matches!(
                    &bucket.dimension,
                    BucketDimension::Dependency { dependency, .. }
                        if category_name(dependency.category) == category
                            && dependency.target == target
                )
            })
            .collect())
    }

    async fn dependency_call(
        &self,
        context: &ApmQueryContext,
        kind: MapEdgeKind,
        buckets: Vec<MergedBucket>,
    ) -> Result<MapEdgeDetail> {
        let schema = &self.config.histogram;
        let mut operations = dependency_summaries(context, &buckets, schema)?
            .into_iter()
            .map(|summary| MapOperation {
                name: summary.dependency.operation.unwrap_or_default(),
                kind: None,
                red: summary.red,
            })
            .collect::<Vec<_>>();

        operations.sort_by_key(|operation| std::cmp::Reverse(operation.red.request_count));
        operations.truncate(LISTED);

        Ok(MapEdgeDetail {
            meta: self
                .response_meta(context, overflow_dimensions(&buckets), false)
                .await?,
            kind,
            red: red_from_buckets(&buckets, schema)?,
            trend: trend_from_buckets(&buckets, schema)?,
            operations,
            every_caller: false,
            errors: Vec::new(),
        })
    }

    async fn service_call(
        &self,
        context: &ApmQueryContext,
        caller: &ApmQueryContext,
        callee: &ApmQueryContext,
        callee_name: &str,
        graph: &[GraphPoint],
    ) -> Result<MapEdgeDetail> {
        let schema = &self.config.histogram;
        let named = self.called(caller, "service", callee_name).await?;
        let mut detail = self
            .dependency_call(context, MapEdgeKind::Call, named)
            .await?;

        if !graph.is_empty() {
            detail.trend = graph.iter().map(graph_point).collect();
            detail.red = graph_total(graph);
        }

        let transaction_query = self.bucket_query(callee, BucketKind::Transaction);
        let error_query = self.bucket_query(callee, BucketKind::Error);
        let group_query = self.error_group_query(callee, None);

        let (transactions, errors, groups, thresholds) = tokio::try_join!(
            self.repository.query_buckets(&transaction_query),
            self.repository.query_buckets(&error_query),
            self.repository.list_error_groups(&group_query),
            self.apdex_thresholds(&context.org_id),
        )?;

        // Without its operations named by the caller, the call is told by the endpoints it reached.
        if detail.operations.is_empty() {
            let mut endpoints = transaction_summaries(callee, &transactions, schema, &thresholds)?;
            endpoints.sort_by_key(|endpoint| std::cmp::Reverse(endpoint.red.request_count));

            detail.operations = endpoints
                .into_iter()
                .take(LISTED)
                .map(|summary| MapOperation {
                    name: summary.transaction.name,
                    kind: Some(summary.transaction.kind),
                    red: summary.red,
                })
                .collect();
            detail.every_caller = true;
        }

        let mut summaries = error_summaries(callee, &groups, &errors, schema)?;
        summaries.retain(|summary| summary.occurrence_count > 0);
        summaries.sort_by_key(|summary| std::cmp::Reverse(summary.occurrence_count));
        summaries.truncate(LISTED);
        self.attach_issues(callee, &mut summaries).await?;
        detail.errors = summaries;

        Ok(detail)
    }

    async fn consumption(
        &self,
        context: &ApmQueryContext,
        consumer: &ApmQueryContext,
        topic: &str,
    ) -> Result<MapEdgeDetail> {
        let schema = &self.config.histogram;
        let transaction_query = self.bucket_query(consumer, BucketKind::Transaction);
        let (buckets, thresholds) = tokio::try_join!(
            self.repository.query_buckets(&transaction_query),
            self.apdex_thresholds(&context.org_id),
        )?;
        let buckets = buckets
            .into_iter()
            .filter(|bucket| {
                matches!(
                    &bucket.dimension,
                    BucketDimension::Transaction { transaction, .. }
                        if transaction.kind == TransactionKind::Messaging
                            && consumed_topic(&transaction.name) == Some(topic)
                )
            })
            .collect::<Vec<_>>();

        let operations = transaction_summaries(consumer, &buckets, schema, &thresholds)?
            .into_iter()
            .take(LISTED)
            .map(|summary| MapOperation {
                name: summary.transaction.name,
                kind: Some(summary.transaction.kind),
                red: summary.red,
            })
            .collect();

        Ok(MapEdgeDetail {
            meta: self
                .response_meta(context, overflow_dimensions(&buckets), false)
                .await?,
            kind: MapEdgeKind::Consume,
            red: red_from_buckets(&buckets, schema)?,
            trend: trend_from_buckets(&buckets, schema)?,
            operations,
            every_caller: false,
            errors: Vec::new(),
        })
    }
}

fn graph_point(point: &GraphPoint) -> RedTrendPoint {
    RedTrendPoint {
        bucket_at: point.bucket_at,
        red: graph_red(
            point.call.request_count,
            point.call.error_count,
            point.call.p95_micros,
        ),
    }
}

/// The calls together: counts summed, the P95 of the slowest point.
fn graph_total(points: &[GraphPoint]) -> RedSummary {
    let requests = points.iter().map(|point| point.call.request_count).sum();
    let errors = points.iter().map(|point| point.call.error_count).sum();
    let p95 = points
        .iter()
        .filter_map(|point| point.call.p95_micros)
        .max();

    graph_red(requests, errors, p95)
}

fn graph_red(requests: u64, errors: u64, p95_micros: Option<u64>) -> RedSummary {
    RedSummary {
        request_count: requests,
        error_count: errors,
        error_rate: if requests == 0 {
            0.0
        } else {
            errors as f64 / requests as f64
        },
        p95_time: p95_micros,
        ..RedSummary::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_id_says_what_it_names() {
        assert!(matches!(
            end("service:checkout"),
            Ok(End::Service("checkout"))
        ));
        assert!(matches!(end("topic:orders"), Ok(End::Topic("orders"))));
        assert!(matches!(
            end("database:postgresql:ab12"),
            Ok(End::External("database", "postgresql:ab12"))
        ));
        assert!(end("checkout").is_err());
        assert!(end("service:").is_err());
    }

    #[test]
    fn the_graph_points_add_up_to_the_call() {
        let point = |at: i64, requests: u64, errors: u64, p95: u64| GraphPoint {
            bucket_at: TimestampMicros(at),
            call: GraphCall {
                client: "web".into(),
                server: "api".into(),
                request_count: requests,
                error_count: errors,
                p95_micros: Some(p95),
            },
        };
        let total = graph_total(&[point(0, 10, 1, 100), point(60, 30, 3, 300)]);

        assert_eq!(total.request_count, 40);
        assert_eq!(total.p95_time, Some(300));
        assert!((total.error_rate - 0.1).abs() < 1e-9);
    }
}
