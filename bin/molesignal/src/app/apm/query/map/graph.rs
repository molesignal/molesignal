// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The calls between services as the service graph keeps them, read under
//! the names APM reads: the graph keeps the names and environments spans
//! carry, which the organization's naming rules, environment mapping and
//! aliases read otherwise.

use std::collections::BTreeMap;

use futures::future::try_join_all;
use service_graph::{EdgeSnapshot, ServiceGraphRepository};

use super::super::{
    ApmQueryContext, ApmQueryService, GraphCall, GraphPoint, MapEdgeDetail, MapResponse,
};
use crate::{
    app::apm::OrgNames,
    domain::apm::QueryResolution,
    shared::{Error, Result, time::TimestampMicros},
};

const MINUTE_MICROS: i64 = 60_000_000;
const HOUR_MICROS: i64 = 3_600_000_000;
/// Ranges longer than this draw an edge's trend an hour at a time.
const MINUTE_TREND_MAX_MICROS: i64 = 24 * HOUR_MICROS;

impl ApmQueryService {
    /// The map of `context`, the calls between its services read from `graph`.
    pub async fn map_of(
        &self,
        context: &ApmQueryContext,
        graph: &dyn ServiceGraphRepository,
    ) -> Result<MapResponse> {
        let (from, to) = (context.range.start.0, context.range.end.0);
        let length = to - from;
        let environment = context.environment.as_deref();
        let names = self.org_names(context).await;
        // A mapped environment is read under each name spans give it.
        let graph_environment = environment.filter(|_| !names.maps_environments());

        let (now, before) = tokio::try_join!(
            graph.summarize(&context.org_id, from, to, graph_environment),
            graph.summarize(&context.org_id, from - length, from, graph_environment),
        )?;

        self.map(
            context,
            &calls(&now, &names, environment),
            &calls(&before, &names, environment),
        )
        .await
    }

    /// One call of the map of `context`, from `source` to `target` (node ids);
    /// between two services with its trend from `graph`.
    pub async fn map_edge_of(
        &self,
        context: &ApmQueryContext,
        graph: &dyn ServiceGraphRepository,
        source: &str,
        target: &str,
    ) -> Result<MapEdgeDetail> {
        if source.len() > 512 || target.len() > 512 {
            return Err(Error::invalid("a map node id is at most 512 bytes"));
        }

        let points = match (
            source.strip_prefix("service:"),
            target.strip_prefix("service:"),
        ) {
            (Some(client), Some(server)) => {
                self.graph_trend(context, graph, client, server).await?
            }
            _ => Vec::new(),
        };

        self.map_edge(context, source, target, &points).await
    }

    /// The calls from `client` to `server`, as APM names them, a minute or an
    /// hour at a time.
    async fn graph_trend(
        &self,
        context: &ApmQueryContext,
        graph: &dyn ServiceGraphRepository,
        client: &str,
        server: &str,
    ) -> Result<Vec<GraphPoint>> {
        let (from, to) = (context.range.start.0, context.range.end.0);
        let step = match context.resolution {
            QueryResolution::Hour => HOUR_MICROS,
            QueryResolution::Minute => MINUTE_MICROS,
            QueryResolution::Auto if to - from > MINUTE_TREND_MAX_MICROS => HOUR_MICROS,
            QueryResolution::Auto => MINUTE_MICROS,
        };
        let names = self.org_names(context).await;
        let environment = context.environment.as_deref();

        // The pairs of names spans carry that read as these two.
        let pairs = if names.is_plain() {
            vec![(
                client.to_owned(),
                server.to_owned(),
                environment.map(str::to_owned),
            )]
        } else {
            spans_pairs(
                &graph.summarize(&context.org_id, from, to, None).await?,
                &names,
                (client, server),
                environment,
            )
        };

        let trends = try_join_all(pairs.iter().map(|(client, server, environment)| {
            graph.trend(
                &context.org_id,
                from,
                to,
                client,
                server,
                environment.as_deref(),
                step,
            )
        }))
        .await?;

        Ok(summed_points(trends.into_iter().flatten()))
    }

    async fn org_names(&self, context: &ApmQueryContext) -> OrgNames {
        OrgNames::of(self.org_settings(&context.org_id).await.as_ref())
    }
}

/// The graph's calls under the names APM reads, in `environment` when one is in view.
fn calls(
    snapshots: &[EdgeSnapshot],
    names: &OrgNames,
    environment: Option<&str>,
) -> Vec<GraphCall> {
    snapshots
        .iter()
        .filter(|snapshot| {
            environment
                .is_none_or(|environment| names.environment(&snapshot.environment) == environment)
        })
        .map(|snapshot| GraphCall {
            client: names.service(&snapshot.client_service),
            server: names.service(&snapshot.server_service),
            ..call(snapshot)
        })
        .collect()
}

/// The names, as spans carry them, of the calls from `client` to `server` as
/// APM reads them, with their environment when one is in view.
fn spans_pairs(
    snapshots: &[EdgeSnapshot],
    names: &OrgNames,
    (client, server): (&str, &str),
    environment: Option<&str>,
) -> Vec<(String, String, Option<String>)> {
    let mut pairs = snapshots
        .iter()
        .filter(|snapshot| {
            names.service(&snapshot.client_service) == client
                && names.service(&snapshot.server_service) == server
                && environment.is_none_or(|environment| {
                    names.environment(&snapshot.environment) == environment
                })
        })
        .map(|snapshot| {
            (
                snapshot.client_service.clone(),
                snapshot.server_service.clone(),
                environment.map(|_| snapshot.environment.clone()),
            )
        })
        .collect::<Vec<_>>();

    pairs.sort();
    pairs.dedup();
    pairs
}

/// One point a step of every trend, the earliest first: calls summed, the slowest P95 kept.
fn summed_points(snapshots: impl Iterator<Item = EdgeSnapshot>) -> Vec<GraphPoint> {
    let mut points = BTreeMap::<i64, GraphCall>::new();

    for snapshot in snapshots {
        let next = call(&snapshot);

        points
            .entry(snapshot.bucket_at)
            .and_modify(|point| {
                point.request_count += next.request_count;
                point.error_count += next.error_count;
                point.p95_micros = point.p95_micros.max(next.p95_micros);
            })
            .or_insert(next);
    }

    points
        .into_iter()
        .map(|(bucket_at, call)| GraphPoint {
            bucket_at: TimestampMicros(bucket_at),
            call,
        })
        .collect()
}

fn call(snapshot: &EdgeSnapshot) -> GraphCall {
    GraphCall {
        client: snapshot.client_service.clone(),
        server: snapshot.server_service.clone(),
        request_count: snapshot.request_count,
        error_count: snapshot.error_count,
        p95_micros: snapshot
            .p95_us
            .and_then(|micros| u64::try_from(micros).ok()),
    }
}
