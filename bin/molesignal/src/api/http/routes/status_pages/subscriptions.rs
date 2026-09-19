// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{access::mask_email, cookie_value, request_hostname};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{iam::IamContext, status_page::StatusPageSubscriptionInput},
    domain::{
        iam::permission,
        status_page::{
            StatusPageDeliveryPage, StatusPageNotificationDelivery, StatusPageSubscriber,
            StatusPageSubscriberChannel, StatusPageSubscriberPage, StatusPageSubscriberStatus,
            StatusPageVisibility,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn public_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/public/status-pages/{slug}/subscriptions",
            post(subscribe),
        )
        .route(
            "/api/v1/public/status-pages/{slug}/subscriptions/confirm",
            post(confirm),
        )
        .route(
            "/api/v1/public/status-pages/{slug}/subscriptions/unsubscribe",
            post(unsubscribe),
        )
}

pub(super) fn management_routes() -> Router<AppState> {
    Router::new()
        .route("/status-pages/{page_id}/subscribers", get(list_subscribers))
        .route(
            "/status-pages/{page_id}/subscribers/{subscriber_id}",
            delete(revoke_subscriber),
        )
        .route(
            "/status-pages/{page_id}/subscribers/{subscriber_id}/resend",
            post(resend_confirmation),
        )
        .route("/status-pages/{page_id}/deliveries", get(list_deliveries))
}

#[derive(Debug, Deserialize)]
struct SubscribeRequest {
    channel: StatusPageSubscriberChannel,
    #[serde(default)]
    target: String,
}

#[derive(Debug, Deserialize)]
struct SubscriptionTokenRequest {
    token: String,
}

#[derive(Debug, Deserialize)]
struct PageQuery {
    #[serde(default = "default_page")]
    page: u32,
}

#[derive(Debug, Serialize)]
struct SubscriberResponse {
    id: Id,
    channel: StatusPageSubscriberChannel,
    masked_target: String,
    status: StatusPageSubscriberStatus,
    confirmation_sent_at: Option<TimestampMicros>,
    confirmed_at: Option<TimestampMicros>,
    unsubscribed_at: Option<TimestampMicros>,
    created_at: TimestampMicros,
    updated_at: TimestampMicros,
}

impl From<StatusPageSubscriber> for SubscriberResponse {
    fn from(subscriber: StatusPageSubscriber) -> Self {
        Self {
            id: subscriber.id,
            channel: subscriber.channel,
            masked_target: mask_target(subscriber.channel, &subscriber.target),
            status: subscriber.status,
            confirmation_sent_at: subscriber.confirmation_sent_at,
            confirmed_at: subscriber.confirmed_at,
            unsubscribed_at: subscriber.unsubscribed_at,
            created_at: subscriber.created_at,
            updated_at: subscriber.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct SubscriberPageResponse {
    items: Vec<SubscriberResponse>,
    page: u32,
    per_page: u32,
    total: u64,
}

impl From<StatusPageSubscriberPage> for SubscriberPageResponse {
    fn from(page: StatusPageSubscriberPage) -> Self {
        Self {
            items: page
                .items
                .into_iter()
                .map(SubscriberResponse::from)
                .collect(),
            page: page.page,
            per_page: page.per_page,
            total: page.total,
        }
    }
}

#[derive(Debug, Serialize)]
struct DeliveryResponse {
    id: Id,
    subscriber_id: Id,
    channel: StatusPageSubscriberChannel,
    masked_target: String,
    event_key: String,
    status: String,
    attempts: i32,
    next_attempt_at: TimestampMicros,
    last_error: Option<String>,
    delivered_at: Option<TimestampMicros>,
    created_at: TimestampMicros,
    updated_at: TimestampMicros,
}

impl From<StatusPageNotificationDelivery> for DeliveryResponse {
    fn from(delivery: StatusPageNotificationDelivery) -> Self {
        Self {
            id: delivery.id,
            subscriber_id: delivery.subscriber_id,
            channel: delivery.channel,
            masked_target: mask_target(delivery.channel, &delivery.target),
            event_key: delivery.event_key,
            status: delivery.status.as_str().to_string(),
            attempts: delivery.attempts,
            next_attempt_at: delivery.next_attempt_at,
            last_error: delivery.last_error,
            delivered_at: delivery.delivered_at,
            created_at: delivery.created_at,
            updated_at: delivery.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct DeliveryPageResponse {
    items: Vec<DeliveryResponse>,
    page: u32,
    per_page: u32,
    total: u64,
}

impl From<StatusPageDeliveryPage> for DeliveryPageResponse {
    fn from(page: StatusPageDeliveryPage) -> Self {
        Self {
            items: page.items.into_iter().map(DeliveryResponse::from).collect(),
            page: page.page,
            per_page: page.per_page,
            total: page.total,
        }
    }
}

async fn subscribe(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SubscribeRequest>,
) -> Result<(
    StatusCode,
    Json<crate::app::status_page::StatusPageSubscriptionOutcome>,
)> {
    let metadata = state.status_pages.access_metadata(&slug).await?;
    let outcome = if metadata.visibility == StatusPageVisibility::Private {
        if request.channel != StatusPageSubscriberChannel::Email {
            return Err(Error::invalid(
                "private status pages only support verified email subscriptions",
            ));
        }
        let cookie_name = state
            .status_pages
            .access_cookie_name_for_slug(&slug)
            .await?;
        let token = cookie_value(&headers, &cookie_name)
            .ok_or_else(|| Error::unauthorized("private status page access is required"))?;
        let origin = request_hostname(&headers).unwrap_or_default();
        state
            .status_pages
            .subscribe_private_email(&slug, token, &origin)
            .await?
    } else {
        state
            .status_pages
            .subscribe_public(
                &slug,
                StatusPageSubscriptionInput {
                    channel: request.channel,
                    target: request.target,
                },
            )
            .await?
    };
    Ok((StatusCode::ACCEPTED, Json(outcome)))
}

async fn confirm(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(request): Json<SubscriptionTokenRequest>,
) -> Result<Json<crate::app::status_page::StatusPageSubscriptionOutcome>> {
    Ok(Json(
        state
            .status_pages
            .confirm_public_subscription(&slug, &request.token)
            .await?,
    ))
}

async fn unsubscribe(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(request): Json<SubscriptionTokenRequest>,
) -> Result<Json<crate::app::status_page::StatusPageSubscriptionOutcome>> {
    Ok(Json(
        state
            .status_pages
            .unsubscribe_public(&slug, &request.token)
            .await?,
    ))
}

#[permission("status_pages.manage")]
async fn list_subscribers(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<SubscriberPageResponse>> {
    Ok(Json(
        state
            .status_pages
            .list_subscribers(&context.org_id, &Id(page_id), query.page)
            .await?
            .into(),
    ))
}

#[permission("status_pages.manage")]
async fn revoke_subscriber(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, subscriber_id)): Path<(String, String)>,
) -> Result<Json<SubscriberResponse>> {
    let page_id = Id(page_id);
    let subscriber = state
        .status_pages
        .revoke_subscriber(&context.org_id, &page_id, &Id(subscriber_id.clone()))
        .await?;
    record_subscriber_audit(&state, &context, "revoke", &page_id, &subscriber_id).await;
    Ok(Json(subscriber.into()))
}

#[permission("status_pages.manage")]
async fn resend_confirmation(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((page_id, subscriber_id)): Path<(String, String)>,
) -> Result<Json<SubscriberResponse>> {
    let page_id = Id(page_id);
    let subscriber = state
        .status_pages
        .resend_pending_confirmation(&context.org_id, &page_id, &Id(subscriber_id.clone()))
        .await?;
    record_subscriber_audit(&state, &context, "resend", &page_id, &subscriber_id).await;
    Ok(Json(subscriber.into()))
}

#[permission("status_pages.manage")]
async fn list_deliveries(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(page_id): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<DeliveryPageResponse>> {
    Ok(Json(
        state
            .status_pages
            .list_notification_deliveries(&context.org_id, &Id(page_id), query.page)
            .await?
            .into(),
    ))
}

async fn record_subscriber_audit(
    state: &AppState,
    context: &IamContext,
    verb: &str,
    page_id: &Id,
    subscriber_id: &str,
) {
    activity_audit::record(
        state,
        context,
        &format!("status_page.subscriber.{verb}"),
        "status_page_subscriber",
        subscriber_id,
        serde_json::json!({"status_page_id": page_id}),
    )
    .await;
}

fn mask_target(channel: StatusPageSubscriberChannel, target: &str) -> String {
    match channel {
        StatusPageSubscriberChannel::Email => mask_email(target),
        StatusPageSubscriberChannel::Webhook => Url::parse(target)
            .ok()
            .and_then(|url| {
                let host = url.host_str()?;
                let port = url
                    .port()
                    .map(|port| format!(":{port}"))
                    .unwrap_or_default();
                Some(format!("{}://{host}{port}/***", url.scheme()))
            })
            .unwrap_or_else(|| "***".into()),
    }
}

fn default_page() -> u32 {
    1
}
