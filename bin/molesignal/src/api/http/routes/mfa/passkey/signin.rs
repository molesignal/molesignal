// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 用 Passkey 登录。
//!
//! - 第二步：`POST /auth/signin/verify/passkey` 为等着的那次登录领一次 Passkey 验证（只接受这个
//!   账号的凭据），再和验证码一样请求 `POST /auth/signin/verify`。
//! - 免密码：`POST /auth/signin/passkey/options` 不需要账号，登录页一打开就能拿到，配合浏览器的
//!   自动填充；`POST /auth/signin/passkey` 从凭据认出账号。Passkey 要求用户验证，本身就算过了
//!   多因素认证，不再要第二步。

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use webauthn_rs::prelude::{DiscoverableAuthentication, DiscoverableKey, PublicKeyCredential};

use super::{
    PasskeyCeremony, begin_ceremony, record_use, relying_party, start_proof, take_ceremony,
};
use crate::{
    api::{
        AppState,
        http::routes::{
            auth::{SessionView, signin_target},
            mfa::signin::expired,
        },
    },
    app::iam::{
        SignInMethod,
        mfa::{challenge_digest, passkey},
    },
    infra::mfa::MfaChallengePurpose,
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/signin/verify/passkey", post(second_step_options))
        .route("/auth/signin/passkey/options", post(options))
        .route("/auth/signin/passkey", post(sign_in))
}

#[derive(Debug, Deserialize)]
struct SecondStepRequest {
    challenge: String,
}

async fn second_step_options(
    State(state): State<AppState>,
    Json(request): Json<SecondStepRequest>,
) -> Result<Response> {
    let user_id = state
        .iam
        .mfa_challenges
        .find_by_code(
            &challenge_digest(&request.challenge),
            TimestampMicros::now(),
        )
        .await?
        .filter(|challenge| challenge.purpose == MfaChallengePurpose::SignIn)
        .and_then(|challenge| challenge.user_id)
        .ok_or_else(expired)?;

    let ceremony = start_proof(&state, &user_id).await?;

    Ok(([(CACHE_CONTROL, "no-store")], Json(ceremony)).into_response())
}

async fn options(State(state): State<AppState>) -> Result<Response> {
    let webauthn = relying_party(&state)?;

    let (options, authentication) = webauthn
        .start_discoverable_authentication()
        .map_err(|error| Error::internal(format!("passkey sign-in options: {error}")))?;

    let ceremony = begin_ceremony(
        &state,
        None,
        MfaChallengePurpose::PasskeySignIn,
        passkey::stored_state(&authentication)?,
    )
    .await?;

    Ok((
        [(CACHE_CONTROL, "no-store")],
        Json(PasskeyCeremony { ceremony, options }),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
struct SignInRequest {
    ceremony: String,
    credential: Box<PublicKeyCredential>,
    /// 记住我：关掉浏览器后仍保持登录。
    #[serde(default)]
    remember: bool,
}

async fn sign_in(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SignInRequest>,
) -> Result<Response> {
    let webauthn = relying_party(&state)?;

    let pending = take_ceremony(
        &state,
        &request.ceremony,
        MfaChallengePurpose::PasskeySignIn,
        None,
    )
    .await?;

    let authentication: DiscoverableAuthentication = passkey::read_state(pending.webauthn_state)?;

    let (handle, credential_id) = webauthn
        .identify_discoverable_authentication(&request.credential)
        .map_err(|_| not_recognized())?;

    let stored = state
        .iam
        .passkeys
        .by_credential(credential_id)
        .await?
        .filter(|stored| passkey::user_handle(&stored.user_id) == handle)
        .ok_or_else(not_recognized)?;

    let result = webauthn
        .finish_discoverable_authentication(
            &request.credential,
            authentication,
            &[DiscoverableKey::from(&stored.passkey)],
        )
        .map_err(|_| Error::unauthorized("the passkey could not be verified"))?;

    let user = state.iam.service.users.get(&stored.user_id).await?;
    state.iam.service.ensure_user_access(&user.id).await?;
    let method = SignInMethod::passkey();
    let (org, roles, scope) = signin_target(&state, &user.id, &method).await?;

    record_use(&state, stored, &result, TimestampMicros::now()).await?;

    let session = SessionView::new(user, org, roles, scope);
    let cookies = session.sign_in(&state, &headers, request.remember, method)?;

    Ok((
        cookies,
        [(CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "login": session })),
    )
        .into_response())
}

/// 验证器里还留着、这里已经没有的 Passkey：页面据此让验证器删掉它。
fn not_recognized() -> Error {
    Error::unauthorized("this passkey is not registered here")
}
