// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 工单：
//!
//! - `GET/POST /alerts/tickets/connectors`、`PUT/DELETE /alerts/tickets/connectors/{id}`、
//!   `POST /alerts/tickets/connectors/{id}/test`：Jira、ServiceNow 与飞书项目的连接器（看要
//!   `alerts.tickets.read`，改要 `alerts.tickets.manage`）。秘密不出库，保存时留空的保持原样。
//! - `GET /alerts/tickets/sites`：飞书项目各平台的地址，页面按所选平台显示。
//! - `GET/POST /alerts/incidents/{id}/tickets`：事件关联的工单，建一张（要 `alerts.incidents.tickets`）；
//! - `POST /api/v1/public/tickets/jira/{connector}`：Jira 的 Webhook，不带登录，验 `X-Hub-Signature`；
//!   没设 Webhook 密钥的连接器不收。

use std::collections::BTreeMap;

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::{
        alerting::{
            incident::Incident,
            ticket::{MEEGLE_SITES, TicketAutoRule, TicketComments, TicketConnector, TicketSystem},
        },
        iam::{permission, resource_permission},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const WEBHOOK_PREFIX: &str = "/api/v1/public/tickets/jira/";

/// Webhook 的请求体最多读多少字节。
const MAX_WEBHOOK_BYTES: usize = 1024 * 1024;

/// 连接器存的秘密叫什么。
const SECRET_KEYS: [&str; 6] = [
    "apiToken",
    "webhookSecret",
    "pluginSecret",
    "password",
    "clientSecret",
    "apiKey",
];

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/alerts/tickets/connectors",
            get(list_connectors).post(create_connector),
        )
        .route(
            "/alerts/tickets/connectors/{id}",
            put(update_connector).delete(delete_connector),
        )
        .route("/alerts/tickets/connectors/{id}/test", post(test_connector))
        .route("/alerts/tickets/sites", get(list_sites))
        .route(
            "/alerts/incidents/{id}/tickets",
            get(incident_tickets).post(create_ticket),
        )
}

pub fn public_routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/public/tickets/jira/{connector}",
        post(jira_webhook).layer(DefaultBodyLimit::max(MAX_WEBHOOK_BYTES)),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorReq {
    name: String,
    system: TicketSystem,
    #[serde(default)]
    settings: Value,
    /// 新设的秘密；留空的保持原样。
    #[serde(default)]
    secrets: BTreeMap<String, String>,
    #[serde(default)]
    comments: TicketComments,
    #[serde(default)]
    transition_on_resolve: bool,
    #[serde(default)]
    resolve_incident_when_done: bool,
    #[serde(default)]
    auto: TicketAutoRule,
    #[serde(default)]
    on_major_incident: bool,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl ConnectorReq {
    fn into_connector(
        self,
        id: Id,
        org_id: Id,
        mut secrets: BTreeMap<String, String>,
        created_at: TimestampMicros,
    ) -> Result<TicketConnector> {
        for (key, value) in self.secrets {
            if !SECRET_KEYS.contains(&key.as_str()) {
                return Err(Error::invalid(format!("`{key}` is not a connector secret")));
            }

            let value = value.trim().to_string();

            if !value.is_empty() {
                secrets.insert(key, value);
            }
        }

        let mut connector = TicketConnector {
            id,
            org_id,
            name: self.name,
            system: self.system,
            settings: self.settings,
            secrets,
            secrets_set: Vec::new(),
            comments: self.comments,
            transition_on_resolve: self.transition_on_resolve,
            resolve_incident_when_done: self.resolve_incident_when_done,
            auto: self.auto,
            on_major_incident: self.on_major_incident,
            enabled: self.enabled,
            created_at,
            updated_at: TimestampMicros::now(),
        };

        connector.validate()?;

        let required = match connector.system {
            TicketSystem::Jira => "apiToken",
            TicketSystem::Meegle => "pluginSecret",
            TicketSystem::Servicenow => connector.servicenow()?.required_secret(),
        };

        if connector.secret(required).is_none() {
            return Err(Error::invalid(match required {
                "apiToken" => {
                    "Jira needs an API token (Cloud) or a personal access token (Data Center)"
                }
                "pluginSecret" => "Meegle needs the plugin secret",
                "password" => "ServiceNow basic authentication needs the password",
                "clientSecret" => "ServiceNow OAuth needs the client secret",
                _ => "ServiceNow needs the API key",
            }));
        }

        Ok(connector)
    }
}

/// 给页面的连接器：Jira 的还带 Webhook 的路径。
fn connector_json(connector: &TicketConnector) -> Value {
    let mut value = serde_json::to_value(connector).unwrap_or(Value::Null);

    if connector.system == TicketSystem::Jira
        && let Some(object) = value.as_object_mut()
    {
        object.insert(
            "webhook_path".into(),
            json!(format!("{WEBHOOK_PREFIX}{}", connector.id)),
        );
    }

    value
}

/// 飞书项目各平台的地址：页面按所选的平台显示，不能改。
#[permission("alerts.tickets.read")]
async fn list_sites(Extension(ctx): Extension<IamContext>) -> Result<Json<Value>> {
    let meegle: Vec<Value> = MEEGLE_SITES
        .iter()
        .map(|(site, url)| json!({ "key": site.as_str(), "url": url }))
        .collect();

    Ok(Json(json!({ "meegle": meegle })))
}

#[permission("alerts.tickets.read")]
async fn list_connectors(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let connectors = state
        .alerting
        .tickets
        .repository()
        .list_connectors(&ctx.org_id)
        .await?;

    let connectors: Vec<Value> = connectors.iter().map(connector_json).collect();

    Ok(Json(json!({ "connectors": connectors })))
}

#[permission("alerts.tickets.manage")]
async fn create_connector(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<ConnectorReq>,
) -> Result<Json<Value>> {
    let connector = req.into_connector(
        Id::new(),
        ctx.org_id.clone(),
        BTreeMap::new(),
        TimestampMicros::now(),
    )?;

    let saved = state
        .alerting
        .tickets
        .repository()
        .save_connector(connector)
        .await?;

    audit(&state, &ctx, "ticket_connector.created", &saved).await;
    Ok(Json(connector_json(&saved)))
}

#[permission("alerts.tickets.manage")]
async fn update_connector(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<ConnectorReq>,
) -> Result<Json<Value>> {
    let repository = state.alerting.tickets.repository();
    let existing = repository
        .get_connector(&ctx.org_id, &Id::from_string(id))
        .await?
        .ok_or_else(|| Error::not_found("ticket connector not found"))?;

    if req.system != existing.system {
        return Err(Error::invalid("a connector keeps its ticket system"));
    }

    let connector = req.into_connector(
        existing.id.clone(),
        existing.org_id.clone(),
        existing.secrets.clone(),
        existing.created_at,
    )?;

    let saved = repository.save_connector(connector).await?;

    audit(&state, &ctx, "ticket_connector.updated", &saved).await;
    Ok(Json(connector_json(&saved)))
}

#[permission("alerts.tickets.manage")]
async fn delete_connector(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let id = Id::from_string(id);

    if !state
        .alerting
        .tickets
        .repository()
        .delete_connector(&ctx.org_id, &id)
        .await?
    {
        return Err(Error::not_found("ticket connector not found"));
    }

    activity_audit::record(
        &state,
        &ctx,
        "ticket_connector.deleted",
        "ticket_connector",
        &id.0,
        json!({}),
    )
    .await;

    Ok(Json(json!({ "deleted": true })))
}

#[permission("alerts.tickets.manage")]
async fn test_connector(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let connector = state
        .alerting
        .tickets
        .repository()
        .get_connector(&ctx.org_id, &Id::from_string(id))
        .await?
        .ok_or_else(|| Error::not_found("ticket connector not found"))?;

    let detail = state.alerting.tickets.check(&connector).await?;

    Ok(Json(json!({ "ok": true, "detail": detail })))
}

async fn audit(state: &AppState, ctx: &IamContext, what: &str, connector: &TicketConnector) {
    activity_audit::record(
        state,
        ctx,
        what,
        "ticket_connector",
        &connector.id.0,
        json!({
            "name": connector.name,
            "system": connector.system,
            "enabled": connector.enabled,
            "auto": connector.auto.enabled,
        }),
    )
    .await;
}

#[resource_permission(
    action = "alerts.incidents.read",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn incident_tickets(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let tickets = state
        .alerting
        .tickets
        .repository()
        .list_for_incident(&incident.org_id, &incident.id)
        .await?;

    Ok(Json(json!({ "tickets": tickets })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TicketReq {
    connector_id: Id,
}

#[resource_permission(
    action = "alerts.incidents.tickets",
    resource = Incident,
    id = Id::from_string(id),
    bind = incident
)]
async fn create_ticket(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<TicketReq>,
) -> Result<Json<Value>> {
    let ticket = state
        .alerting
        .tickets
        .create_with(
            &incident.org_id,
            &req.connector_id,
            &incident,
            Some(ctx.principal_id()),
        )
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "incident.ticket_created",
        "incident",
        &incident.id.0,
        json!({ "system": ticket.system, "key": ticket.external_key }),
    )
    .await;

    Ok(Json(serde_json::to_value(ticket).unwrap_or(Value::Null)))
}

async fn jira_webhook(
    State(state): State<AppState>,
    Path(connector): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let signature = headers
        .get("X-Hub-Signature")
        .and_then(|value| value.to_str().ok());

    match state
        .alerting
        .tickets
        .jira_webhook(&Id::from_string(connector), signature, &body)
        .await
    {
        Ok(resolve) => {
            if let Some(incident_id) = resolve
                && let Err(error) = state
                    .alerting
                    .evaluator
                    .close_from_ticket(&incident_id, TimestampMicros::now())
                    .await
            {
                tracing::warn!(%incident_id, %error, "incident could not be resolved after its ticket was done");
            }

            StatusCode::NO_CONTENT.into_response()
        }
        Err(Error::NotFound(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(Error::Unauthorized(_) | Error::Forbidden(_)) => {
            StatusCode::UNAUTHORIZED.into_response()
        }
        Err(error) => error.into_response(),
    }
}
