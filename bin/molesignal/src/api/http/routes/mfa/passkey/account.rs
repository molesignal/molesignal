// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 账号页上的 Passkey，只接受登录了的浏览器（API token 不能改）。
//!
//! - `POST /me/passkeys/options`：添加前的确认（[`factor::check_adding`]）和注册参数。
//! - `POST /me/passkeys`：存下浏览器创建的 Passkey。账号的第一个第二因素同时开启多因素认证：给出
//!   一组恢复码（只显示这一次），别的会话作废。
//! - `PATCH /me/passkeys/{id}`：改名。
//! - `POST /me/passkeys/{id}/remove`：当场验证一次才能删；别的会话作废，最后一个第二因素删掉时
//!   多因素认证随之关闭。
//! - `POST /me/mfa/passkey-proof`：账号页上的操作要用 Passkey 确认时，先领一次验证。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::{patch, post},
};
use serde::Deserialize;
use webauthn_rs::prelude::{
    CreationChallengeResponse, RegisterPublicKeyCredential, RequestChallengeResponse,
};

use super::{
    PasskeyCeremony, checked_name, finish_registration, relying_party, start_proof,
    start_registration,
};
use crate::{
    api::{
        AppState,
        http::{
            routes::mfa::{
                account::{browser_session, reconfirmed},
                factor::{self, Checked, FactorProof},
                notice::{self, Notice},
                requirements,
            },
            signin_limit::SigninSource,
        },
    },
    app::iam::{IamContext, SignInSession, sessions_end_now, signin_limit::SigninError},
    shared::{Error, Result, ids::Id},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/me/mfa/passkey-proof", post(proof_options))
        .route("/me/passkeys/options", post(registration_options))
        .route("/me/passkeys", post(add))
        .route("/me/passkeys/{id}", patch(rename))
        .route("/me/passkeys/{id}/remove", post(remove))
}

async fn proof_options(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
) -> Result<Response> {
    browser_session(session)?;

    let ceremony: PasskeyCeremony<RequestChallengeResponse> =
        start_proof(&state, &ctx.user_id).await?;

    Ok(([(CACHE_CONTROL, "no-store")], Json(ceremony)).into_response())
}

#[derive(Debug, Default, Deserialize)]
struct OptionsRequest {
    /// 添加前的确认，见 [`factor::check_adding`]。
    #[serde(default)]
    proof: Option<FactorProof>,
}

async fn registration_options(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
    SigninSource(source): SigninSource,
    Json(request): Json<OptionsRequest>,
) -> Result<Response, SigninError> {
    let session = browser_session(session)?;
    relying_party(&state)?;
    let user = state.iam.service.users.get(&ctx.user_id).await?;

    factor::check_adding(
        &state,
        &user,
        &ctx.org_id,
        &session,
        request.proof.as_ref(),
        source,
    )
    .await?;

    let ceremony: PasskeyCeremony<CreationChallengeResponse> =
        start_registration(&state, &user).await?;

    Ok(([(CACHE_CONTROL, "no-store")], Json(ceremony)).into_response())
}

#[derive(Debug, Deserialize)]
struct AddRequest {
    ceremony: String,
    credential: Box<RegisterPublicKeyCredential>,
    #[serde(default)]
    name: String,
}

async fn add(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
    Json(request): Json<AddRequest>,
) -> Result<Response> {
    let session = browser_session(session)?;
    let user = state.iam.service.users.get(&ctx.user_id).await?;

    let added = finish_registration(
        &state,
        &user,
        &ctx.org_id,
        &request.ceremony,
        &request.credential,
        &request.name,
    )
    .await?;

    let cookies = reconfirmed(&state, &headers, &session, true)?;

    Ok((cookies, [(CACHE_CONTROL, "no-store")], Json(added)).into_response())
}

#[derive(Debug, Deserialize)]
struct RenameRequest {
    name: String,
}

async fn rename(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
    Path(id): Path<String>,
    Json(request): Json<RenameRequest>,
) -> Result<StatusCode> {
    browser_session(session)?;
    let name = checked_name(&request.name)?;

    let renamed = state
        .iam
        .passkeys
        .rename(&ctx.user_id, &Id::from_string(id), &name)
        .await?;

    if !renamed {
        return Err(Error::not_found("passkey"));
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
    Path(id): Path<String>,
    Json(proof): Json<FactorProof>,
) -> Result<Response> {
    let session = browser_session(session)?;
    let user = state.iam.service.users.get(&ctx.user_id).await?;
    let id = Id::from_string(id);

    let passkeys = state.iam.passkeys.list(&user.id).await?;

    if !passkeys.iter().any(|stored| stored.id == id) {
        return Err(Error::not_found("passkey"));
    }

    if passkeys.len() == 1 && state.iam.mfa_factors.totp(&user.id).await?.is_none() {
        requirements::ensure_may_remove_last_factor(&state, &user.id).await?;
    }

    if let Checked::Failed = factor::check(&state, &user, &ctx.org_id, &proof).await? {
        return Err(Error::invalid(factor::rejection(&proof)));
    }

    let still_on = state
        .iam
        .passkeys
        .remove(&user.id, &id, sessions_end_now())
        .await?
        .ok_or_else(|| Error::not_found("passkey"))?;

    let cookies = reconfirmed(&state, &headers, &session, true)?;

    notice::audit(
        &state,
        &ctx.org_id,
        &user.id,
        "mfa.passkey.remove",
        &user.id,
        serde_json::json!({ "passkeyId": id.0 }),
    )
    .await;

    if !still_on {
        notice::tell_owner(&state, &user, &ctx.org_id, Notice::Disabled).await;
    }

    Ok((StatusCode::NO_CONTENT, cookies, ()).into_response())
}
