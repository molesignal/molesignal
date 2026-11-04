// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 设置身份验证器的两步，账号页和登录时现场开启共用：先给新的密钥和二维码，第一个验证码对上才
//! 生效。

use serde::Serialize;

use super::{
    enabled,
    notice::{self, Notice},
};
use crate::{
    api::AppState,
    app::iam::{
        mfa::{
            MAX_ATTEMPTS, SETUP_CHALLENGE_TTL_MICROS, challenge_code, challenge_digest, issuer, qr,
            recovery, totp,
        },
        sessions_end_now,
    },
    domain::iam::User,
    infra::mfa::{MfaChallenge, MfaChallengePurpose, NewTotp},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 等第一个验证码的设置。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SetupResponse {
    /// 确认时带回来的凭据。
    setup: String,
    /// 手动输入用的密钥，4 个字符一组。
    secret: String,
    otpauth_url: String,
    qr_svg: String,
    expires_at: i64,
}

pub(super) async fn start(state: &AppState, user: &User) -> Result<SetupResponse> {
    let secret = totp::generate_secret();
    let code = challenge_code();
    let now = TimestampMicros::now();
    let expires_at = TimestampMicros(now.0 + SETUP_CHALLENGE_TTL_MICROS);

    let created = state
        .iam
        .mfa_challenges
        .create(MfaChallenge {
            id: Id::new(),
            user_id: Some(user.id.clone()),
            code_hash: challenge_digest(&code),
            purpose: MfaChallengePurpose::TotpSetup,
            action: None,
            remember: false,
            secret: Some(secret.clone()),
            webauthn_state: None,
            attempts: 0,
            created_at: now,
            expires_at,
        })
        .await?;

    if !created {
        return Err(Error::resource_exhausted(
            "too many authenticator setups are open; try again later",
        ));
    }

    let otpauth_url =
        totp::otpauth_url(&issuer(&state.platform.external_url), &user.email, &secret);

    Ok(SetupResponse {
        setup: code,
        secret: totp::grouped(&totp::base32(&secret)),
        qr_svg: qr::svg(&otpauth_url)?,
        otpauth_url,
        expires_at: expires_at.0,
    })
}

/// 第一个验证码对上之后，账号的身份验证器生效。开启了多因素认证时带着恢复码，只显示这一次；
/// 已经开着时原来的恢复码照用。`org_id` 是这次操作所在的组织，用于审计和邮件语言。
pub(super) async fn confirm(
    state: &AppState,
    user: &User,
    org_id: &Id,
    setup: &str,
    code: &str,
) -> Result<Option<Vec<String>>> {
    let now = TimestampMicros::now();

    let pending = state
        .iam
        .mfa_challenges
        .find_by_code(&challenge_digest(setup), now)
        .await?
        .filter(|pending| {
            pending.purpose == MfaChallengePurpose::TotpSetup
                && pending.user_id.as_ref() == Some(&user.id)
        })
        .ok_or_else(|| Error::invalid("this setup has expired; start again"))?;

    let secret = pending
        .secret
        .clone()
        .ok_or_else(|| Error::internal("authenticator setup without a secret"))?;

    let Some(step) = totp::matching_step(&secret, code, now.0.div_euclid(1_000_000), 0) else {
        let attempts = state.iam.mfa_challenges.record_failure(&pending.id).await?;

        if attempts >= MAX_ATTEMPTS {
            state.iam.mfa_challenges.take(&pending.id).await?;
            return Err(Error::invalid(
                "too many wrong codes; start the setup again",
            ));
        }

        return Err(Error::invalid("the verification code is not correct"));
    };

    if !state.iam.mfa_challenges.take(&pending.id).await? {
        return Err(Error::invalid("this setup has expired; start again"));
    }

    let replacing = state.iam.mfa_factors.totp(&user.id).await?.is_some();
    let turning_on = !enabled(state, &user.id).await?;
    let codes = turning_on.then(recovery::generate);
    let hashes = recovery::digests(&user.id, codes.as_deref().unwrap_or_default());

    state
        .iam
        .mfa_factors
        .save_totp(
            &user.id,
            NewTotp {
                secret: &secret,
                confirmed_step: step,
                recovery_code_hashes: &hashes,
                at: now,
                sessions_end: (replacing || turning_on).then(sessions_end_now),
            },
        )
        .await?;

    let action = if replacing {
        "mfa.totp.replace"
    } else {
        "mfa.totp.enable"
    };

    notice::audit(
        state,
        org_id,
        &user.id,
        action,
        &user.id,
        serde_json::json!({}),
    )
    .await;

    if turning_on {
        notice::tell_owner(state, user, org_id, Notice::Enabled).await;
    }

    Ok(codes)
}
