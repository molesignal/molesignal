// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Deployments a pipeline reports, with an API token that holds `apm.deploy`,
//! and the deployments of a service, listed by whoever reads APM. A reported
//! service and environment are named as APM names them: renamed by the
//! organization's naming rules, an alias as its service, the environment mapped.

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{apm::OrgNames, iam::IamContext},
    domain::{
        apm::{
            ChangeKind, DEPLOYMENTS_LISTED_MAX, DeploymentEvent, DeploymentQuery, DeploymentStatus,
            masked_repository_address,
        },
        iam::permission,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/apm/deployments",
        get(list_deployments).post(record_deployment),
    )
}

/// A deployment, or another change, as a pipeline reports it. A deployment
/// needs the service and the version, another change only the service; it
/// went well and happened now unless said otherwise.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentWrite {
    #[serde(default)]
    kind: ChangeKind,
    service: String,
    #[serde(default)]
    namespace: String,
    #[serde(default)]
    environment: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    status: Option<DeploymentStatus>,
    commit: Option<String>,
    repository: Option<String>,
    pull_request: Option<String>,
    author: Option<String>,
    ci_url: Option<String>,
    description: Option<String>,
    /// When it happened, in microseconds; now when absent.
    deployed_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct DeploymentList {
    service: Option<String>,
    namespace: Option<String>,
    environment: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    limit: Option<usize>,
}

#[permission("apm.deploy")]
async fn record_deployment(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<DeploymentWrite>,
) -> Result<(StatusCode, Json<DeploymentEvent>)> {
    let now = TimestampMicros::now();
    let present = |value: Option<String>| {
        value
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let names = OrgNames::of(
        state
            .telemetry
            .apm_query
            .org_settings(&ctx.org_id)
            .await
            .as_ref(),
    );
    let environment = request.environment.trim();

    let event = DeploymentEvent {
        id: Id::new(),
        kind: request.kind,
        namespace: request.namespace.trim().to_owned(),
        service: names.service(request.service.trim()),
        environment: if environment.is_empty() {
            String::new()
        } else {
            names.environment(environment).to_owned()
        },
        version: request.version.trim().to_owned(),
        status: request.status.unwrap_or(DeploymentStatus::Succeeded),
        commit: present(request.commit),
        // A password in the address never reaches the table.
        repository_url: present(request.repository)
            .map(|address| {
                masked_repository_address(&address)
                    .ok_or_else(|| Error::invalid("repository is not a repository address"))
            })
            .transpose()?,
        pull_request_url: present(request.pull_request),
        author: present(request.author),
        ci_url: present(request.ci_url),
        description: present(request.description),
        deployed_at: request.deployed_at.map_or(now, TimestampMicros),
        created_by: ctx.principal_id().clone(),
        created_at: now,
    };

    event.validate()?;

    state
        .telemetry
        .apm_catalog
        .save_deployment(&ctx.org_id, &event)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "apm.deployment.record",
        "apm_deployment",
        event.id.as_str(),
        serde_json::json!({
            "kind": event.kind,
            "service": event.service,
            "environment": event.environment,
            "version": event.version,
            "status": event.status,
        }),
    )
    .await;

    Ok((StatusCode::CREATED, Json(event)))
}

#[permission(any("apm.deployments.read", "sys.telemetry.read"))]
async fn list_deployments(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<DeploymentList>,
) -> Result<Json<Vec<DeploymentEvent>>> {
    let blank = |value: Option<String>| value.filter(|value| !value.trim().is_empty());

    Ok(Json(
        state
            .telemetry
            .apm_catalog
            .list_deployments(
                &ctx.org_id,
                &DeploymentQuery {
                    namespace: query.namespace,
                    service: blank(query.service),
                    environment: blank(query.environment),
                    from: query.from.map(TimestampMicros),
                    to: query.to.map(TimestampMicros),
                    limit: query.limit.unwrap_or(DEPLOYMENTS_LISTED_MAX),
                },
            )
            .await?,
    ))
}
