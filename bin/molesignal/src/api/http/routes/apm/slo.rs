// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Service level objectives of APM services: listed with what each has met,
//! one with its days, and set by whoever manages APM. Burn-rate alerts on an
//! objective are ordinary alert rules, written through the alert rule API.

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{
        apm::{SloDetailResponse, SloStatus},
        iam::IamContext,
    },
    domain::{
        apm::{ServiceLevelObjective, SloKind, TransactionIdentity},
        iam::permission,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/apm/slos", get(list_slos).post(create_slo))
        .route(
            "/apm/slos/{id}",
            get(get_slo).put(update_slo).delete(delete_slo),
        )
}

#[derive(Debug, Default, Deserialize)]
struct ServiceFilter {
    namespace: Option<String>,
    service: Option<String>,
    environment: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SloWrite {
    name: String,
    #[serde(default)]
    description: String,
    namespace: String,
    service: String,
    environment: String,
    #[serde(default)]
    transaction: Option<TransactionIdentity>,
    kind: SloKind,
    #[serde(default)]
    latency_threshold: Option<u64>,
    objective: f64,
    window_days: u16,
}

impl SloWrite {
    fn into_slo(
        self,
        id: Id,
        created_by: Id,
        created_at: TimestampMicros,
        ctx: &IamContext,
    ) -> Result<ServiceLevelObjective> {
        let slo = ServiceLevelObjective {
            id,
            name: self.name.trim().to_owned(),
            description: self.description.trim().to_owned(),
            namespace: self.namespace.trim().to_owned(),
            service: self.service.trim().to_owned(),
            environment: self.environment.trim().to_owned(),
            transaction: self.transaction,
            kind: self.kind,
            latency_threshold: self.latency_threshold,
            objective: self.objective,
            window_days: self.window_days,
            created_by,
            created_at,
            updated_by: ctx.principal_id().clone(),
            updated_at: TimestampMicros::now(),
        };

        slo.validate()?;
        Ok(slo)
    }
}

#[permission(any("apm.slos.read", "sys.telemetry.read"))]
async fn list_slos(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(filter): Query<ServiceFilter>,
) -> Result<Json<Vec<SloStatus>>> {
    let service = match (&filter.namespace, &filter.service, &filter.environment) {
        (Some(namespace), Some(service), Some(environment)) => {
            Some((namespace.as_str(), service.as_str(), environment.as_str()))
        }
        (None, None, None) => None,
        _ => {
            return Err(Error::invalid(
                "a service is named by its namespace, name and environment together",
            ));
        }
    };

    Ok(Json(
        state
            .telemetry
            .apm_query
            .slo_statuses(&ctx.org_id, service, TimestampMicros::now())
            .await?,
    ))
}

#[permission(any("apm.slos.read", "sys.telemetry.read"))]
async fn get_slo(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<SloDetailResponse>> {
    Ok(Json(
        state
            .telemetry
            .apm_query
            .slo_detail(&ctx.org_id, &Id::from_string(id), TimestampMicros::now())
            .await?,
    ))
}

#[permission("apm.slos.manage")]
async fn create_slo(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<SloWrite>,
) -> Result<Json<ServiceLevelObjective>> {
    let slo = request.into_slo(
        Id::new(),
        ctx.principal_id().clone(),
        TimestampMicros::now(),
        &ctx,
    )?;

    state
        .telemetry
        .apm_catalog
        .save_slo(&ctx.org_id, &slo)
        .await?;
    audit(&state, &ctx, "apm.slo.create", &slo).await;

    Ok(Json(slo))
}

#[permission("apm.slos.manage")]
async fn update_slo(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<SloWrite>,
) -> Result<Json<ServiceLevelObjective>> {
    let existing = state
        .telemetry
        .apm_catalog
        .get_slo(&ctx.org_id, &Id::from_string(id))
        .await?
        .ok_or_else(|| Error::not_found("SLO not found"))?;
    let slo = request.into_slo(existing.id, existing.created_by, existing.created_at, &ctx)?;

    state
        .telemetry
        .apm_catalog
        .save_slo(&ctx.org_id, &slo)
        .await?;
    audit(&state, &ctx, "apm.slo.update", &slo).await;

    Ok(Json(slo))
}

#[permission("apm.slos.delete")]
async fn delete_slo(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let id = Id::from_string(id);

    // A burn-rate rule left without its objective would fail every evaluation.
    let watched = state
        .alerting
        .service
        .list_rules(&ctx.org_id)
        .await?
        .iter()
        .any(|rule| rule.query.apm.as_ref().and_then(|apm| apm.slo_id.as_ref()) == Some(&id));

    if watched {
        return Err(Error::conflict(
            "alert rules watch this SLO; delete them before the SLO",
        ));
    }

    if !state
        .telemetry
        .apm_catalog
        .delete_slo(&ctx.org_id, &id)
        .await?
    {
        return Err(Error::not_found("SLO not found"));
    }

    activity_audit::record(
        &state,
        &ctx,
        "apm.slo.delete",
        "apm_slo",
        id.as_str(),
        serde_json::json!({}),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

async fn audit(state: &AppState, ctx: &IamContext, action: &str, slo: &ServiceLevelObjective) {
    activity_audit::record(
        state,
        ctx,
        action,
        "apm_slo",
        slo.id.as_str(),
        serde_json::json!({
            "name": slo.name,
            "service": format!("{}/{}/{}", slo.namespace, slo.service, slo.environment),
            "kind": slo.kind,
            "objective": slo.objective,
            "windowDays": slo.window_days,
        }),
    )
    .await;
}
