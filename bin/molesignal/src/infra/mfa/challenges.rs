// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 等第二步验证的请求，存在 `one_time_tokens` 里（`kind = 'mfa_challenge'`，`token_hash` 是 `code` 的摘要，
//! `action`、`remember`、`webauthn_state` 在 `payload` 里）。
//!
//! 已过第一关的登录先存在这里：发起登录的浏览器拿着 `code`，带着验证码或 Passkey 来换会话，
//! 验证通过后按 `action` 签发会话或完成单点登录关联。要现场开启第二因素的登录、正在设置的身份
//! 验证器、正在添加的 Passkey、账号页上用 Passkey 确认的操作和免密码的 Passkey 登录也在这里等
//! 浏览器回来。只存 `code` 的
//! 摘要；正在设置的密钥用根密钥加密；WebAuthn 的中间状态只在这里，不发给浏览器。存在 Postgres
//! 里，任何节点都能接着验证。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, types::Json};

use crate::{
    infra::{cipher::CipherRootKey, persistence::sqlx_err},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 每个账号同一用途同时未过期的请求上限，挡住反复发起的登录。
pub const MAX_PENDING_PER_USER: i64 = 5;

/// 免密码的 Passkey 登录开始时还不知道是谁：这类未过期的请求合计最多留这么多。撑满表的请求只会
/// 让别人等着的登录早点过期，挡不住登录。
pub const MAX_PENDING_ANONYMOUS: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MfaChallengePurpose {
    /// 已过第一关的登录。
    SignIn,
    /// 正在设置的身份验证器。
    TotpSetup,
    /// 正在添加的 Passkey。
    PasskeyRegister,
    /// 账号页上的操作，用一个已有的 Passkey 确认。
    PasskeyProof,
    /// 免密码的 Passkey 登录。
    PasskeySignIn,
    /// 已过第一关的登录，组织要求多因素认证，账号要现场开启才能进去。
    Enroll,
}

impl MfaChallengePurpose {
    /// 到了上限时挤掉最早的，而不是拒绝新的。页面打开对话框、登录页时就先领好 Passkey 的请求，
    /// 大多不会用上，不该挡住后面真要用的。
    fn replaces_oldest(self) -> bool {
        matches!(
            self,
            Self::PasskeyRegister | Self::PasskeyProof | Self::PasskeySignIn
        )
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::SignIn => "signin",
            Self::TotpSetup => "totp_setup",
            Self::PasskeyRegister => "passkey_register",
            Self::PasskeyProof => "passkey_proof",
            Self::PasskeySignIn => "passkey_signin",
            Self::Enroll => "mfa_enroll",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "signin" => Ok(Self::SignIn),
            "totp_setup" => Ok(Self::TotpSetup),
            "passkey_register" => Ok(Self::PasskeyRegister),
            "passkey_proof" => Ok(Self::PasskeyProof),
            "passkey_signin" => Ok(Self::PasskeySignIn),
            "mfa_enroll" => Ok(Self::Enroll),
            other => Err(Error::internal(format!(
                "unknown multi-factor challenge purpose `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MfaChallenge {
    pub id: Id,
    /// 免密码的 Passkey 登录开始时为空。
    pub user_id: Option<Id>,
    pub code_hash: String,
    pub purpose: MfaChallengePurpose,
    /// 验证通过后要做的事，由调用方定义形状。
    pub action: Option<serde_json::Value>,
    pub remember: bool,
    /// 正在设置的身份验证器密钥（解开后的）。
    pub secret: Option<Vec<u8>>,
    /// webauthn-rs 的中间状态。
    pub webauthn_state: Option<serde_json::Value>,
    pub attempts: i32,
    pub created_at: TimestampMicros,
    pub expires_at: TimestampMicros,
}

#[async_trait]
pub trait MfaChallengeRepository: Send + Sync {
    /// 存下新请求并清掉过期的。未过期的同类请求已到上限时：Passkey 的请求挤掉最早的，其余返回
    /// `false`。
    async fn create(&self, challenge: MfaChallenge) -> Result<bool>;
    /// 未过期的请求。
    async fn find_by_code(
        &self,
        code_hash: &str,
        now: TimestampMicros,
    ) -> Result<Option<MfaChallenge>>;
    /// 记一次输错，返回累计次数。
    async fn record_failure(&self, id: &Id) -> Result<i32>;
    /// 用掉请求：删掉了才算拿到，同一个请求只能用一次。
    async fn take(&self, id: &Id) -> Result<bool>;
}

pub struct PgMfaChallengeRepository {
    pool: PgPool,
    cipher: CipherRootKey,
}

/// `payload` 里存的部分。
#[derive(Serialize, Deserialize)]
struct StoredPayload {
    #[serde(default)]
    action: Option<serde_json::Value>,
    #[serde(default)]
    remember: bool,
    #[serde(default)]
    webauthn_state: Option<serde_json::Value>,
}

const CHALLENGE_COLS: &str = "id, user_id, token_hash, purpose, payload, secret_nonce, \
    secret_ciphertext, attempts, created_at, expires_at";

impl PgMfaChallengeRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }

    fn row_to_challenge(&self, row: sqlx::postgres::PgRow) -> Result<MfaChallenge> {
        let Json(payload): Json<StoredPayload> = row.try_get("payload").map_err(sqlx_err)?;
        let nonce: Option<Vec<u8>> = row.try_get("secret_nonce").map_err(sqlx_err)?;
        let ciphertext: Option<Vec<u8>> = row.try_get("secret_ciphertext").map_err(sqlx_err)?;

        let secret = match (nonce, ciphertext) {
            (Some(nonce), Some(ciphertext)) => {
                Some(self.cipher.open(&nonce, &ciphertext).map_err(|error| {
                    Error::internal(format!(
                        "pending authenticator secret cannot be opened: {error}"
                    ))
                })?)
            }
            _ => None,
        };

        Ok(MfaChallenge {
            id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
            user_id: row
                .try_get::<Option<String>, _>("user_id")
                .map_err(sqlx_err)?
                .map(Id::from_string),
            code_hash: row.try_get("token_hash").map_err(sqlx_err)?,
            purpose: MfaChallengePurpose::parse(
                &row.try_get::<String, _>("purpose").map_err(sqlx_err)?,
            )?,
            action: payload.action,
            remember: payload.remember,
            secret,
            webauthn_state: payload.webauthn_state,
            attempts: row.try_get("attempts").map_err(sqlx_err)?,
            created_at: row
                .try_get::<TimestampMicros, _>("created_at")
                .map_err(sqlx_err)?,
            expires_at: row
                .try_get::<TimestampMicros, _>("expires_at")
                .map_err(sqlx_err)?,
        })
    }
}

#[async_trait]
impl MfaChallengeRepository for PgMfaChallengeRepository {
    async fn create(&self, challenge: MfaChallenge) -> Result<bool> {
        let sealed = challenge
            .secret
            .as_deref()
            .map(|secret| self.cipher.seal(secret))
            .transpose()
            .map_err(|error| {
                Error::internal(format!(
                    "pending authenticator secret cannot be sealed: {error}"
                ))
            })?;

        let (nonce, ciphertext) = sealed.unzip();
        let user_id = challenge.user_id.as_ref().map(|user_id| user_id.0.clone());

        let payload = StoredPayload {
            action: challenge.action,
            remember: challenge.remember,
            webauthn_state: challenge.webauthn_state,
        };

        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query(
            "DELETE FROM one_time_tokens WHERE kind = 'mfa_challenge' AND expires_at <= $1",
        )
        .bind(challenge.created_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let pending: i64 = sqlx::query(
            "SELECT COUNT(*)::BIGINT AS n FROM one_time_tokens
             WHERE kind = 'mfa_challenge' AND user_id IS NOT DISTINCT FROM $1 AND purpose = $2",
        )
        .bind(&user_id)
        .bind(challenge.purpose.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .try_get("n")
        .map_err(sqlx_err)?;

        let limit = if user_id.is_some() {
            MAX_PENDING_PER_USER
        } else {
            MAX_PENDING_ANONYMOUS
        };

        if pending >= limit && !challenge.purpose.replaces_oldest() {
            tx.rollback().await.map_err(sqlx_err)?;
            return Ok(false);
        }

        if pending >= limit {
            sqlx::query(
                "DELETE FROM one_time_tokens WHERE token_hash IN (
                    SELECT token_hash FROM one_time_tokens
                    WHERE kind = 'mfa_challenge'
                      AND user_id IS NOT DISTINCT FROM $1 AND purpose = $2
                    ORDER BY expires_at LIMIT $3)",
            )
            .bind(&user_id)
            .bind(challenge.purpose.as_str())
            .bind(pending - limit + 1)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }

        sqlx::query(
            "INSERT INTO one_time_tokens
                (token_hash, kind, id, user_id, purpose, payload, secret_nonce,
                 secret_ciphertext, attempts, created_at, expires_at)
             VALUES ($1, 'mfa_challenge', $2, $3, $4, $5, $6, $7, 0, $8, $9)",
        )
        .bind(&challenge.code_hash)
        .bind(&challenge.id.0)
        .bind(&user_id)
        .bind(challenge.purpose.as_str())
        .bind(Json(payload))
        .bind(nonce)
        .bind(ciphertext)
        .bind(challenge.created_at)
        .bind(challenge.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(true)
    }

    async fn find_by_code(
        &self,
        code_hash: &str,
        now: TimestampMicros,
    ) -> Result<Option<MfaChallenge>> {
        sqlx::query(&format!(
            "SELECT {CHALLENGE_COLS} FROM one_time_tokens
             WHERE token_hash = $1 AND kind = 'mfa_challenge' AND expires_at > $2"
        ))
        .bind(code_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(|row| self.row_to_challenge(row))
        .transpose()
    }

    async fn record_failure(&self, id: &Id) -> Result<i32> {
        let attempts: Option<i32> = sqlx::query(
            "UPDATE one_time_tokens SET attempts = attempts + 1
             WHERE kind = 'mfa_challenge' AND id = $1 RETURNING attempts",
        )
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(|row| row.try_get("attempts"))
        .transpose()
        .map_err(sqlx_err)?;

        Ok(attempts.unwrap_or(i32::MAX))
    }

    async fn take(&self, id: &Id) -> Result<bool> {
        let deleted =
            sqlx::query("DELETE FROM one_time_tokens WHERE kind = 'mfa_challenge' AND id = $1")
                .bind(&id.0)
                .execute(&self.pool)
                .await
                .map_err(sqlx_err)?
                .rows_affected();

        Ok(deleted == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purposes_round_trip_through_their_stored_names() {
        for purpose in [
            MfaChallengePurpose::SignIn,
            MfaChallengePurpose::TotpSetup,
            MfaChallengePurpose::PasskeyRegister,
            MfaChallengePurpose::PasskeyProof,
            MfaChallengePurpose::PasskeySignIn,
            MfaChallengePurpose::Enroll,
        ] {
            // The column holds thirty-two characters.
            assert!(purpose.as_str().len() <= 32);
            assert_eq!(
                MfaChallengePurpose::parse(purpose.as_str()).unwrap(),
                purpose
            );
        }

        assert!(MfaChallengePurpose::parse("passkey").is_err());
    }

    #[test]
    fn only_passkey_requests_replace_the_oldest_at_the_limit() {
        assert!(MfaChallengePurpose::PasskeyProof.replaces_oldest());
        assert!(MfaChallengePurpose::PasskeyRegister.replaces_oldest());
        assert!(MfaChallengePurpose::PasskeySignIn.replaces_oldest());
        assert!(!MfaChallengePurpose::SignIn.replaces_oldest());
        assert!(!MfaChallengePurpose::TotpSetup.replaces_oldest());
    }
}
