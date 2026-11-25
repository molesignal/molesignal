// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The assets of MoleSignal Node Agent's data source page (see [`crate::app::node_agent`]).
//!
//! - `GET  /node-agent/assets`: whether the agent's streams have arrived, and where the indexes of
//!   the host events, the host dashboard, the saved queries and the alert rules stand.
//! - `POST /node-agent/assets`: installs what is missing, as far as the caller may: the indexes
//!   (`streams.configure`), the dashboard (`dashboards.create`), the saved queries
//!   (`saved_views.create`) and the alert rules, disabled (`alerts.rules.manage`). What the caller
//!   may not install is named in `skipped`. Installing again changes nothing a person has edited.

use axum::{Extension, Json, Router, extract::State, routing::get};
use domain::{
    alerting::incident::Severity,
    federation::{CudAction, ResourceKind},
    iam::{IamContext, permission},
    stream::StreamType,
};
use kernel::{Error, Result, ids::Id};
use serde::Serialize;

use crate::{
    api::{AppState, http::federation::emit_cud},
    app::{
        dashboard::builtin::{Builtin, HostDashboard, Standing},
        node_agent::{
            EVENTS_STREAM, METRICS_STREAM,
            alerts::{ALERTS, made_by},
            indexes::{self, IndexState},
            views::{self, VIEWS},
        },
    },
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/node-agent/assets", get(status).post(install))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Assets {
    streams: Streams,
    indexes: IndexState,
    dashboard: DashboardStatus,
    views: Vec<ViewStatus>,
    /// `None` when the caller may not read alert rules.
    alerts: Option<Vec<AlertStatus>>,
    /// What the caller may not install.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<&'static str>,
}

#[derive(Serialize)]
struct Streams {
    metrics: bool,
    events: bool,
}

#[derive(Serialize)]
struct DashboardStatus {
    id: Id,
    state: Standing,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewStatus {
    key: &'static str,
    id: Id,
    installed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertStatus {
    key: &'static str,
    severity: Severity,
    /// The rule the template made, when there is one.
    rule_id: Option<Id>,
    enabled: bool,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn status(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Assets>> {
    Ok(Json(assets(&state, &ctx, Vec::new()).await?))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn install(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Assets>> {
    let mut skipped = Vec::new();
    let dashboard = HostDashboard::Overview;

    match ctx.has_permission("streams.configure") {
        true => {
            indexes::apply(state.telemetry.streams.as_ref(), &ctx.org_id).await?;
        }
        false => skipped.push("indexes"),
    }

    match ctx.has_permission("dashboards.create") {
        true => {
            state.dashboard.install(&ctx.org_id, dashboard).await?;
        }
        false => skipped.push("dashboard"),
    }

    match ctx.has_permission("saved_views.create") {
        true => {
            views::install(
                state.platform.saved_view.as_ref(),
                &ctx.org_id,
                &ctx.user_id,
            )
            .await?;
        }
        false => skipped.push("views"),
    }

    match ctx.has_permission("alerts.rules.manage") {
        true => install_alerts(&state, &ctx.org_id, dashboard.id(&ctx.org_id)).await?,
        false => skipped.push("alerts"),
    }

    Ok(Json(assets(&state, &ctx, skipped).await?))
}

/// Makes the rules of the templates the organization has none of, disabled.
async fn install_alerts(state: &AppState, org_id: &Id, dashboard: Id) -> Result<()> {
    let rules = state.alerting.service.list_rules(org_id).await?;

    for alert in ALERTS {
        if made_by(&rules, &alert).is_some() {
            continue;
        }

        let saved = state
            .alerting
            .service
            .create_rule(alert.rule(org_id, dashboard.clone()))
            .await?;

        emit_cud(
            state,
            org_id,
            ResourceKind::AlertRule,
            CudAction::Created,
            &saved.id.0,
            &saved,
        )
        .await;
    }

    Ok(())
}

async fn assets(state: &AppState, ctx: &IamContext, skipped: Vec<&'static str>) -> Result<Assets> {
    let org_id = &ctx.org_id;
    let streams = state.telemetry.streams.as_ref();

    let present = |name: &'static str, kind: StreamType| async move {
        match streams.get(org_id, name, kind).await {
            Ok(_) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    };

    let dashboard = HostDashboard::Overview;
    let installed = views::installed(state.platform.saved_view.as_ref(), org_id).await?;

    let alerts = match ctx.has_permission("alerts.rules.read") {
        true => {
            let rules = state.alerting.service.list_rules(org_id).await?;

            Some(
                ALERTS
                    .iter()
                    .map(|alert| {
                        let rule = made_by(&rules, alert);

                        AlertStatus {
                            key: alert.key,
                            severity: alert.severity,
                            rule_id: rule.map(|rule| rule.id.clone()),
                            enabled: rule.is_some_and(|rule| rule.enabled),
                        }
                    })
                    .collect(),
            )
        }
        false => None,
    };

    Ok(Assets {
        streams: Streams {
            metrics: present(METRICS_STREAM, StreamType::METRICS).await?,
            events: present(EVENTS_STREAM, StreamType::LOGS).await?,
        },
        indexes: indexes::state(streams, org_id).await?,
        dashboard: DashboardStatus {
            id: dashboard.id(org_id),
            state: state.dashboard.standing(org_id, dashboard).await?,
        },
        views: VIEWS
            .iter()
            .map(|view| ViewStatus {
                key: view.key,
                id: view.id(org_id),
                installed: installed.contains(&view.key),
            })
            .collect(),
        alerts,
        skipped,
    })
}
