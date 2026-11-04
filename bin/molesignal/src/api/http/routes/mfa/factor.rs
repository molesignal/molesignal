// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 核对一次第二因素：身份验证器的验证码、一个 Passkey 或一个恢复码；加一种第二因素之前确认是
//! 本人。
//!
//! 登录的第二步和账号页上的改动（关闭身份验证器、换一组恢复码、删除 Passkey）都走这里。验证码
//! 和恢复码共用账号的输错锁定：拿到会话的人不能在账号页上无限次试码。Passkey 猜不出来，不计入
//! 输错，也不受锁定限制，账号被别人试锁了，本人仍能用 Passkey 证明身份。账号密码的确认和密码登录
//! 共用输错次数：拿到会话的人不能在这里不限次地猜密码。

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::{PasskeyAuthentication, PublicKeyCredential};

use super::{
    enabled,
    notice::{self, Notice},
    passkey,
};
use crate::{
    api::AppState,
    app::iam::{
        SignInSession, check_password,
        mfa::{self, LOCKOUT, RECENT_SIGN_IN_MICROS, recovery, totp},
        signin_limit::{SigninAccount, SigninError},
    },
    domain::iam::User,
    infra::mfa::{Failure, MfaChallengePurpose},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 请求里带的第二因素，`method` 说明是哪一种。
#[derive(Debug, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub(crate) enum FactorProof {
    /// 身份验证器 App 里的 6 位验证码。
    Totp { code: String },
    /// 一个恢复码。
    RecoveryCode { code: String },
    /// 一个 Passkey：先领到的那次验证（[`passkey::start_proof`]）和浏览器给的凭据。
    Passkey {
        ceremony: String,
        credential: Box<PublicKeyCredential>,
    },
    /// 账号密码：还没开多因素认证时，确认加第一种第二因素的是本人。
    Password { password: String },
}

/// 核对的结果。
pub(crate) enum Checked {
    /// 对上了；用的是恢复码时带着还剩几个。
    Passed { recovery_codes_left: Option<i64> },
    /// 没对上。验证码和恢复码计入了账号的输错次数。
    Failed,
}

/// 没对上时告诉本人的话。
pub(crate) fn rejection(proof: &FactorProof) -> &'static str {
    match proof {
        FactorProof::Totp { .. } | FactorProof::RecoveryCode { .. } => {
            "the verification code is not correct"
        }
        FactorProof::Passkey { .. } => "the passkey could not be verified",
        FactorProof::Password { .. } => "the password is not correct",
    }
}

/// 核对 `proof`。`org_id` 是这次操作所在的组织，用于审计和邮件语言。
pub(crate) async fn check(
    state: &AppState,
    user: &User,
    org_id: &Id,
    proof: &FactorProof,
) -> Result<Checked> {
    let now = TimestampMicros::now();

    match proof {
        FactorProof::Totp { code } => {
            refuse_while_locked(state, &user.id, now).await?;
            let passed = totp_matches(state, &user.id, code, now).await?;

            settle_code(state, user, org_id, now, passed.then_some(None)).await
        }
        FactorProof::RecoveryCode { code } => {
            refuse_while_locked(state, &user.id, now).await?;
            let left = recovery_code_used(state, &user.id, code).await?;

            settle_code(state, user, org_id, now, left.map(Some)).await
        }
        FactorProof::Passkey {
            ceremony,
            credential,
        } => {
            let passed = passkey_matches(state, &user.id, ceremony, credential, now).await?;

            Ok(if passed {
                Checked::Passed {
                    recovery_codes_left: None,
                }
            } else {
                Checked::Failed
            })
        }
        FactorProof::Password { .. } => Err(Error::invalid(
            "verify with the authenticator app, a passkey or a recovery code",
        )),
    }
}

async fn refuse_while_locked(state: &AppState, user_id: &Id, now: TimestampMicros) -> Result<()> {
    match state.iam.mfa_lockouts.locked_until(user_id, now).await? {
        Some(until) => Err(locked(until, now)),
        None => Ok(()),
    }
}

/// 验证码或恢复码核对完：对上了（用的是恢复码时带着还剩几个）清掉输错次数，恢复码告诉本人；
/// 没对上计入输错次数，刚好锁上时告诉本人。
async fn settle_code(
    state: &AppState,
    user: &User,
    org_id: &Id,
    now: TimestampMicros,
    passed: Option<Option<i64>>,
) -> Result<Checked> {
    let Some(recovery_codes_left) = passed else {
        return failed(state, user, org_id, now).await;
    };

    state.iam.mfa_lockouts.clear(&user.id).await?;

    if let Some(left) = recovery_codes_left {
        notice::audit(
            state,
            org_id,
            &user.id,
            "mfa.recovery_code.use",
            &user.id,
            serde_json::json!({ "left": left }),
        )
        .await;

        notice::tell_owner(state, user, org_id, Notice::RecoveryCodeUsed { left }).await;
    }

    Ok(Checked::Passed {
        recovery_codes_left,
    })
}

/// 加一种第二因素（开启身份验证器、添加 Passkey）之前要过的一关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AddingRequires {
    /// 已经开着多因素认证：用已有的第二因素确认。
    Factor,
    /// 还没开：输一次账号密码。
    Password,
    /// 没有密码，刚登录不久：不用再确认。
    Nothing,
    /// 没有密码，登录已久：先重新登录。
    SignInAgain,
}

/// 这个账号这次登录加第二因素要过哪一关。拿到会话的人不能不经本人确认就给账号加上自己的第二
/// 因素，再把本人锁在外面。
pub(super) fn adding_requires(
    user: &User,
    mfa_on: bool,
    session: Option<&SignInSession>,
    now: TimestampMicros,
) -> AddingRequires {
    if mfa_on {
        AddingRequires::Factor
    } else if !user.password_hash.is_empty() {
        AddingRequires::Password
    } else if session.is_some_and(|session| now.0 - session.verified_at.0 <= RECENT_SIGN_IN_MICROS)
    {
        AddingRequires::Nothing
    } else {
        AddingRequires::SignInAgain
    }
}

/// 确认加第二因素的是本人：按 [`adding_requires`] 核对 `proof`。
pub(super) async fn check_adding(
    state: &AppState,
    user: &User,
    org_id: &Id,
    session: &SignInSession,
    proof: Option<&FactorProof>,
    source: Option<IpAddr>,
) -> Result<(), SigninError> {
    let mfa_on = enabled(state, &user.id).await?;

    match adding_requires(user, mfa_on, Some(session), TimestampMicros::now()) {
        AddingRequires::Factor => {
            let proof = proof
                .filter(|proof| !matches!(proof, FactorProof::Password { .. }))
                .ok_or_else(|| {
                    Error::invalid(
                        "verify with the authenticator app, a passkey or a recovery code first",
                    )
                })?;

            match check(state, user, org_id, proof).await? {
                Checked::Passed { .. } => Ok(()),
                Checked::Failed => Err(Error::invalid(rejection(proof)).into()),
            }
        }
        AddingRequires::Password => {
            let Some(FactorProof::Password { password }) = proof else {
                return Err(Error::invalid("enter the account password first").into());
            };

            let attempt = state
                .iam
                .signin_limiter
                .begin(&SigninAccount::Email(&user.email), source)
                .await?;

            let outcome = check_password(password, &user.password_hash).await;

            attempt.settle(&outcome).await;

            outcome.map_err(|error| match error {
                Error::Unauthorized(_) => Error::invalid("the password is not correct").into(),
                other => other.into(),
            })
        }
        AddingRequires::Nothing => Ok(()),
        AddingRequires::SignInAgain => {
            Err(Error::invalid("sign in again, then add it within 10 minutes of signing in").into())
        }
    }
}

/// 用 Passkey 确认：取回领到的那次验证（只能用一次），交给 webauthn-rs 校验；对上了记下签名计数
/// 和使用时间。
async fn passkey_matches(
    state: &AppState,
    user_id: &Id,
    ceremony: &str,
    credential: &PublicKeyCredential,
    now: TimestampMicros,
) -> Result<bool> {
    let webauthn = passkey::relying_party(state)?;

    let pending = passkey::take_ceremony(
        state,
        ceremony,
        MfaChallengePurpose::PasskeyProof,
        Some(user_id),
    )
    .await?;

    let authentication: PasskeyAuthentication = mfa::passkey::read_state(pending.webauthn_state)?;

    let Ok(result) = webauthn.finish_passkey_authentication(credential, &authentication) else {
        return Ok(false);
    };

    let stored = state
        .iam
        .passkeys
        .by_credential(result.cred_id().as_slice())
        .await?
        .filter(|stored| &stored.user_id == user_id);

    if let Some(stored) = stored {
        passkey::record_use(state, stored, &result, now).await?;
    }

    Ok(true)
}

async fn totp_matches(
    state: &AppState,
    user_id: &Id,
    code: &str,
    now: TimestampMicros,
) -> Result<bool> {
    let Some(factor) = state.iam.mfa_factors.totp(user_id).await? else {
        return Ok(false);
    };

    let Some(step) = totp::matching_step(
        &factor.secret,
        code,
        now.0.div_euclid(1_000_000),
        factor.last_used_step,
    ) else {
        return Ok(false);
    };

    state.iam.mfa_factors.use_totp_step(user_id, step).await
}

async fn recovery_code_used(state: &AppState, user_id: &Id, code: &str) -> Result<Option<i64>> {
    let code = recovery::normalize(code);

    if code.is_empty() {
        return Ok(None);
    }

    state
        .iam
        .mfa_factors
        .use_recovery_code(user_id, &recovery::digest(user_id, &code))
        .await
}

async fn failed(
    state: &AppState,
    user: &User,
    org_id: &Id,
    now: TimestampMicros,
) -> Result<Checked> {
    match state
        .iam
        .mfa_lockouts
        .record_failure(&user.id, now, &LOCKOUT)
        .await?
    {
        Failure::Counted => Ok(Checked::Failed),
        Failure::Locked { until } => {
            let minutes = minutes_until(until, now);

            notice::audit(
                state,
                org_id,
                &user.id,
                "signin.mfa.lockout",
                &user.id,
                serde_json::json!({ "minutes": minutes }),
            )
            .await;

            notice::tell_owner(state, user, org_id, Notice::Locked { minutes }).await;
            Err(locked(until, now))
        }
    }
}

fn minutes_until(until: TimestampMicros, now: TimestampMicros) -> i64 {
    let remaining = (until.0 - now.0).max(0);

    ((remaining + 59_999_999) / 60_000_000).max(1)
}

/// 账号锁着时的回答。
pub(super) fn locked(until: TimestampMicros, now: TimestampMicros) -> Error {
    Error::resource_exhausted(format!(
        "too many wrong verification codes; try again in {} minutes",
        minutes_until(until, now)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::iam::SignInMethod,
        domain::iam::{IamScope, UserStatus},
    };

    fn user(password_hash: &str) -> User {
        User {
            id: Id::from_string("user-1"),
            email: "alice@example.com".into(),
            display_name: "Alice".into(),
            avatar_url: None,
            bio: String::new(),
            password_hash: password_hash.into(),
            disabled: false,
            status: UserStatus::Active,
            created_at: TimestampMicros(0),
            sessions_valid_after: TimestampMicros(0),
        }
    }

    fn session(verified_at: i64) -> SignInSession {
        SignInSession {
            user_id: Id::from_string("user-1"),
            org_id: Id::from_string("org-1"),
            scope: IamScope::Organization,
            expires_at: 0,
            remember: false,
            method: SignInMethod::single_factor(),
            auth_time: TimestampMicros(i64::MAX),
            verified_at: TimestampMicros(verified_at),
        }
    }

    #[test]
    fn a_lock_is_told_in_whole_minutes_rounded_up() {
        let now = TimestampMicros(0);

        assert_eq!(minutes_until(TimestampMicros(60_000_000), now), 1);
        assert_eq!(minutes_until(TimestampMicros(60_000_001), now), 2);
        assert_eq!(minutes_until(TimestampMicros(3_600_000_000), now), 60);
        assert_eq!(minutes_until(TimestampMicros(1), now), 1);
    }

    #[test]
    fn a_proof_names_its_method_in_snake_case() {
        let proof: FactorProof =
            serde_json::from_value(serde_json::json!({"method": "recovery_code", "code": "x"}))
                .unwrap();
        assert!(matches!(proof, FactorProof::RecoveryCode { code } if code == "x"));

        let proof: FactorProof = serde_json::from_value(serde_json::json!({
            "method": "passkey",
            "ceremony": "abc",
            "credential": {
                "id": "AQID",
                "rawId": "AQID",
                "type": "public-key",
                "response": {
                    "authenticatorData": "AQID",
                    "clientDataJSON": "AQID",
                    "signature": "AQID",
                    "userHandle": null,
                },
                "clientExtensionResults": {},
            },
        }))
        .unwrap();
        assert!(matches!(proof, FactorProof::Passkey { ceremony, .. } if ceremony == "abc"));
    }

    #[test]
    fn adding_a_factor_needs_the_owner() {
        let now = TimestampMicros(RECENT_SIGN_IN_MICROS * 3);
        let recent = session(now.0 - RECENT_SIGN_IN_MICROS);
        let earlier = session(now.0 - RECENT_SIGN_IN_MICROS - 1);

        assert_eq!(
            adding_requires(&user("$argon2id$x"), true, Some(&recent), now),
            AddingRequires::Factor
        );
        assert_eq!(
            adding_requires(&user("$argon2id$x"), false, Some(&recent), now),
            AddingRequires::Password
        );
        assert_eq!(
            adding_requires(&user(""), false, Some(&recent), now),
            AddingRequires::Nothing
        );
        assert_eq!(
            adding_requires(&user(""), false, Some(&earlier), now),
            AddingRequires::SignInAgain
        );
        assert_eq!(
            adding_requires(&user(""), false, None, now),
            AddingRequires::SignInAgain
        );
    }
}
