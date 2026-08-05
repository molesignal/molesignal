// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM invitation lifecycle endpoints.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::invitations::Invitation,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/invitations", get(list).post(create))
        .route("/invitations/{id}/resend", axum::routing::post(resend))
        .route("/invitations/{id}/revoke", axum::routing::post(revoke))
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    email: String,
    #[serde(default)]
    role_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct Resp {
    id: String,
    org_id: String,
    email: String,
    role_id: String,
    role_key: String,
    role_name: String,
    inviter_id: String,
    status: String,
    sent_at_micros: i64,
    updated_at_micros: i64,
}

async fn to_resp(state: &AppState, invitation: Invitation) -> Result<Resp> {
    let role = state
        .iam
        .access
        .repository()
        .role_summary(&invitation.org_id, &invitation.role_id)
        .await?
        .ok_or_else(|| Error::internal("invitation references a missing IAM role"))?;
    Ok(Resp {
        id: invitation.id.0,
        org_id: invitation.org_id.0,
        email: invitation.email,
        role_id: role.id.0,
        role_key: role.key,
        role_name: role.name,
        inviter_id: invitation.inviter_id.0,
        status: invitation.status,
        sent_at_micros: invitation.sent_at.0,
        updated_at_micros: invitation.updated_at.0,
    })
}

#[permission("org.members.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<Resp>>> {
    let invitations = state.iam.invitations.list(&ctx.org_id).await?;
    let mut responses = Vec::with_capacity(invitations.len());
    for invitation in invitations {
        responses.push(to_resp(&state, invitation).await?);
    }
    Ok(Json(responses))
}

#[permission("org.members.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<Resp>> {
    let email = req.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(Error::invalid("email must be valid"));
    }
    // org 配置了邮箱域白名单时，拒绝邀请名单外的域（空名单 = 不限制）。
    super::email_domains::ensure_email_allowed(&state, &ctx.org_id, &email).await?;
    let role_id = match req.role_id {
        Some(role_id) => Id::from_string(role_id),
        None => {
            state
                .iam
                .service
                .iam_memberships
                .role_id_for_purpose(&ctx.org_id, "self_service_signup")
                .await?
        }
    };
    state
        .iam
        .access
        .repository()
        .role_summary(&ctx.org_id, &role_id)
        .await?
        .ok_or_else(|| Error::invalid("role_id must reference an IAM role in this organization"))?;
    let now = TimestampMicros::now();
    let invitation = Invitation {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        email,
        role_id,
        inviter_id: ctx.user_id.clone(),
        status: "pending".to_string(),
        sent_at: now,
        updated_at: now,
    };
    let invitation = state.iam.invitations.create(invitation).await?;
    Ok(Json(to_resp(&state, invitation).await?))
}

#[permission("org.members.manage")]
async fn resend(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Resp>> {
    let now = TimestampMicros::now();
    let invitation = state
        .iam
        .invitations
        .update_status(&ctx.org_id, &Id::from_string(id), "pending", Some(now), now)
        .await?;
    Ok(Json(to_resp(&state, invitation).await?))
}

#[permission("org.members.manage")]
async fn revoke(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Resp>> {
    let now = TimestampMicros::now();
    let invitation = state
        .iam
        .invitations
        .update_status(&ctx.org_id, &Id::from_string(id), "revoked", None, now)
        .await?;
    Ok(Json(to_resp(&state, invitation).await?))
}
