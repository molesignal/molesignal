// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 账号自己的多因素认证设置，只接受登录了的浏览器（API token 不能改）。
//!
//! - `GET  /me/mfa`：身份验证器开没开、什么时候开的，账号的 Passkey，还剩几个恢复码，加第二因素
//!   要过哪一关，哪些组织要求多因素认证。
//! - `POST /me/mfa/totp/setup`：先确认是本人（[`factor::check_adding`]），再给新的密钥、扫码地址
//!   和二维码，等第一个验证码。
//! - `POST /me/mfa/totp/confirm`：第一个验证码对上才生效。开启了多因素认证时给出一组恢复码，只
//!   显示这一次。
//! - `POST /me/mfa/totp/disable`、`POST /me/mfa/recovery-codes`：当场验证一次才能关掉、换一组
//!   恢复码。还有 Passkey 时，关掉身份验证器不会关掉多因素认证。
//! - `POST /me/sessions/revoke-others`：别的设备退出登录。
//!
//! 开启多因素认证、换验证器、关掉一种第二因素、退出别的设备都会让账号此前开始的登录作废；当前
//! 浏览器同时拿到重新确认的会话，自己不会被踢出去。验证码不对时回答 400：这些接口在登录之后，
//! 401 会被页面当成会话过期。Passkey 的接口在 [`super::passkey`]。

use axum::{
    Extension, Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use super::{
    enabled,
    factor::{self, AddingRequires, Checked, FactorProof},
    notice::{self, Notice},
    passkey::PasskeyView,
    requirements,
    totp_setup::{self, SetupResponse},
};
use crate::{
    api::{
        AppState,
        http::{session_cookie, signin_limit::SigninSource},
    },
    app::iam::{
        IamContext, SignInSession,
        mfa::{
            passkey::{self, Unavailable},
            recovery,
        },
        sessions_end_now,
        signin_limit::SigninError,
    },
    domain::iam::User,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/me/mfa", get(status))
        .route("/me/mfa/totp/setup", post(setup))
        .route("/me/mfa/totp/confirm", post(confirm))
        .route("/me/mfa/totp/disable", post(disable))
        .route("/me/mfa/recovery-codes", post(regenerate))
        .route("/me/sessions/revoke-others", post(revoke_others))
}

/// 这些设置只在登录了的浏览器里改。
pub(super) fn browser_session(session: Option<Extension<SignInSession>>) -> Result<SignInSession> {
    session
        .map(|Extension(session)| session)
        .ok_or_else(|| Error::forbidden("security settings are changed from a signed-in browser"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MfaStatus {
    totp_enabled: bool,
    totp_enabled_at: Option<i64>,
    recovery_codes_left: i64,
    passkeys: Vec<PasskeyView>,
    /// Passkey 为什么不可用；可用时为空。
    passkeys_unavailable: Option<Unavailable>,
    /// 加一种第二因素前要过哪一关。
    adding_requires: AddingRequires,
    /// 账号所在的组织里要求多因素认证的那些：有的话不能去掉最后一种第二因素。
    required_by: Vec<String>,
}

async fn status(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
) -> Result<Json<MfaStatus>> {
    let user = state.iam.service.users.get(&ctx.user_id).await?;
    let totp = state.iam.mfa_factors.totp(&user.id).await?;
    let passkeys = state.iam.passkeys.list(&user.id).await?;
    let mfa_on = totp.is_some() || !passkeys.is_empty();

    let recovery_codes_left = if mfa_on {
        state.iam.mfa_factors.recovery_codes_left(&user.id).await?
    } else {
        0
    };

    let adding_requires = factor::adding_requires(
        &user,
        mfa_on,
        session.as_ref().map(|Extension(session)| session),
        TimestampMicros::now(),
    );

    Ok(Json(MfaStatus {
        totp_enabled: totp.is_some(),
        totp_enabled_at: totp.map(|totp| totp.created_at.0),
        recovery_codes_left,
        passkeys: passkeys.iter().map(PasskeyView::from).collect(),
        passkeys_unavailable: passkey::relying_party(&state.platform.external_url).err(),
        adding_requires,
        required_by: requirements::required_by(&state, &user.id).await?,
    }))
}

#[derive(Debug, Default, Deserialize)]
struct SetupRequest {
    /// 确认是本人，见 [`factor::check_adding`]。
    #[serde(default)]
    proof: Option<FactorProof>,
}

async fn setup(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
    SigninSource(source): SigninSource,
    Json(request): Json<SetupRequest>,
) -> Result<Response, SigninError> {
    let session = browser_session(session)?;
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

    let setup: SetupResponse = totp_setup::start(&state, &user).await?;

    Ok(([(CACHE_CONTROL, "no-store")], Json(setup)).into_response())
}

#[derive(Debug, Deserialize)]
struct ConfirmRequest {
    setup: String,
    code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmResponse {
    /// 开启了多因素认证时的恢复码，只显示这一次；已经开着时原来的恢复码照用。
    recovery_codes: Option<Vec<String>>,
}

async fn confirm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
    Json(request): Json<ConfirmRequest>,
) -> Result<Response> {
    let session = browser_session(session)?;
    let user = state.iam.service.users.get(&ctx.user_id).await?;

    let recovery_codes =
        totp_setup::confirm(&state, &user, &ctx.org_id, &request.setup, &request.code).await?;

    let cookies = reconfirmed(&state, &headers, &session, true)?;

    Ok((
        cookies,
        [(CACHE_CONTROL, "no-store")],
        Json(ConfirmResponse { recovery_codes }),
    )
        .into_response())
}

async fn disable(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
    Json(proof): Json<FactorProof>,
) -> Result<Response> {
    let session = browser_session(session)?;
    let user = state.iam.service.users.get(&ctx.user_id).await?;
    let not_on = || Error::invalid("the authenticator app is not turned on for this account");

    if state.iam.mfa_factors.totp(&user.id).await?.is_none() {
        return Err(not_on());
    }

    if state.iam.passkeys.list(&user.id).await?.is_empty() {
        requirements::ensure_may_remove_last_factor(&state, &user.id).await?;
    }

    if let Checked::Failed = factor::check(&state, &user, &ctx.org_id, &proof).await? {
        return Err(Error::invalid(factor::rejection(&proof)));
    }

    let still_on = state
        .iam
        .mfa_factors
        .remove_totp(&user.id, sessions_end_now())
        .await?
        .ok_or_else(not_on)?;

    let cookies = reconfirmed(&state, &headers, &session, true)?;

    notice::audit(
        &state,
        &ctx.org_id,
        &user.id,
        "mfa.totp.disable",
        &user.id,
        serde_json::json!({}),
    )
    .await;

    if !still_on {
        notice::tell_owner(&state, &user, &ctx.org_id, Notice::Disabled).await;
    }

    Ok((StatusCode::NO_CONTENT, cookies, ()).into_response())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryCodes {
    recovery_codes: Vec<String>,
}

async fn regenerate(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
    Json(proof): Json<FactorProof>,
) -> Result<Response> {
    browser_session(session)?;
    let user = turned_on(&state, &ctx.user_id).await?;

    if let Checked::Failed = factor::check(&state, &user, &ctx.org_id, &proof).await? {
        return Err(Error::invalid(factor::rejection(&proof)));
    }

    let codes = recovery::generate();

    state
        .iam
        .mfa_factors
        .replace_recovery_codes(
            &user.id,
            &recovery::digests(&user.id, &codes),
            TimestampMicros::now(),
        )
        .await?;

    notice::audit(
        &state,
        &ctx.org_id,
        &user.id,
        "mfa.recovery_codes.regenerate",
        &user.id,
        serde_json::json!({}),
    )
    .await;

    Ok((
        [(CACHE_CONTROL, "no-store")],
        Json(RecoveryCodes {
            recovery_codes: codes,
        }),
    )
        .into_response())
}

async fn revoke_others(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(ctx): Extension<IamContext>,
    session: Option<Extension<SignInSession>>,
) -> Result<Response> {
    let session = browser_session(session)?;

    state
        .iam
        .mfa_factors
        .end_sessions(&ctx.user_id, sessions_end_now())
        .await?;

    let cookies = reconfirmed(&state, &headers, &session, false)?;

    notice::audit(
        &state,
        &ctx.org_id,
        &ctx.user_id,
        "sessions.revoke_others",
        &ctx.user_id,
        serde_json::json!({}),
    )
    .await;

    Ok((StatusCode::NO_CONTENT, cookies, ()).into_response())
}

/// 开着多因素认证的账号。
async fn turned_on(state: &AppState, user_id: &Id) -> Result<User> {
    if !enabled(state, user_id).await? {
        return Err(Error::invalid(
            "multi-factor authentication is not turned on for this account",
        ));
    }

    state.iam.service.users.get(user_id).await
}

/// 别的会话作废后，当前浏览器的会话带着新的确认时间留下来。
pub(super) fn reconfirmed(
    state: &AppState,
    headers: &HeaderMap,
    session: &SignInSession,
    verified: bool,
) -> Result<session_cookie::SessionCookies> {
    session_cookie::issue(
        state,
        headers,
        &state.iam.service.reconfirm_session(session, verified),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setup_can_carry_a_proof() {
        let request: SetupRequest = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(request.proof.is_none());

        let replacing: SetupRequest = serde_json::from_value(serde_json::json!({
            "proof": { "method": "recovery_code", "code": "abcde-fghij" }
        }))
        .unwrap();
        assert!(matches!(
            replacing.proof,
            Some(FactorProof::RecoveryCode { .. })
        ));

        let first: SetupRequest = serde_json::from_value(serde_json::json!({
            "proof": { "method": "password", "password": "hunter2" }
        }))
        .unwrap();
        assert!(matches!(first.proof, Some(FactorProof::Password { .. })));
    }
}
