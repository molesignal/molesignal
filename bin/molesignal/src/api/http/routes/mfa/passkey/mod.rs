// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Passkey 的接口。
//!
//! - 账号页（`account`）：`/me/passkeys` 下添加、改名、删除；`POST /me/mfa/passkey-proof` 领一次
//!   确认用的 Passkey 验证。
//! - 登录（`signin`）：`POST /auth/signin/verify/passkey` 是密码之后的第二步，
//!   `/auth/signin/passkey` 是免密码登录。
//!
//! 每次 Passkey 操作分两步：服务端存下 webauthn-rs 的中间状态（MFA 挑战，不发给浏览器，
//! 任何节点都能接着验），把参数和 `ceremony` 交给浏览器；浏览器完成后带着 `ceremony` 和验证器的
//! 应答回来。一个 `ceremony` 只能用一次。规则在 [`crate::app::iam::mfa::passkey`]。

use axum::Router;
use serde::Serialize;
use webauthn_rs::prelude::{
    AuthenticationResult, CreationChallengeResponse, Passkey, PasskeyRegistration,
    RegisterPublicKeyCredential, RequestChallengeResponse, Webauthn,
};

use super::{
    enabled,
    notice::{self, Notice},
};
use crate::{
    api::AppState,
    app::iam::{
        mfa::{
            PASSKEY_CHALLENGE_TTL_MICROS, challenge_code, challenge_digest,
            passkey::{self, Unavailable},
            recovery,
        },
        sessions_end_now,
    },
    domain::iam::User,
    infra::mfa::{MfaChallenge, MfaChallengePurpose, StoredPasskey, TurnOn},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod account;
mod signin;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .merge(account::routes())
        .merge(signin::routes())
}

/// Passkey 名称最长多少个字符（`user_passkeys.name`）。
const MAX_NAME_CHARS: usize = 64;

/// 交给浏览器的一次 Passkey 操作：完成后带回来的 `ceremony`，和给 WebAuthn 接口的参数。
#[derive(Debug, Serialize)]
pub(super) struct PasskeyCeremony<T> {
    ceremony: String,
    options: T,
}

/// 账号页显示的一个 Passkey。没起名字时 `name` 为空，页面按 `aaguid` 显示它存在哪里。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PasskeyView {
    id: String,
    name: String,
    aaguid: Option<String>,
    created_at: i64,
    last_used_at: Option<i64>,
}

impl From<&StoredPasskey> for PasskeyView {
    fn from(stored: &StoredPasskey) -> Self {
        Self {
            id: stored.id.0.clone(),
            name: stored.name.clone(),
            aaguid: stored.aaguid.clone(),
            created_at: stored.created_at.0,
            last_used_at: stored.last_used_at.map(|at| at.0),
        }
    }
}

/// 这个实例的依赖方；不可用时说明原因。
pub(super) fn relying_party(state: &AppState) -> Result<Webauthn> {
    passkey::relying_party(&state.platform.external_url).map_err(|reason| {
        Error::invalid(match reason {
            Unavailable::NotConfigured => "passkeys need http.external_url to be configured",
            Unavailable::IpAddress => "passkeys need http.external_url to be a domain name",
            Unavailable::NotHttps => "passkeys need http.external_url to use HTTPS",
        })
    })
}

/// 存下一次 Passkey 操作的中间状态，返回交给浏览器的 `ceremony`。
async fn begin_ceremony(
    state: &AppState,
    user_id: Option<Id>,
    purpose: MfaChallengePurpose,
    webauthn_state: serde_json::Value,
) -> Result<String> {
    let code = challenge_code();
    let now = TimestampMicros::now();

    let created = state
        .iam
        .mfa_challenges
        .create(MfaChallenge {
            id: Id::new(),
            user_id,
            code_hash: challenge_digest(&code),
            purpose,
            action: None,
            remember: false,
            secret: None,
            webauthn_state: Some(webauthn_state),
            attempts: 0,
            created_at: now,
            expires_at: TimestampMicros(now.0 + PASSKEY_CHALLENGE_TTL_MICROS),
        })
        .await?;

    if !created {
        return Err(Error::resource_exhausted(
            "too many passkey requests are open; try again later",
        ));
    }

    Ok(code)
}

/// 取回一次 Passkey 操作，取到就作废。过期、用途不对、不是这个账号的都一样回答过期。
pub(super) async fn take_ceremony(
    state: &AppState,
    ceremony: &str,
    purpose: MfaChallengePurpose,
    user_id: Option<&Id>,
) -> Result<MfaChallenge> {
    let expired = || Error::invalid("this passkey request has expired; try again");

    let pending = state
        .iam
        .mfa_challenges
        .find_by_code(&challenge_digest(ceremony), TimestampMicros::now())
        .await?
        .filter(|pending| pending.purpose == purpose && pending.user_id.as_ref() == user_id)
        .ok_or_else(expired)?;

    if !state.iam.mfa_challenges.take(&pending.id).await? {
        return Err(expired());
    }

    Ok(pending)
}

/// 领一次确认用的 Passkey 验证：只接受这个账号的凭据。
async fn start_proof(
    state: &AppState,
    user_id: &Id,
) -> Result<PasskeyCeremony<RequestChallengeResponse>> {
    let webauthn = relying_party(state)?;

    let passkeys: Vec<Passkey> = state
        .iam
        .passkeys
        .list(user_id)
        .await?
        .into_iter()
        .map(|stored| stored.passkey)
        .collect();

    if passkeys.is_empty() {
        return Err(Error::invalid("this account has no passkeys"));
    }

    let (options, authentication) = webauthn
        .start_passkey_authentication(&passkeys)
        .map_err(|error| Error::internal(format!("passkey request options: {error}")))?;

    let ceremony = begin_ceremony(
        state,
        Some(user_id.clone()),
        MfaChallengePurpose::PasskeyProof,
        passkey::stored_state(&authentication)?,
    )
    .await?;

    Ok(PasskeyCeremony { ceremony, options })
}

/// 去掉首尾空白；可以空着，页面按验证器型号显示。
fn checked_name(name: &str) -> Result<String> {
    let name = name.trim();

    if name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::invalid(format!(
            "a passkey name has at most {MAX_NAME_CHARS} characters"
        )));
    }

    Ok(name.to_owned())
}

/// 添加 Passkey 的第一步，账号页和登录时现场开启共用：账号已有的凭据不再重复添加。
pub(super) async fn start_registration(
    state: &AppState,
    user: &User,
) -> Result<PasskeyCeremony<CreationChallengeResponse>> {
    let webauthn = relying_party(state)?;

    let existing = state
        .iam
        .passkeys
        .list(&user.id)
        .await?
        .iter()
        .map(|stored| stored.passkey.cred_id().clone())
        .collect();

    let (options, registration) = passkey::start_registration(&webauthn, user, existing)?;

    let ceremony = begin_ceremony(
        state,
        Some(user.id.clone()),
        MfaChallengePurpose::PasskeyRegister,
        passkey::stored_state(&registration)?,
    )
    .await?;

    Ok(PasskeyCeremony { ceremony, options })
}

/// 存下来的 Passkey。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Added {
    passkey: PasskeyView,
    /// 这个 Passkey 开启了多因素认证时的恢复码，只显示这一次。
    pub recovery_codes: Option<Vec<String>>,
}

/// 存下浏览器创建的 Passkey。账号的第一个第二因素同时开启多因素认证：给出一组恢复码，别的会话
/// 作废。`org_id` 是这次操作所在的组织，用于审计和邮件语言。
pub(super) async fn finish_registration(
    state: &AppState,
    user: &User,
    org_id: &Id,
    ceremony: &str,
    credential: &RegisterPublicKeyCredential,
    name: &str,
) -> Result<Added> {
    let webauthn = relying_party(state)?;
    let name = checked_name(name)?;

    let pending = take_ceremony(
        state,
        ceremony,
        MfaChallengePurpose::PasskeyRegister,
        Some(&user.id),
    )
    .await?;

    let registration: PasskeyRegistration = passkey::read_state(pending.webauthn_state)?;

    let key = webauthn
        .finish_passkey_registration(credential, &registration)
        .map_err(|error| Error::invalid(format!("the passkey could not be added: {error}")))?;

    let turning_on = !enabled(state, &user.id).await?;
    let codes = turning_on.then(recovery::generate);
    let hashes = recovery::digests(&user.id, codes.as_deref().unwrap_or_default());

    let stored = StoredPasskey {
        id: Id::new(),
        user_id: user.id.clone(),
        passkey: key,
        name,
        aaguid: passkey::aaguid(credential),
        created_at: TimestampMicros::now(),
        last_used_at: None,
    };

    let turn_on = turning_on.then(|| TurnOn {
        recovery_code_hashes: &hashes,
        sessions_end: sessions_end_now(),
    });

    if !state.iam.passkeys.add(&stored, turn_on).await? {
        return Err(Error::conflict("this passkey is already added"));
    }

    notice::audit(
        state,
        org_id,
        &user.id,
        "mfa.passkey.add",
        &user.id,
        serde_json::json!({ "passkeyId": stored.id.0, "aaguid": stored.aaguid }),
    )
    .await;

    notice::tell_owner(state, user, org_id, Notice::PasskeyAdded).await;

    Ok(Added {
        passkey: PasskeyView::from(&stored),
        recovery_codes: codes,
    })
}

/// 用过一次：记下更新过的签名计数、备份状态和使用时间。
pub(super) async fn record_use(
    state: &AppState,
    mut stored: StoredPasskey,
    result: &AuthenticationResult,
    now: TimestampMicros,
) -> Result<()> {
    stored.passkey.update_credential(result);

    state
        .iam
        .passkeys
        .record_use(&stored.id, &stored.passkey, now)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_trimmed_and_limited_in_characters() {
        assert_eq!(checked_name("  MacBook  ").unwrap(), "MacBook");
        assert_eq!(checked_name("").unwrap(), "");
        assert!(checked_name(&"钥".repeat(MAX_NAME_CHARS)).is_ok());
        assert!(checked_name(&"钥".repeat(MAX_NAME_CHARS + 1)).is_err());
    }
}
