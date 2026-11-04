// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把浏览器跳转登录的结果交给前端。
//!
//! callback 把结果（要登录的身份、待本人确认的关联或失败原因）存成一次性交接凭据，303 回
//! 前端 `/signin?sso=<凭据>`；前端 `POST /auth/sso/exchange` 用凭据取结果：`{"login": …}` 或
//! `{"link": …}`，带上登录前要去的 `next`。登录在这一步才写进 Cookie，交接期间服务端只存
//! 身份、不存 token；凭据不进地址栏以外的地方，失败原因也只能由服务端写入，链接里带不进
//! 任意文字。

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect, Response},
    routing::post,
};
use serde::Deserialize;

use super::{provision::SignIn, random_token};
use crate::{
    api::{AppState, http::routes::auth::SessionView},
    app::iam::SignInMethod,
    infra::sso::SsoHandoff,
    shared::{Error, Result, ids::Id},
};

/// 前端登录页；凭据放在查询参数 `sso` 里。
const SIGNIN_PATH: &str = "/signin";

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/auth/sso/exchange", post(exchange))
}

/// 存下登录结果，把浏览器送回前端登录页。
pub(super) async fn finish(
    state: &AppState,
    outcome: Result<SignIn>,
    next: Option<String>,
) -> Response {
    if let Err(error) = &outcome {
        tracing::warn!(%error, "sso sign-in failed");
    }

    let body = outcome.and_then(|outcome| {
        serde_json::to_value(outcome).map_err(|e| Error::internal(format!("sso login encode: {e}")))
    });

    let code = random_token();

    match state
        .iam
        .sso_state_store
        .put_handoff(&code, SsoHandoff { body, next })
        .await
    {
        Ok(()) => Redirect::to(&format!("{SIGNIN_PATH}?sso={code}")).into_response(),
        Err(error) => error.into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct ExchangeRequest {
    code: String,
    /// 记住我：离开去身份源之前在登录页选的。
    #[serde(default)]
    remember: bool,
}

async fn exchange(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ExchangeRequest>,
) -> Result<Response> {
    let handoff = state
        .iam
        .sso_state_store
        .take_handoff(request.code.trim())
        .await?
        .ok_or_else(|| Error::unauthorized("this sign-in link has expired; sign in again"))?;

    let mut body = handoff.body?;

    let cookies = match body.get("login") {
        Some(login) => {
            let session = SessionView::deserialize(login)
                .map_err(|e| Error::internal(format!("sso login decode: {e}")))?;

            // 跳转登录只有 OIDC / OAuth 2.0 / SAML，登录进的就是服务商所在的组织。
            let method = SignInMethod::single_sign_on(Id::from_string(session.org_id.clone()));

            Some(session.sign_in(&state, &headers, request.remember, method)?)
        }
        None => None,
    };

    if let (Some(next), Some(object)) = (handoff.next, body.as_object_mut()) {
        object.insert("next".into(), next.into());
    }

    Ok((cookies, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}
