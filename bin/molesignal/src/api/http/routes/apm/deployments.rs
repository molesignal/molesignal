// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Deployments and other changes a pipeline reports, with an API token that
//! holds `apm.deploy`; `GET /changes` lists them. A reported service and
//! environment are named as APM names them: renamed by the organization's
//! naming rules, an alias as its service, the environment mapped.

use axum::{Extension, Json, Router, extract::State, http::StatusCode, routing::post};
use domain::{
    apm::DeploymentEvent,
    iam::{IamContext, permission},
};
use kernel::{Result, time::TimestampMicros};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{
        apm::OrgNames,
        changes::{ReportedChange, change_event},
    },
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/apm/deployments", post(record_deployment))
}

#[permission("apm.deploy")]
async fn record_deployment(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<ReportedChange>,
) -> Result<(StatusCode, Json<DeploymentEvent>)> {
    let names = OrgNames::of(
        state
            .telemetry
            .apm_query
            .org_settings(&ctx.org_id)
            .await
            .as_ref(),
    );

    let event = change_event(
        &names,
        request,
        ctx.principal_id().clone(),
        TimestampMicros::now(),
    )?;

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
