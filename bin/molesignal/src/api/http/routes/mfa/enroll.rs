// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 登录时现场开启第二因素。
//!
//! 组织要求多因素认证、账号还没有第二因素时，第一关通过后不签发会话：存下待开启的登录，回答
//! `{"enroll": {"challenge", "methods", "organization_name", "expires_at"}}`。浏览器带着
//! challenge 设置身份验证器或 Passkey，设置好才拿到会话，同时拿到恢复码（只显示这一次）。
//!
//! 只有账号还没有第二因素时这条路才通：这期间开启了的话，要重新登录，过第二步。
//!
//! - `POST /auth/signin/enroll/totp`、`POST /auth/signin/enroll/totp/confirm`
//! - `POST /auth/signin/enroll/passkey/options`、`POST /auth/signin/enroll/passkey`

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::{CreationChallengeResponse, RegisterPublicKeyCredential};

use super::{
    enabled,
    passkey::{self, PasskeyCeremony},
    signin::{AfterVerification, after_verification, expired},
    totp_setup::{self, SetupResponse},
};
use crate::{
    api::AppState,
    app::iam::mfa::{
        SETUP_CHALLENGE_TTL_MICROS, challenge_code, challenge_digest, passkey as rules,
    },
    domain::iam::User,
    infra::mfa::{MfaChallenge, MfaChallengePurpose},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/signin/enroll/totp", post(start_totp))
        .route("/auth/signin/enroll/totp/confirm", post(confirm_totp))
        .route("/auth/signin/enroll/passkey/options", post(passkey_options))
        .route("/auth/signin/enroll/passkey", post(add_passkey))
}

/// 浏览器要接着做的现场开启。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EnrollRequired {
    challenge: String,
    methods: Vec<&'static str>,
    /// 要求开启的组织。
    organization_name: String,
    expires_at: i64,
}

/// 存下要现场开启才能进去的登录，回答 `{"enroll": …}`。
pub(crate) async fn begin(
    state: &AppState,
    user_id: &Id,
    organization_name: String,
    then: AfterVerification,
    remember: bool,
) -> Result<Response> {
    let action = serde_json::to_value(&then)
        .map_err(|error| Error::internal(format!("pending sign-in encode: {error}")))?;

    let code = challenge_code();
    let now = TimestampMicros::now();
    let expires_at = TimestampMicros(now.0 + SETUP_CHALLENGE_TTL_MICROS);

    let created = state
        .iam
        .mfa_challenges
        .create(MfaChallenge {
            id: Id::new(),
            user_id: Some(user_id.clone()),
            code_hash: challenge_digest(&code),
            purpose: MfaChallengePurpose::Enroll,
            action: Some(action),
            remember,
            secret: None,
            webauthn_state: None,
            attempts: 0,
            created_at: now,
            expires_at,
        })
        .await?;

    if !created {
        return Err(Error::resource_exhausted(
            "too many sign-ins are waiting for a second factor; try again later",
        ));
    }

    let mut methods = Vec::new();

    if rules::relying_party(&state.platform.external_url).is_ok() {
        methods.push("passkey");
    }

    methods.push("totp");

    let required = EnrollRequired {
        challenge: code,
        methods,
        organization_name,
        expires_at: expires_at.0,
    };

    Ok((
        [(CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "enroll": required })),
    )
        .into_response())
}

/// 等着现场开启的登录和它的账号。
async fn pending(state: &AppState, code: &str) -> Result<(MfaChallenge, User)> {
    let challenge = state
        .iam
        .mfa_challenges
        .find_by_code(&challenge_digest(code), TimestampMicros::now())
        .await?
        .filter(|challenge| challenge.purpose == MfaChallengePurpose::Enroll)
        .ok_or_else(expired)?;

    let user_id = challenge
        .user_id
        .as_ref()
        .ok_or_else(|| Error::internal("pending sign-in without an account"))?;

    let user = state.iam.service.users.get(user_id).await?;

    if enabled(state, &user.id).await? {
        state.iam.mfa_challenges.take(&challenge.id).await?;
        return Err(expired());
    }

    Ok((challenge, user))
}

/// 第二因素开好了：用掉这次登录，签发会话。
async fn finish(
    state: &AppState,
    headers: &HeaderMap,
    challenge: MfaChallenge,
    user: User,
    recovery_codes: Option<Vec<String>>,
) -> Result<Response> {
    if !state.iam.mfa_challenges.take(&challenge.id).await? {
        return Err(expired());
    }

    state.iam.service.ensure_user_access(&user.id).await?;

    let then = AfterVerification::read(&challenge)?;
    let (cookies, login) =
        after_verification(state, headers, then, challenge.remember, user).await?;

    Ok((
        cookies,
        [(CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "login": login,
            "recoveryCodes": recovery_codes,
        })),
    )
        .into_response())
}

/// 这次操作所在的组织：审计记在这里，邮件用它的默认语言。
fn org_of(challenge: &MfaChallenge) -> Result<Id> {
    AfterVerification::read(challenge).map(|then| then.org_id())
}

#[derive(Debug, Deserialize)]
struct StartRequest {
    challenge: String,
}

async fn start_totp(
    State(state): State<AppState>,
    Json(request): Json<StartRequest>,
) -> Result<Response> {
    let (_, user) = pending(&state, &request.challenge).await?;
    let setup: SetupResponse = totp_setup::start(&state, &user).await?;

    Ok(([(CACHE_CONTROL, "no-store")], Json(setup)).into_response())
}

#[derive(Debug, Deserialize)]
struct ConfirmTotpRequest {
    challenge: String,
    setup: String,
    code: String,
}

async fn confirm_totp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ConfirmTotpRequest>,
) -> Result<Response> {
    let (challenge, user) = pending(&state, &request.challenge).await?;
    let org_id = org_of(&challenge)?;

    let codes = totp_setup::confirm(&state, &user, &org_id, &request.setup, &request.code).await?;

    finish(&state, &headers, challenge, user, codes).await
}

async fn passkey_options(
    State(state): State<AppState>,
    Json(request): Json<StartRequest>,
) -> Result<Response> {
    let (_, user) = pending(&state, &request.challenge).await?;
    let ceremony: PasskeyCeremony<CreationChallengeResponse> =
        passkey::start_registration(&state, &user).await?;

    Ok(([(CACHE_CONTROL, "no-store")], Json(ceremony)).into_response())
}

#[derive(Debug, Deserialize)]
struct AddPasskeyRequest {
    challenge: String,
    ceremony: String,
    credential: Box<RegisterPublicKeyCredential>,
    #[serde(default)]
    name: String,
}

async fn add_passkey(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AddPasskeyRequest>,
) -> Result<Response> {
    let (challenge, user) = pending(&state, &request.challenge).await?;
    let org_id = org_of(&challenge)?;

    let added = passkey::finish_registration(
        &state,
        &user,
        &org_id,
        &request.ceremony,
        &request.credential,
        &request.name,
    )
    .await?;

    finish(&state, &headers, challenge, user, added.recovery_codes).await
}
