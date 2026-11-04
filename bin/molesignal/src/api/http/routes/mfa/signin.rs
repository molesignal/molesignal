// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 登录的第二步。
//!
//! 第一关（密码、LDAP、单点登录关联的确认）通过后，账号开了多因素认证就先不写 Cookie：存下
//! 待验证的登录，回答 `{"mfa": {"challenge", "methods", "expires_at"}}`。浏览器带着
//! challenge 和验证码（或 Passkey，见 [`super::passkey`]）请求 `POST /auth/signin/verify`，对上后
//! 按存下的 [`AfterVerification`] 签发会话或完成关联，回答 `{"login": …}`；用的是恢复码时一并告诉
//! 还剩几个。
//!
//! 组织的登录要求也在第一关之后判断（[`super::requirements`]）：只能经单点登录进入的组织拒绝；
//! 要求多因素认证、账号还没有第二因素时，转去现场开启（[`super::enroll`]）。

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};

use super::{
    enroll,
    factor::{self, Checked, FactorProof},
    requirements::{admission, sso_only},
};
use crate::{
    api::{
        AppState,
        http::{
            routes::{auth::SessionView, sso},
            session_cookie::SessionCookies,
        },
    },
    app::iam::{
        SignInMethod,
        mfa::{
            MAX_ATTEMPTS, SIGNIN_CHALLENGE_TTL_MICROS, challenge_code, challenge_digest, passkey,
        },
        signin_policy::Admission,
    },
    domain::iam::User,
    infra::{
        mfa::{MfaChallenge, MfaChallengePurpose},
        sso::LinkedIdentity,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/auth/signin/verify", post(verify))
}

/// 第二步验证通过后要做的事。
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum AfterVerification {
    /// 签发这个身份的会话。
    SignIn { session: SessionView },
    /// 把单点登录身份关联到账号，再登录。本人已用邮件验证码（`by_email`）或账号密码确认过。
    Link {
        provider_id: String,
        org_id: String,
        subject: String,
        identity: LinkedIdentity,
        by_email: bool,
    },
}

impl AfterVerification {
    /// 这次登录要进入的组织：审计记在这里，邮件用它的默认语言。
    pub(super) fn org_id(&self) -> Id {
        match self {
            Self::SignIn { session } => Id::from_string(session.org_id.clone()),
            Self::Link { org_id, .. } => Id::from_string(org_id.clone()),
        }
    }

    /// 存在等待验证的请求里的那件事。
    pub(super) fn read(challenge: &MfaChallenge) -> Result<Self> {
        challenge
            .action
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| Error::internal(format!("pending sign-in decode: {error}")))?
            .ok_or_else(|| Error::internal("pending sign-in without an action"))
    }
}

/// 第二因素过了（或者刚开好）之后：签发会话，或者完成关联再签发。
pub(super) async fn after_verification(
    state: &AppState,
    headers: &HeaderMap,
    then: AfterVerification,
    remember: bool,
    user: User,
) -> Result<(SessionCookies, SessionView)> {
    match then {
        AfterVerification::SignIn { session } => {
            let cookies =
                session.sign_in(state, headers, remember, SignInMethod::second_factor())?;

            Ok((cookies, session))
        }
        AfterVerification::Link {
            provider_id,
            subject,
            identity,
            by_email,
            ..
        } => {
            sso::complete_link(
                state,
                headers,
                sso::PendingLink {
                    provider_id,
                    subject,
                    identity,
                    by_email,
                    user,
                },
                remember,
                true,
            )
            .await
        }
    }
}

/// 账号开了多因素认证。
pub(crate) async fn enabled(state: &AppState, user_id: &Id) -> Result<bool> {
    Ok(state
        .iam
        .mfa_factors
        .enabled_among(std::slice::from_ref(user_id))
        .await?
        .contains(user_id))
}

/// 第一关过了之后这次登录会是什么样：账号开了多因素认证，就要过了第二步才进得去。
pub(crate) fn after_first_step(mfa_on: bool) -> SignInMethod {
    if mfa_on {
        SignInMethod::second_factor()
    } else {
        SignInMethod::single_factor()
    }
}

/// 第一关已过的登录：先看 `session` 的组织放不放行；账号开了多因素认证就要第二步，否则直接签发
/// 只过了一关的会话。
pub(crate) async fn finish_first_step(
    state: &AppState,
    headers: &HeaderMap,
    session: SessionView,
    remember: bool,
) -> Result<Response> {
    let user_id = Id::from_string(session.user_id.clone());
    let org_id = Id::from_string(session.org_id.clone());
    let mfa_on = enabled(state, &user_id).await?;

    match admission(state, &org_id, &user_id, &after_first_step(mfa_on)).await? {
        Admission::Allowed => {}
        Admission::NeedsSso => return Err(sso_only(&session.org_name)),
        Admission::NeedsMfa => {
            let organization_name = session.org_name.clone();

            return enroll::begin(
                state,
                &user_id,
                organization_name,
                AfterVerification::SignIn { session },
                remember,
            )
            .await;
        }
    }

    if mfa_on {
        return second_step(
            state,
            &user_id,
            AfterVerification::SignIn { session },
            remember,
        )
        .await;
    }

    let cookies = session.sign_in(state, headers, remember, SignInMethod::single_factor())?;

    Ok((
        cookies,
        [(CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "login": session })),
    )
        .into_response())
}

/// 浏览器要接着做的第二步。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MfaRequired {
    challenge: String,
    methods: Vec<&'static str>,
    expires_at: i64,
}

/// 这个账号的第二步可以用什么。Passkey 要这个实例的依赖方可用；恢复码总能用。
pub(crate) async fn second_step_methods(
    state: &AppState,
    user_id: &Id,
) -> Result<Vec<&'static str>> {
    let mut methods = Vec::new();

    if state.iam.mfa_factors.totp(user_id).await?.is_some() {
        methods.push("totp");
    }

    if passkey::relying_party(&state.platform.external_url).is_ok()
        && !state.iam.passkeys.list(user_id).await?.is_empty()
    {
        methods.push("passkey");
    }

    methods.push("recovery_code");
    Ok(methods)
}

/// 存下待验证的登录，回答 `{"mfa": …}`。
pub(crate) async fn second_step(
    state: &AppState,
    user_id: &Id,
    then: AfterVerification,
    remember: bool,
) -> Result<Response> {
    let now = TimestampMicros::now();

    if let Some(until) = state.iam.mfa_lockouts.locked_until(user_id, now).await? {
        return Err(factor::locked(until, now));
    }

    let action = serde_json::to_value(&then)
        .map_err(|error| Error::internal(format!("pending sign-in encode: {error}")))?;

    let code = challenge_code();
    let expires_at = TimestampMicros(now.0 + SIGNIN_CHALLENGE_TTL_MICROS);

    let created = state
        .iam
        .mfa_challenges
        .create(MfaChallenge {
            id: Id::new(),
            user_id: Some(user_id.clone()),
            code_hash: challenge_digest(&code),
            purpose: MfaChallengePurpose::SignIn,
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
            "too many sign-ins are waiting for a verification code; try again later",
        ));
    }

    let required = MfaRequired {
        challenge: code,
        methods: second_step_methods(state, user_id).await?,
        expires_at: expires_at.0,
    };

    Ok((
        [(CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "mfa": required })),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
struct VerifyRequest {
    challenge: String,
    #[serde(flatten)]
    proof: FactorProof,
}

async fn verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<VerifyRequest>,
) -> Result<Response> {
    let now = TimestampMicros::now();

    let challenge = state
        .iam
        .mfa_challenges
        .find_by_code(&challenge_digest(&request.challenge), now)
        .await?
        .filter(|challenge| challenge.purpose == MfaChallengePurpose::SignIn)
        .ok_or_else(expired)?;

    if challenge.attempts >= MAX_ATTEMPTS {
        state.iam.mfa_challenges.take(&challenge.id).await?;
        return Err(too_many_attempts());
    }

    let then = AfterVerification::read(&challenge)?;

    let user_id = challenge
        .user_id
        .as_ref()
        .ok_or_else(|| Error::internal("pending sign-in without an account"))?;

    let user = state.iam.service.users.get(user_id).await?;
    let org_id = then.org_id();

    let Checked::Passed {
        recovery_codes_left,
    } = factor::check(&state, &user, &org_id, &request.proof).await?
    else {
        let attempts = state
            .iam
            .mfa_challenges
            .record_failure(&challenge.id)
            .await?;

        if attempts >= MAX_ATTEMPTS {
            state.iam.mfa_challenges.take(&challenge.id).await?;
            return Err(too_many_attempts());
        }

        return Err(Error::unauthorized(factor::rejection(&request.proof)));
    };

    if !state.iam.mfa_challenges.take(&challenge.id).await? {
        return Err(expired());
    }

    state.iam.service.ensure_user_access(&user.id).await?;

    let (cookies, login) =
        after_verification(&state, &headers, then, challenge.remember, user).await?;

    Ok((
        cookies,
        [(CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "login": login,
            "recoveryCodesLeft": recovery_codes_left,
        })),
    )
        .into_response())
}

pub(super) fn expired() -> Error {
    Error::unauthorized("this sign-in has expired; sign in again")
}

fn too_many_attempts() -> Error {
    Error::unauthorized("too many wrong verification codes; sign in again")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pending_link_keeps_what_the_identity_provider_said() {
        let then = AfterVerification::Link {
            provider_id: "provider-1".into(),
            org_id: "org-1".into(),
            subject: "subject-1".into(),
            identity: LinkedIdentity {
                email: "alice@example.com".into(),
                display_name: "Alice".into(),
                groups: vec!["ops".into()],
                email_verified: Some(true),
            },
            by_email: true,
        };

        let stored = serde_json::to_value(&then).unwrap();
        assert_eq!(stored["kind"], "link");

        let read: AfterVerification = serde_json::from_value(stored).unwrap();
        assert_eq!(read.org_id(), Id::from_string("org-1"));
        assert!(matches!(
            read,
            AfterVerification::Link { by_email: true, .. }
        ));
    }

    #[test]
    fn a_verification_request_carries_the_challenge_and_the_code() {
        let request: VerifyRequest = serde_json::from_value(serde_json::json!({
            "challenge": "abc",
            "method": "totp",
            "code": "123456",
        }))
        .unwrap();

        assert_eq!(request.challenge, "abc");
        assert!(matches!(request.proof, FactorProof::Totp { code } if code == "123456"));
    }

    #[test]
    fn a_verification_request_can_carry_a_passkey() {
        let request: VerifyRequest = serde_json::from_value(serde_json::json!({
            "challenge": "abc",
            "method": "passkey",
            "ceremony": "def",
            "credential": {
                "id": "AQID",
                "rawId": "AQID",
                "type": "public-key",
                "response": {
                    "authenticatorData": "AQID",
                    "clientDataJSON": "AQID",
                    "signature": "AQID",
                    "userHandle": "BAUG",
                },
                "clientExtensionResults": {},
            },
        }))
        .unwrap();

        assert_eq!(request.challenge, "abc");

        let FactorProof::Passkey {
            ceremony,
            credential,
        } = request.proof
        else {
            panic!("expected a passkey");
        };

        assert_eq!(ceremony, "def");
        assert_eq!(credential.get_credential_id(), [1, 2, 3]);
        assert_eq!(credential.get_user_unique_id(), Some(&[4, 5, 6][..]));
    }
}
