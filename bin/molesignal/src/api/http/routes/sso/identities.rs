// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 账号页的「已关联的登录方式」。
//!
//! - `GET /me/sso-identities`：当前账号绑定的外部身份，最近登录的在前。
//! - `DELETE /me/sso-identities/{id}`：解除一个绑定。账号没有密码时，不能解除最后一个绑定，
//!   否则这个账号再也登录不了。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
};
use serde::Serialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{IamScope, SsoPlatform, SsoProviderKind, User},
    infra::sso::SsoIdentityView,
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/me/sso-identities", get(list))
        .route("/me/sso-identities/{id}", delete(unlink))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityResponse {
    id: String,
    provider_id: String,
    provider_name: String,
    kind: SsoProviderKind,
    platform: SsoPlatform,
    org_id: String,
    org_name: String,
    linked_at: i64,
    last_login_at: i64,
    /// 解除后账号还能登录。
    can_unlink: bool,
}

impl IdentityResponse {
    fn new(identity: SsoIdentityView, can_unlink: bool) -> Self {
        Self {
            id: identity.id.0,
            provider_id: identity.provider_id.0,
            provider_name: identity.provider_name,
            kind: identity.kind,
            platform: identity.platform,
            org_id: identity.org_id.0,
            org_name: identity.org_name,
            linked_at: identity.created_at.0,
            last_login_at: identity.last_login_at.0,
            can_unlink,
        }
    }
}

async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<IdentityResponse>>> {
    let identities = state.iam.sso_identities.list_for_user(&ctx.user_id).await?;
    let user = state.iam.service.users.get(&ctx.user_id).await?;
    let unlinkable = can_unlink(&user, identities.len());

    Ok(Json(
        identities
            .into_iter()
            .map(|identity| IdentityResponse::new(identity, unlinkable))
            .collect(),
    ))
}

async fn unlink(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    if ctx.scope == IamScope::ApiToken {
        return Err(Error::forbidden(
            "sign in to change how this account signs in",
        ));
    }

    let identity = state.iam.sso_identities.get(&Id::from_string(id)).await?;

    // 别人的绑定按不存在处理，不透露它在不在。
    if identity.user_id != ctx.user_id {
        return Err(Error::not_found("sso identity"));
    }

    let user = state.iam.service.users.get(&ctx.user_id).await?;
    let linked = state.iam.sso_identities.list_for_user(&ctx.user_id).await?;

    if !can_unlink(&user, linked.len()) {
        return Err(Error::invalid(
            "this is the only way this account signs in; set a password first",
        ));
    }

    state.iam.sso_identities.delete(&identity.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 没有密码的账号至少要留一个绑定，否则再也登录不了。
fn can_unlink(user: &User, linked: usize) -> bool {
    !user.password_hash.is_empty() || linked > 1
}
