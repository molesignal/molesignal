// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 登录会话：换新的 access token、退出登录、页面读取当前会话。

use axum::{
    Extension, Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};

use super::SessionView;
use crate::{
    api::{
        AppState,
        http::{
            routes::mfa,
            session_cookie::{self, SessionCookies},
        },
    },
    app::iam::IamContext,
    shared::{Error, Result},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/refresh", post(refresh))
        .route("/auth/signout", post(signout))
        .route("/auth/session", get(current))
}

/// 用 refresh token 换新的 access token。refresh token 不变，登录到原定的时间结束；
/// 换不成（过期、账号停用、组织的登录要求不再满足）时一并删掉 Cookie。
async fn refresh(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(error) = session_cookie::ensure_same_origin(&headers) {
        return error.into_response();
    }

    match renew(&state, &headers).await {
        Ok(cookies) => (StatusCode::NO_CONTENT, cookies, ()).into_response(),
        Err(error @ (Error::Unauthorized(_) | Error::Forbidden(_))) => {
            (session_cookie::clear(&state, &headers), error).into_response()
        }
        Err(error) => error.into_response(),
    }
}

async fn renew(state: &AppState, headers: &HeaderMap) -> Result<SessionCookies> {
    let token = session_cookie::refresh_token(headers)
        .ok_or_else(|| Error::unauthorized("not signed in"))?;

    let session = state
        .iam
        .service
        .verify_with_current_secrets(token, |iam| iam.verify_refresh_token(token))
        .await?;

    state.iam.service.admit_session(&session).await?;
    mfa::ensure_still_admitted(state, &session).await?;

    session_cookie::renew(state, headers, &session)
}

/// 退出登录：让浏览器删掉两个 Cookie。服务端不保存 token，删掉 Cookie 就退出了。
async fn signout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    session_cookie::ensure_same_origin(&headers)?;

    Ok((
        StatusCode::NO_CONTENT,
        session_cookie::clear(&state, &headers),
        (),
    )
        .into_response())
}

/// 当前会话的身份，给没有存下身份的页面（新开的标签页、渲染报表的无头浏览器）。
async fn current(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<SessionView>> {
    let user = state.iam.service.users.get(&context.user_id).await?;
    let organization = state.iam.service.orgs.get(&context.org_id).await?;

    Ok(Json(SessionView::new(
        user,
        organization,
        context.roles,
        context.scope,
    )))
}
