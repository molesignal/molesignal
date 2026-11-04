// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 单个账号：查看、改资料、停用、删除，以及审批自助注册的账号。
//!
//! 管理员只能处理本组织的成员；改账号本身（改名、换头像、停用、删除）只对只属于本组织的成员
//! 开放，见 [`super::member_scope`]。

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;

use super::{
    super::grant_limit, UserView, account_of_current_org, approval_emails::notify_user_decision,
    ensure_user_removable, is_root_user, member_of_current_org, require_key,
};
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::UserStatus,
    shared::{Error, Result, ids::Id},
};

/// 审批自助注册的 pending 用户 → active（Owner/Admin）。
pub(super) async fn approve_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<UserView>> {
    require_key(&ctx, "org.members.approve")?;
    let target = Id::from_string(id);
    member_of_current_org(&state, &ctx, &target).await?;

    state
        .iam
        .service
        .users
        .set_status(&target, UserStatus::Active)
        .await?;

    let user = state.iam.service.users.get(&target).await?;
    notify_user_decision(&state, &ctx.org_id, &user.email, true).await;

    Ok(Json(UserView::from_user(
        user,
        &state.iam.service.auth_settings().root_email,
    )))
}

/// 拒绝 pending 用户 → rejected（保留记录、不可登录）+ 邮件通知本人（Owner/Admin）。
pub(super) async fn reject_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<UserView>> {
    require_key(&ctx, "org.members.approve")?;
    let target = Id::from_string(id);
    member_of_current_org(&state, &ctx, &target).await?;

    state
        .iam
        .service
        .users
        .set_status(&target, UserStatus::Rejected)
        .await?;

    let user = state.iam.service.users.get(&target).await?;
    notify_user_decision(&state, &ctx.org_id, &user.email, false).await;

    Ok(Json(UserView::from_user(
        user,
        &state.iam.service.auth_settings().root_email,
    )))
}

pub(super) async fn get_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<UserView>> {
    let target = Id::from_string(id);

    // 自己，或拥有成员读取权限、且对方是本组织成员的管理员。
    if target != ctx.user_id && !ctx.has_permission("org.members.read") {
        return Err(Error::forbidden("not allowed"));
    }

    member_of_current_org(&state, &ctx, &target).await?;
    let user = state.iam.service.users.get(&target).await?;
    let mut view = UserView::from_user(user, &state.iam.service.auth_settings().root_email);

    view.mfa_enabled = state
        .iam
        .mfa_factors
        .enabled_among(std::slice::from_ref(&target))
        .await?
        .contains(&target);

    Ok(Json(view))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct UpdateUserReq {
    /// Kept only to return an explicit immutable-field error to legacy or
    /// hand-written clients. A user's sign-in identity is never profile data.
    email: Option<String>,
    display_name: Option<String>,
    avatar_url: Option<String>,
    disabled: Option<bool>,
}

pub(super) async fn update_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserReq>,
) -> Result<Json<UserView>> {
    let target = Id::from_string(id);

    if target != ctx.user_id && !ctx.has_permission("org.members.update") {
        return Err(Error::forbidden("not allowed"));
    }

    account_of_current_org(&state, &ctx, &target).await?;
    let mut user = state.iam.service.users.get(&target).await?;

    if req.email.is_some() {
        return Err(Error::invalid("user email is immutable"));
    }

    if let Some(display_name) = req.display_name {
        let display_name = display_name.trim().to_string();

        if display_name.is_empty() {
            return Err(Error::invalid("displayName must not be empty"));
        }

        user.display_name = display_name;
    }

    if let Some(avatar_url) = req.avatar_url {
        user.avatar_url = normalize_avatar_url(avatar_url)?;
    }

    if let Some(disabled) = req.disabled {
        require_key(&ctx, "org.members.update")?;

        if target == ctx.user_id && disabled {
            return Err(Error::invalid("cannot disable your own account"));
        }

        if is_root_user(&user, &state.iam.service.auth_settings().root_email) {
            return Err(Error::forbidden("root user cannot be disabled"));
        }

        if disabled {
            ensure_platform_admin_remains(&state, &target).await?;
            grant_limit::require_member_within_reach(&state, &ctx, &target).await?;
        }

        user.disabled = disabled;
    }

    let user = state.iam.service.users.update(user).await?;

    Ok(Json(UserView::from_user(
        user,
        &state.iam.service.auth_settings().root_email,
    )))
}

pub(super) async fn delete_user(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    require_key(&ctx, "org.members.remove")?;
    let target = Id::from_string(id);

    if target == ctx.user_id {
        return Err(Error::invalid("cannot delete your own account"));
    }

    account_of_current_org(&state, &ctx, &target).await?;
    let user = state.iam.service.users.get(&target).await?;
    ensure_user_removable(&user, &state.iam.service.auth_settings().root_email)?;
    ensure_platform_admin_remains(&state, &target).await?;
    grant_limit::require_member_within_reach(&state, &ctx, &target).await?;
    state.iam.service.users.delete(&target).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ensure_platform_admin_remains(state: &AppState, user_id: &Id) -> Result<()> {
    if state.iam.platform_administrators.is_active(user_id).await? {
        Err(Error::conflict(
            "configured root user cannot be made unusable",
        ))
    } else {
        Ok(())
    }
}

fn normalize_avatar_url(value: String) -> Result<Option<String>> {
    let value = value.trim().to_string();

    if value.is_empty() {
        return Ok(None);
    }

    if value.len() > 2048 {
        return Err(Error::invalid("avatarUrl must be at most 2048 characters"));
    }

    if !(value.starts_with("https://") || value.starts_with("http://")) {
        return Err(Error::invalid("avatarUrl must be an http(s) URL"));
    }

    Ok(Some(value))
}
