// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::Deserialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        notify::delivery::{DeliveryFilter, DeliveryStage, DeliveryStatus, NotifyDelivery},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/notify/deliveries", get(list))
        .route("/notify/deliveries/{id}", get(get_one))
        .route("/notify/deliveries/{id}/ack", post(acknowledge))
        .route("/notify/deliveries/{id}/retry", post(retry))
}

#[derive(Debug, Default, Deserialize)]
struct DeliveryQuery {
    event_id: Option<String>,
    policy_id: Option<Id>,
    user_id: Option<Id>,
    connector_id: Option<Id>,
    status: Option<String>,
    stage: Option<String>,
    from_micros: Option<i64>,
    to_micros: Option<i64>,
    limit: Option<u32>,
}

fn parse_status(value: Option<&str>) -> Result<Option<DeliveryStatus>> {
    value
        .map(|value| match value {
            "pending" => Ok(DeliveryStatus::Pending),
            "sending" => Ok(DeliveryStatus::Sending),
            "success" => Ok(DeliveryStatus::Success),
            "failed" => Ok(DeliveryStatus::Failed),
            "skipped" => Ok(DeliveryStatus::Skipped),
            "acknowledged" => Ok(DeliveryStatus::Acknowledged),
            _ => Err(Error::invalid("unknown notify delivery status")),
        })
        .transpose()
}

fn parse_stage(value: Option<&str>) -> Result<Option<DeliveryStage>> {
    value
        .map(|value| match value {
            "user_primary" => Ok(DeliveryStage::UserPrimary),
            "user_fallback" => Ok(DeliveryStage::UserFallback),
            "team_fallback" => Ok(DeliveryStage::TeamFallback),
            "organization_fallback" => Ok(DeliveryStage::OrganizationFallback),
            "escalation" => Ok(DeliveryStage::Escalation),
            "test" => Ok(DeliveryStage::Test),
            _ => Err(Error::invalid("unknown notify delivery stage")),
        })
        .transpose()
}

#[permission("alerts.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Query(query): Query<DeliveryQuery>,
) -> Result<Json<Vec<NotifyDelivery>>> {
    if query
        .from_micros
        .zip(query.to_micros)
        .is_some_and(|(from, to)| from > to)
    {
        return Err(Error::invalid("from_micros must not exceed to_micros"));
    }
    let filter = DeliveryFilter {
        event_id: query.event_id,
        policy_id: query.policy_id,
        recipient_user_id: query.user_id,
        connector_id: query.connector_id,
        status: parse_status(query.status.as_deref())?,
        stage: parse_stage(query.stage.as_deref())?,
        from: query.from_micros.map(TimestampMicros),
        to: query.to_micros.map(TimestampMicros),
        limit: query.limit.unwrap_or(100),
    };
    Ok(Json(
        state
            .alerting
            .notify
            .list_deliveries(&context.org_id, &filter)
            .await?,
    ))
}

#[permission("alerts.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<NotifyDelivery>> {
    Ok(Json(
        state
            .alerting
            .notify
            .get_delivery(&context.org_id, &Id::from_string(id))
            .await?,
    ))
}

#[permission("alerts.acknowledge")]
async fn acknowledge(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<NotifyDelivery>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .acknowledge_delivery(
                &context.org_id,
                &Id::from_string(id),
                TimestampMicros::now(),
            )
            .await?,
    ))
}

#[permission("alerts.manage")]
async fn retry(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<crate::app::notify::NotifyEventOutcome>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .retry_delivery(&context.org_id, &Id::from_string(id))
            .await?,
    ))
}
