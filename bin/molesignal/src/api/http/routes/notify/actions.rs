// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! One-time action links (see `domain::notify::action_links`). The web app's confirm page reads
//! what a link does; a press there acknowledges or resolves the alert as the person the
//! notification went to.
//!
//! - `GET  /api/v1/public/notify/actions/{token}`: what the link does. Never acts, so mail
//!   scanners that fetch links do no harm.
//! - `POST /api/v1/public/notify/actions/{token}`: does it, once.
//!
//! The token is the authority: only its digest is stored, it lasts a day and acts once. The
//! person still needs `alerts.incidents.acknowledge` on the alert at the time of the press.
//! Refusals carry a `code`: `expired` (404), `used` and `inactive` (409), `no_access` (403).

use agent::runs::RunPrincipal;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use domain::{
    alerting::{
        incident::{Incident, IncidentStatus},
        timeline::ActionVia,
    },
    iam::{IamContext, IamScope},
    notify::{action_links::ActionGrant, reply::ReplyAction},
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde_json::{Value, json};

use crate::{
    api::{AppState, http::middleware::authorize_resource},
    app::notify::action_token_hash,
};

pub(super) fn public_routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/public/notify/actions/{token}",
        get(peek).post(press),
    )
}

fn refused(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "code": code }))).into_response()
}

fn action_name(action: ReplyAction) -> &'static str {
    match action {
        ReplyAction::Acknowledge => "acknowledge",
        ReplyAction::Resolve => "resolve",
    }
}

/// The link's grant, or the refusal to answer with.
async fn grant(
    state: &AppState,
    token: &str,
) -> Result<std::result::Result<ActionGrant, Response>> {
    let found = state
        .alerting
        .action_links
        .get(&action_token_hash(token), TimestampMicros::now())
        .await?;

    Ok(found.ok_or_else(|| refused(StatusCode::NOT_FOUND, "expired")))
}

/// The language the recipient reads MoleSignal in: their own, or their workspace's.
async fn language(state: &AppState, grant: &ActionGrant) -> String {
    if let Ok(Some(personal)) = state
        .iam
        .user_preferences
        .get_optional(&grant.recipient_user_id)
        .await
    {
        return personal.language;
    }

    state
        .iam
        .workspace_preference_defaults
        .get(&grant.organization_id)
        .await
        .map(|workspace| workspace.language)
        .unwrap_or_default()
}

async fn peek(State(state): State<AppState>, Path(token): Path<String>) -> Result<Response> {
    let grant = match grant(&state, &token).await? {
        Ok(grant) => grant,
        Err(refusal) => return Ok(refusal),
    };

    let incident = match state.alerting.service.get_incident(&grant.alert_id).await {
        Ok(incident) if incident.org_id == grant.organization_id => incident,
        Ok(_) | Err(Error::NotFound(_)) => return Ok(refused(StatusCode::NOT_FOUND, "expired")),
        Err(error) => return Err(error),
    };

    Ok(Json(json!({
        "action": action_name(grant.action),
        "alert": {
            "id": incident.id.as_str(),
            "title": incident.summary,
            "severity": serde_json::to_value(incident.severity).unwrap_or(Value::Null),
            "status": incident.status.as_str(),
            "startedAt": incident.created_at.0,
        },
        "expiresAt": grant.expires_at.0,
        "usedAt": grant.used_at.map(|at| at.0),
        "language": language(&state, &grant).await,
    }))
    .into_response())
}

async fn press(State(state): State<AppState>, Path(token): Path<String>) -> Result<Response> {
    let grant = match grant(&state, &token).await? {
        Ok(grant) => grant,
        Err(refusal) => return Ok(refusal),
    };

    if grant.used_at.is_some() {
        return Ok(refused(StatusCode::CONFLICT, "used"));
    }

    let Ok(ctx) = responder(&state, &grant.organization_id, &grant.recipient_user_id).await else {
        return Ok(refused(StatusCode::FORBIDDEN, "no_access"));
    };

    let incident = match authorize_resource::<Incident>(
        &state,
        &ctx,
        grant.alert_id.clone(),
        "alerts.incidents.acknowledge",
    )
    .await
    {
        Ok(incident) if incident.org_id == grant.organization_id => incident,
        Ok(_) | Err(Error::NotFound(_)) => return Ok(refused(StatusCode::NOT_FOUND, "expired")),
        Err(Error::Forbidden(_)) => return Ok(refused(StatusCode::FORBIDDEN, "no_access")),
        Err(error) => return Err(error),
    };

    let still_possible = match grant.action {
        ReplyAction::Acknowledge => incident.status == IncidentStatus::Open,
        ReplyAction::Resolve => incident.is_active(),
    };

    if !still_possible {
        return Ok(refused(StatusCode::CONFLICT, "inactive"));
    }

    let now = TimestampMicros::now();
    let hash = action_token_hash(&token);

    // Only the first press acts.
    if !state.alerting.action_links.mark_used(&hash, now).await? {
        return Ok(refused(StatusCode::CONFLICT, "used"));
    }

    let service = &state.alerting.service;
    let user_id = grant.recipient_user_id.clone();

    let done = match grant.action {
        ReplyAction::Acknowledge => {
            service
                .acknowledge(&incident.id, user_id, now, ActionVia::Web)
                .await
        }
        ReplyAction::Resolve => {
            service
                .resolve(&incident.id, user_id, now, ActionVia::Web)
                .await
        }
    };

    match done {
        Ok(incident) => Ok(Json(json!({ "status": incident.status.as_str() })).into_response()),
        Err(Error::Conflict(_)) => Ok(refused(StatusCode::CONFLICT, "inactive")),
        Err(error) => Err(error),
    }
}

/// The recipient's standing in the organization now: the account and the organization still
/// work, then their membership and permissions.
async fn responder(state: &AppState, org_id: &Id, user_id: &Id) -> Result<IamContext> {
    state.iam.service.ensure_user_access(user_id).await?;
    state.iam.service.ensure_organization_access(org_id).await?;

    let principal = RunPrincipal {
        user_id: user_id.clone(),
        scope: IamScope::Organization,
        credential_token_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
    };

    let mut context = principal.unresolved_context(org_id);
    state.iam.access.enrich_context(&mut context).await?;

    Ok(context)
}
