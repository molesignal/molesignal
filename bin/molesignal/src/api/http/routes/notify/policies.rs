// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        notify::{NotifyPolicyInput, NotifyPolicyPreview},
    },
    domain::{
        alerting::time_window::TimeCondition,
        iam::permission,
        notify::{
            connector::NotifyChannel,
            policy::{
                NotifyDeliveryConfig, NotifyDeliveryMode, NotifyEvent, NotifyFallbackConfig,
                NotifyPolicy,
            },
            preference::NotifyCategory,
        },
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/notify/recipient-resolver-types",
            get(list_recipient_resolver_types),
        )
        .route("/notify/policies", get(list).post(create))
        .route(
            "/notify/policies/preview",
            axum::routing::post(preview_draft),
        )
        .route(
            "/notify/policies/{id}",
            get(get_one).put(update).delete(delete),
        )
        .route("/notify/policies/{id}/test", axum::routing::post(test))
        .route(
            "/notify/policies/{id}/restore",
            axum::routing::post(restore),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyRequest {
    name: String,
    event_type: String,
    category: NotifyCategory,
    #[serde(default = "empty_object")]
    matchers: Value,
    recipient_resolver: String,
    #[serde(default = "empty_object")]
    resolver_config: Value,
    #[serde(default)]
    delivery_mode: NotifyDeliveryMode,
    #[serde(default)]
    delivery_config: NotifyDeliveryConfig,
    #[serde(default)]
    template_id: Option<Id>,
    #[serde(default)]
    channel_templates: BTreeMap<NotifyChannel, Id>,
    #[serde(default)]
    fallback_config: NotifyFallbackConfig,
    #[serde(default)]
    ack_timeout_seconds: Option<i32>,
    #[serde(default)]
    escalation_config: Option<Value>,
    #[serde(default)]
    merge_window_secs: Option<u32>,
    #[serde(default)]
    time: Option<TimeCondition>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default = "default_priority")]
    priority: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyTestRequest {
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default)]
    occurred_at: Option<i64>,
    #[serde(default = "empty_object")]
    attributes: Value,
}

#[derive(Debug, Deserialize)]
struct PolicyPreviewRequest {
    policy: PolicyRequest,
    event: PolicyTestRequest,
}

fn empty_object() -> Value {
    serde_json::json!({})
}

const fn default_true() -> bool {
    true
}

const fn default_priority() -> i32 {
    100
}

impl From<PolicyRequest> for NotifyPolicyInput {
    fn from(request: PolicyRequest) -> Self {
        Self {
            name: request.name,
            event_type: request.event_type,
            category: request.category,
            matchers: request.matchers,
            recipient_resolver: request.recipient_resolver,
            resolver_config: request.resolver_config,
            delivery_mode: request.delivery_mode,
            delivery_config: request.delivery_config,
            template_id: request.template_id,
            channel_templates: request.channel_templates,
            fallback_config: request.fallback_config,
            ack_timeout_seconds: request.ack_timeout_seconds,
            escalation_config: request.escalation_config,
            merge_window_secs: request.merge_window_secs,
            time: request.time,
            enabled: request.enabled,
            priority: request.priority,
        }
    }
}

#[permission("notify.policies.read")]
async fn list_recipient_resolver_types(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<&'static str>>> {
    Ok(Json(
        state.alerting.notify_engine.supported_recipient_resolvers(),
    ))
}

#[permission("notify.policies.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<NotifyPolicy>>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .list_policies(&ctx.org_id)
            .await?,
    ))
}

#[permission("notify.policies.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<NotifyPolicy>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .get_policy(&ctx.org_id, &Id::from_string(id))
            .await?,
    ))
}

#[permission("notify.policies.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<PolicyRequest>,
) -> Result<(StatusCode, Json<NotifyPolicy>)> {
    let policy = state
        .alerting
        .notify_engine
        .create_policy(&ctx.org_id, request.into())
        .await?;

    Ok((StatusCode::CREATED, Json(policy)))
}

#[permission("notify.policies.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<PolicyRequest>,
) -> Result<Json<NotifyPolicy>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .update_policy(&ctx.org_id, &Id::from_string(id), request.into())
            .await?,
    ))
}

#[permission("notify.policies.delete")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    state
        .alerting
        .notify_engine
        .delete_policy(&ctx.org_id, &Id::from_string(id))
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[permission("notify.policies.manage")]
async fn restore(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<NotifyPolicy>> {
    Ok(Json(
        state
            .alerting
            .notify_engine
            .restore_policy(&ctx.org_id, &Id::from_string(id))
            .await?,
    ))
}

#[permission("notify.policies.manage")]
async fn test(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<PolicyTestRequest>,
) -> Result<Json<NotifyPolicyPreview>> {
    let id = Id::from_string(id);

    let policy = state
        .alerting
        .notify_engine
        .get_policy(&ctx.org_id, &id)
        .await?;

    let event = NotifyEvent {
        id: request
            .event_id
            .unwrap_or_else(|| format!("policy-preview-{}", policy.id)),
        event_type: request.event_type.unwrap_or(policy.event_type),
        organization_id: ctx.org_id.clone(),
        occurred_at: request
            .occurred_at
            .map(TimestampMicros)
            .unwrap_or_else(TimestampMicros::now),
        attributes: request.attributes,
    };

    Ok(Json(
        state
            .alerting
            .notify_engine
            .preview_policy(&ctx.org_id, &id, event)
            .await?,
    ))
}

#[permission("notify.policies.manage")]
async fn preview_draft(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<PolicyPreviewRequest>,
) -> Result<Json<NotifyPolicyPreview>> {
    let event_type = request
        .event
        .event_type
        .clone()
        .unwrap_or_else(|| request.policy.event_type.clone());

    let event = NotifyEvent {
        id: request
            .event
            .event_id
            .unwrap_or_else(|| "notify-policy-preview".into()),
        event_type,
        organization_id: ctx.org_id.clone(),
        occurred_at: request
            .event
            .occurred_at
            .map(TimestampMicros)
            .unwrap_or_else(TimestampMicros::now),
        attributes: request.event.attributes,
    };

    Ok(Json(
        state
            .alerting
            .notify_engine
            .preview_policy_input(&ctx.org_id, request.policy.into(), event)
            .await?,
    ))
}
