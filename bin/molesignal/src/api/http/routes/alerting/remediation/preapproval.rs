// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Pre-approval of remediation actions:
//!
//! - `GET /alerts/remediation/preapproval`: the organization's switch and, for every action with a
//!   pre-approval, how many runs went out in its window and where it stands
//!   (`alerts.remediation.read`).
//! - `PUT /alerts/remediation/preapproval`: turn it on or off.
//! - `PUT /alerts/remediation/actions/{id}/preapproval`: set an action's pre-approval; the person
//!   who sets it is its first approver, and setting it again starts the approvers over.
//! - `DELETE /alerts/remediation/actions/{id}/preapproval`: take it away.
//! - `POST /alerts/remediation/actions/{id}/preapproval/confirm`: another person confirms it (an
//!   L3 action needs two people, and an edit of the action takes the approvals away).
//!
//! Changes take `alerts.remediation.preapprove`, and pre-approving also takes `agent.approve`, as
//! approving does: nobody pre-approves what they could not approve. A pre-approval is given by a
//! person signed in, not by an API token. Channels are checked like a log inspection's: the
//! connector is in the organization and takes the group or address. Every change is audited.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post, put},
};
use domain::{
    alerting::remediation::{
        RemediationAction, RemediationPreapproval,
        preapproval::{
            PreapprovalApprover, PreapprovalChannel, PreapprovalSettings, changes_what_was_approved,
        },
    },
    iam::{IamContext, IamScope, permission},
    notify::connector::NotifyTarget,
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::api::{AppState, http::routes::activity_audit};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/alerts/remediation/preapproval",
            get(read_settings).put(switch),
        )
        .route(
            "/alerts/remediation/actions/{id}/preapproval",
            put(set).delete(clear),
        )
        .route(
            "/alerts/remediation/actions/{id}/preapproval/confirm",
            post(confirm),
        )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwitchReq {
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreapprovalReq {
    services: Vec<String>,
    max_runs: u32,
    window_secs: u32,
    #[serde(default)]
    expires_at: Option<i64>,
    #[serde(default)]
    channels: Vec<PreapprovalChannel>,
}

#[permission("alerts.remediation.read")]
async fn read_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let repository = state.alerting.remediation.repository();
    let settings = repository.preapproval_settings(&ctx.org_id).await?;
    let now = TimestampMicros::now();
    let mut actions = Map::new();

    for action in repository.list_actions(&ctx.org_id).await? {
        let Some(preapproval) = &action.preapproval else {
            continue;
        };

        let used = repository
            .preapproved_runs_since(&ctx.org_id, &action.id, preapproval.window_start(now))
            .await?;

        actions.insert(
            action.id.0.clone(),
            json!({
                "used": used,
                "state": preapproval.state(settings.enabled, action.risk, used, now),
            }),
        );
    }

    Ok(Json(
        json!({ "enabled": settings.enabled, "actions": actions }),
    ))
}

#[permission("alerts.remediation.preapprove")]
async fn switch(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<SwitchReq>,
) -> Result<Json<Value>> {
    state
        .alerting
        .remediation
        .repository()
        .save_preapproval_settings(
            &ctx.org_id,
            PreapprovalSettings {
                enabled: req.enabled,
            },
            ctx.principal_id(),
            TimestampMicros::now(),
        )
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "remediation_preapproval.switched",
        "organization",
        &ctx.org_id.0,
        json!({ "enabled": req.enabled }),
    )
    .await;

    Ok(Json(json!({ "enabled": req.enabled })))
}

#[permission("alerts.remediation.preapprove")]
async fn set(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<PreapprovalReq>,
) -> Result<Json<Value>> {
    may_preapprove(&ctx)?;

    let action = action(&state, &ctx, id).await?;
    let now = TimestampMicros::now();

    let mut preapproval = RemediationPreapproval {
        services: req.services,
        max_runs: req.max_runs,
        window_secs: req.window_secs,
        expires_at: req.expires_at.map(TimestampMicros),
        channels: req.channels,
        approvers: vec![PreapprovalApprover {
            user_id: ctx.user_id.clone(),
            at: now,
        }],
        set_by: ctx.user_id.clone(),
        set_at: now,
    };

    preapproval.validate(&action, now)?;

    for channel in &preapproval.channels {
        let target = NotifyTarget {
            target_type: channel.target_type,
            value: channel.target.clone(),
            metadata: Default::default(),
        };

        state
            .alerting
            .notify
            .validate_direct_target(&ctx.org_id, &channel.connector_id, &target)
            .await?;
    }

    save(&state, &ctx, &action, Some(&preapproval)).await?;
    audit(
        &state,
        &ctx,
        "remediation_preapproval.set",
        &action,
        &preapproval,
    )
    .await;

    Ok(Json(json!({ "preapproval": preapproval })))
}

#[permission("alerts.remediation.preapprove")]
async fn clear(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let action = action(&state, &ctx, id).await?;

    save(&state, &ctx, &action, None).await?;

    activity_audit::record(
        &state,
        &ctx,
        "remediation_preapproval.cleared",
        "remediation_action",
        &action.id.0,
        json!({ "name": action.name }),
    )
    .await;

    Ok(Json(json!({ "cleared": true })))
}

#[permission("alerts.remediation.preapprove")]
async fn confirm(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    may_preapprove(&ctx)?;

    let action = action(&state, &ctx, id).await?;
    let now = TimestampMicros::now();

    let Some(mut preapproval) = action.preapproval.clone() else {
        return Err(Error::not_found(
            "this remediation action has no pre-approval",
        ));
    };

    // It must still fit the action and not have expired.
    preapproval.validate(&action, now)?;
    preapproval.confirm(&ctx.user_id, action.risk, now)?;

    save(&state, &ctx, &action, Some(&preapproval)).await?;
    audit(
        &state,
        &ctx,
        "remediation_preapproval.confirmed",
        &action,
        &preapproval,
    )
    .await;

    Ok(Json(json!({ "preapproval": preapproval })))
}

/// After an edit of the action: approvals of what changed do not carry over, so the pre-approval
/// waits for its approvers again. Returns the action as it now is.
pub(super) async fn after_update(
    state: &AppState,
    existing: &RemediationAction,
    mut saved: RemediationAction,
) -> Result<RemediationAction> {
    let Some(mut preapproval) = saved.preapproval.clone() else {
        return Ok(saved);
    };

    if preapproval.approvers.is_empty() || !changes_what_was_approved(existing, &saved) {
        return Ok(saved);
    }

    preapproval.approvers.clear();

    state
        .alerting
        .remediation
        .repository()
        .save_preapproval(&saved.org_id, &saved.id, Some(&preapproval))
        .await?;

    saved.preapproval = Some(preapproval);
    Ok(saved)
}

/// Pre-approving is approving ahead of time: it takes what approving takes, from a person.
fn may_preapprove(ctx: &IamContext) -> Result<()> {
    if ctx.scope != IamScope::Organization {
        return Err(Error::forbidden(
            "a pre-approval is given by a person signed in to the organization, not by an API token",
        ));
    }

    if !ctx.has_permission("agent.approve") {
        return Err(Error::forbidden(
            "pre-approving takes agent.approve, as approving does",
        ));
    }

    Ok(())
}

async fn action(state: &AppState, ctx: &IamContext, id: String) -> Result<RemediationAction> {
    state
        .alerting
        .remediation
        .repository()
        .get_action(&ctx.org_id, &Id::from_string(id))
        .await?
        .ok_or_else(|| Error::not_found("remediation action not found"))
}

async fn save(
    state: &AppState,
    ctx: &IamContext,
    action: &RemediationAction,
    preapproval: Option<&RemediationPreapproval>,
) -> Result<()> {
    let saved = state
        .alerting
        .remediation
        .repository()
        .save_preapproval(&ctx.org_id, &action.id, preapproval)
        .await?;

    if saved {
        Ok(())
    } else {
        Err(Error::not_found("remediation action not found"))
    }
}

async fn audit(
    state: &AppState,
    ctx: &IamContext,
    what: &str,
    action: &RemediationAction,
    preapproval: &RemediationPreapproval,
) {
    activity_audit::record(
        state,
        ctx,
        what,
        "remediation_action",
        &action.id.0,
        json!({
            "name": action.name,
            "services": preapproval.services,
            "maxRuns": preapproval.max_runs,
            "windowSecs": preapproval.window_secs,
            "expiresAt": preapproval.expires_at,
            "approvers": preapproval
                .approvers
                .iter()
                .map(|approver| approver.user_id.0.clone())
                .collect::<Vec<_>>(),
        }),
    )
    .await;
}
