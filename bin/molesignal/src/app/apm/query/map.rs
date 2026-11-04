// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The service map of APM: services and the calls between them, the
//! databases, caches and outside endpoints they call, and the topics they
//! publish to and consume from, each with what it served or was asked in the
//! time in view. Calls new against the stretch of the same length before,
//! and those gone since, are marked. Calls between services come from the
//! service graph, which pairs a span with its parent in another service; the
//! rest from the APM buckets.

use std::collections::{BTreeSet, HashMap};

use serde::Serialize;

use self::builder::Map;
use super::{
    ApmQueryContext, ApmQueryService, ApmResponseMeta, RedSummary, ServiceHealth,
    common::overflow_dimensions,
};
use crate::{
    domain::apm::{BucketKind, DependencyCategory, OTHER_DIMENSION},
    shared::{
        Result,
        time::{TimeRange, TimestampMicros},
    },
};

/// The most nodes a map draws; past it the least busy services are left out.
const MAX_NODES: usize = 400;

/// Calls from one service to another, as the service graph pairs them: the
/// counts over the window and the P95 of its slowest minute.
#[derive(Debug, Clone)]
pub struct GraphCall {
    pub client: String,
    pub server: String,
    pub request_count: u64,
    pub error_count: u64,
    pub p95_micros: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapResponse {
    pub meta: ApmResponseMeta,
    pub nodes: Vec<MapNode>,
    pub edges: Vec<MapEdge>,
    /// The namespaces of the services in the environment in view.
    pub namespaces: Vec<String>,
    /// Every environment the services run in, whichever one is in view.
    pub environments: Vec<String>,
    /// More nodes than a map draws: the least busy services were left out.
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MapNodeKind {
    Service,
    /// A database, a cache, an outside endpoint, or a service that reports nothing of its own.
    External,
    Topic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MapEdgeKind {
    Call,
    Publish,
    Consume,
}

/// A call or a node against the stretch before: `new` when it was not
/// there, `gone` when it is no longer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MapChange {
    New,
    Gone,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapRed {
    pub request_count: u64,
    pub error_count: u64,
    pub error_rate: f64,
    pub p95_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapNode {
    /// `service:<name>`, `topic:<name>`, or `<category>:<target>`.
    pub id: String,
    pub kind: MapNodeKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<DependencyCategory>,
    /// The environments a service ran in.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub environments: Vec<String>,
    /// What a service served; what an external node or a topic was asked.
    pub red: MapRed,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<ServiceHealth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<MapChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub kind: MapEdgeKind,
    pub red: MapRed,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<MapChange>,
}

impl ApmQueryService {
    /// The map of `context`'s environment and namespace, from the service
    /// graph's `calls` of the window and `calls_before` of the stretch before.
    pub async fn map(
        &self,
        context: &ApmQueryContext,
        calls: &[GraphCall],
        calls_before: &[GraphCall],
    ) -> Result<MapResponse> {
        let mut before = context.clone();
        let length = context.range.duration_micros();
        before.range = TimeRange::new(
            TimestampMicros(context.range.start.0 - length),
            context.range.start,
        );

        let mut every_namespace = context.clone();
        every_namespace.namespace = None;
        let mut everywhere = every_namespace.clone();
        everywhere.environment = None;

        let service_query = self.bucket_query(context, BucketKind::Service);
        let dependency_query = self.bucket_query(context, BucketKind::Dependency);
        let transaction_query = self.bucket_query(context, BucketKind::Transaction);
        let dependencies_before_query = self.bucket_query(&before, BucketKind::Dependency);
        let transactions_before_query = self.bucket_query(&before, BucketKind::Transaction);
        let catalog_query = self.catalog_query(&every_namespace);
        let everywhere_query = self.catalog_query(&everywhere);

        let (
            service_buckets,
            dependency_buckets,
            transaction_buckets,
            dependencies_before,
            transactions_before,
            catalog,
            everywhere_catalog,
            thresholds,
        ) = tokio::try_join!(
            self.repository.query_buckets(&service_query),
            self.repository.query_buckets(&dependency_query),
            self.repository.query_buckets(&transaction_query),
            self.repository.query_buckets(&dependencies_before_query),
            self.repository.query_buckets(&transactions_before_query),
            self.repository.list_services(&catalog_query),
            self.repository.list_services(&everywhere_query),
            self.apdex_thresholds(&context.org_id),
        )?;

        let namespaces = catalog
            .iter()
            .map(|observation| {
                (
                    observation.service.name.clone(),
                    observation.service.namespace.clone(),
                )
            })
            .collect::<HashMap<_, _>>();
        let schema = &self.config.histogram;
        let lines = self.org_settings(&context.org_id).await.health;

        let mut map = Map::default();
        map.services(&service_buckets, schema, (&thresholds, &lines))?;
        map.calls(calls);
        map.dependencies(&dependency_buckets, schema)?;
        map.consumers(&transaction_buckets, schema)?;

        let mut earlier = Map::default();
        earlier.calls(calls_before);
        earlier.dependencies(&dependencies_before, schema)?;
        earlier.consumers(&transactions_before, schema)?;
        map.mark_changes(&earlier);

        if let Some(namespace) = context.namespace.as_deref() {
            map.keep_namespace(namespace, &namespaces);
        }

        map.name_namespaces(&namespaces);
        let truncated = map.keep_busiest(MAX_NODES);

        let mut listed = catalog
            .iter()
            .map(|observation| observation.service.namespace.clone())
            .collect::<BTreeSet<_>>();
        listed.remove("");

        let mut environments = everywhere_catalog
            .iter()
            .map(|observation| observation.service.environment.clone())
            .collect::<BTreeSet<_>>();
        environments.remove("");

        let mut overflows = overflow_dimensions(&service_buckets);
        overflows.extend(overflow_dimensions(&dependency_buckets));
        overflows.extend(overflow_dimensions(&transaction_buckets));

        Ok(MapResponse {
            meta: self.response_meta(context, overflows, false).await?,
            nodes: map.nodes.into_values().collect(),
            edges: map.edges.into_values().collect(),
            namespaces: listed.into_iter().collect(),
            environments: environments.into_iter().collect(),
            truncated,
        })
    }
}

/// The id of a service's node.
pub(super) fn service_node(name: &str) -> String {
    format!("service:{name}")
}

/// The id of a topic's node.
pub(super) fn topic_node(name: &str) -> String {
    format!("topic:{name}")
}

/// The id of what a service calls: its node, and whether it is a topic.
pub(super) fn target_node(category: DependencyCategory, target: &str) -> (String, MapNodeKind) {
    match category {
        DependencyCategory::Service => (service_node(target), MapNodeKind::Service),
        DependencyCategory::Messaging => (topic_node(target), MapNodeKind::Topic),
        _ => (
            format!("{}:{target}", category_name(category)),
            MapNodeKind::External,
        ),
    }
}

pub(super) fn category_name(category: DependencyCategory) -> &'static str {
    match category {
        DependencyCategory::Service => "service",
        DependencyCategory::Database => "database",
        DependencyCategory::Cache => "cache",
        DependencyCategory::Messaging => "messaging",
        DependencyCategory::ExternalHttp => "external_http",
        DependencyCategory::ExternalRpc => "external_rpc",
        DependencyCategory::Other => "other",
    }
}

/// The topic a consumer's entry reads from: its endpoint is named
/// `<operation> <destination>`.
pub(super) fn consumed_topic(transaction: &str) -> Option<&str> {
    transaction
        .split_once(' ')
        .map(|(_, destination)| destination.trim())
        .filter(|destination| !destination.is_empty() && *destination != OTHER_DIMENSION)
}

pub(super) fn map_red(red: &RedSummary) -> MapRed {
    MapRed {
        request_count: red.request_count,
        error_count: red.error_count,
        error_rate: red.error_rate,
        p95_time: red.p95_time,
    }
}

mod builder;
mod graph;
#[cfg(test)]
mod tests;
