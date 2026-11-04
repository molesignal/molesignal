// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 管理员重置成员的多因素认证：`POST /users/{id}/mfa/reset`。
//!
//! 清掉身份验证器和恢复码，成员所有设备上的登录都作废，之后用密码登录、重新开启。账号是整个
//! 实例共用的，所以组织管理员只能重置只属于本组织的成员，平台管理员可以重置本组织的任何成员
//! （[`account_of_current_org`]）。平台管理员自己的只能在服务器上用 `molesignal reset-mfa`
//! 重置：没有比它更高的人来确认。

use axum::{
    Extension, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::post,
};

use super::notice::{self, Notice};
use crate::{
    api::{
        AppState,
        http::routes::iam::{directory::account_of_current_org, grant_limit},
    },
    app::iam::{IamContext, sessions_end_now},
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/users/{id}/mfa/reset", post(reset))
}

async fn reset(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    if !ctx.has_permission("org.members.mfa_reset") {
        return Err(Error::forbidden(
            "org.members.mfa_reset permission is required to reset a member's multi-factor authentication",
        ));
    }

    let target = Id::from_string(id);

    if target == ctx.user_id {
        return Err(Error::invalid(
            "turn your own multi-factor authentication off under Account settings → Security",
        ));
    }

    account_of_current_org(&state, &ctx, &target).await?;
    grant_limit::require_member_within_reach(&state, &ctx, &target).await?;

    if state.iam.platform_administrators.is_active(&target).await? {
        return Err(Error::forbidden(
            "the platform administrator's multi-factor authentication is reset on the server with `molesignal reset-mfa`",
        ));
    }

    let enabled = state
        .iam
        .mfa_factors
        .enabled_among(std::slice::from_ref(&target))
        .await?;

    if !enabled.contains(&target) {
        return Err(Error::conflict(
            "this member has not turned on multi-factor authentication",
        ));
    }

    let member = state.iam.service.users.get(&target).await?;
    let admin = state.iam.service.users.get(&ctx.user_id).await?;

    state
        .iam
        .mfa_factors
        .remove_all(&target, sessions_end_now())
        .await?;

    notice::audit(
        &state,
        &ctx.org_id,
        &ctx.user_id,
        "mfa.reset",
        &target,
        serde_json::json!({ "email": member.email }),
    )
    .await;

    notice::tell_owner(
        &state,
        &member,
        &ctx.org_id,
        Notice::Reset {
            admin: &admin.display_name,
        },
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}
