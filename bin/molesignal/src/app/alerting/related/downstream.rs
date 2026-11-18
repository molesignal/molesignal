// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The services downstream of an incident's services on the APM service map: what they call,
//! where their trouble may come from. Their changes count as the incident's changes too.

use domain::alerting::incident::Incident;
use kernel::ids::Id;
use service_graph::ServiceGraphRepository;

use super::{AFTER, BEFORE};
use crate::app::apm::{ApmQueryRequest, ApmQueryService, MapNodeKind};

/// How many of the incident's services are followed downstream.
const MOST_SERVICES: usize = 3;

/// The services downstream of the incident's services around the time it began, best effort:
/// empty when APM has no map or it cannot be read.
pub async fn downstream_of(
    apm: &ApmQueryService,
    graph: &dyn ServiceGraphRepository,
    org_id: &Id,
    incident: &Incident,
) -> Vec<String> {
    let mut names = Vec::new();

    for service in incident
        .affected_services
        .iter()
        .filter(|service| !service.trim().is_empty())
        .take(MOST_SERVICES)
    {
        let request = ApmQueryRequest {
            from: incident.created_at.0 - BEFORE,
            to: incident.created_at.0 + AFTER,
            namespace: None,
            service: None,
            environment: None,
            version: None,
            resolution: Default::default(),
            sort: None,
            direction: None,
            limit: None,
            cursor: None,
        };

        let Ok(context) = apm.context(org_id.clone(), request, &["request_count"], "request_count")
        else {
            continue;
        };

        let Ok(radius) = apm.blast_radius(&context, graph, service).await else {
            continue;
        };

        names.extend(
            radius
                .downstream
                .into_iter()
                .filter(|node| node.kind == MapNodeKind::Service)
                .map(|node| node.name),
        );
    }

    names.sort();
    names.dedup();
    names
}
