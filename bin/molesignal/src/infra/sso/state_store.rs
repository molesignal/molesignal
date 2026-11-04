// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO 短期状态，存在 `one_time_tokens` 里（`kind` 为 `sso_login`、`sso_handoff`）。存在 Postgres 里，发起登录、
//! 回调和换会话落到不同节点也能接着处理。
//!
//! - 登录发起：`state`（OIDC / OAuth 2.0）或 `RelayState`（SAML）是 CSRF 防护，
//!   callback 必须带回同一个值；`nonce` 一并存下，用于 ID Token 校验；`next` 是登录后
//!   要回到的页面。
//! - 登录完成：callback 把结果（会话或失败原因）存成一次性交接凭据，浏览器带着凭据
//!   回到前端，前端再用它换会话，令牌不出现在地址栏里。
//!
//! 两类条目都只能取一次：删掉了才算拿到。过期即作废，存新条目时顺手清理过期项。库里只有凭据的
//! 摘要，其余内容用根密钥加密：结果里有账号的身份，还可能有关联请求的 `code`，而
//! `sso_link_requests` 只存它的摘要。
//!
//! 这些请求都不需要登录，所以条目数有上限，防止撑大表。到上限时挤掉最早的，不拒绝新的：开了
//! 「只允许单点登录」的组织只有这一个入口，撑满表的请求只会让别人等着的登录早点过期，挡不住登录。
//! 上限按发放顺序算（`one_time_tokens.seq`，各类一次性令牌共用），不用数行数，开销不随表的大小增长。

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

use crate::{
    infra::{cipher::CipherRootKey, persistence::sqlx_err},
    shared::{Error, Result, time::TimestampMicros},
};

/// 条目数的上限：只留最近发放的这么多个。
pub const MAX_ENTRIES: i64 = 50_000;

/// 存一个条目时最多顺手清理这么多，一次请求的开销不随积压增长。
const SWEEP_BATCH: i64 = 100;

/// 交接凭据的有效期：callback 跳回前端后立即兑换。
const HANDOFF_TTL_SECS: u64 = 120;

/// 失败原因只留这么多字：回调的 `error_description` 是请求带来的任意文字，不限制的话一个条目能有
/// 一个 URL 那么长。
const MAX_FAILURE_CHARS: usize = 1024;

const KIND_LOGIN: &str = "sso_login";
const KIND_HANDOFF: &str = "sso_handoff";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsoStateEntry {
    pub provider_id: String,
    /// SAML 没有 nonce，存空字符串。
    pub nonce: String,
    pub next: Option<String>,
    /// 这次是测试登录：回调只记结果，不登录。
    #[serde(default)]
    pub test_id: Option<String>,
}

/// 登录完成（或失败）后等前端来取的结果：会话，或要本人确认的关联，或失败原因。
#[derive(Debug)]
pub struct SsoHandoff {
    pub body: Result<serde_json::Value>,
    pub next: Option<String>,
}

/// 交接结果存下来的样子。失败存错误码和文字，取出来还是同一个错误。
#[derive(Debug, Serialize, Deserialize)]
struct StoredHandoff {
    outcome: StoredOutcome,
    next: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredOutcome {
    Body(serde_json::Value),
    Failure { code: String, message: String },
}

impl From<SsoHandoff> for StoredHandoff {
    fn from(handoff: SsoHandoff) -> Self {
        let outcome = match handoff.body {
            Ok(body) => StoredOutcome::Body(body),
            Err(error) => StoredOutcome::Failure {
                code: error.http_error_code().to_owned(),
                message: failure_message(error)
                    .chars()
                    .take(MAX_FAILURE_CHARS)
                    .collect(),
            },
        };

        Self {
            outcome,
            next: handoff.next,
        }
    }
}

impl From<StoredHandoff> for SsoHandoff {
    fn from(stored: StoredHandoff) -> Self {
        let body = match stored.outcome {
            StoredOutcome::Body(body) => Ok(body),
            StoredOutcome::Failure { code, message } => Err(failure_of(&code, message)),
        };

        Self {
            body,
            next: stored.next,
        }
    }
}

fn failure_message(error: Error) -> String {
    match error {
        Error::NotFound(message)
        | Error::Conflict(message)
        | Error::InvalidArgument(message)
        | Error::Validation { message, .. }
        | Error::Unauthorized(message)
        | Error::Forbidden(message)
        | Error::Internal(message)
        | Error::Cancelled(message)
        | Error::ResourceExhausted(message)
        | Error::PayloadTooLarge(message)
        | Error::Unavailable(message) => message,
        Error::Other(error) => error.to_string(),
    }
}

/// [`Error::http_error_code`] 的反向。单点登录这条路径不产生带 `issues` 的校验错误。
fn failure_of(code: &str, message: String) -> Error {
    match code {
        "not_found" => Error::NotFound(message),
        "conflict" => Error::Conflict(message),
        "invalid_argument" => Error::InvalidArgument(message),
        "validation_failed" => Error::validation(message, Vec::new()),
        "unauthorized" => Error::Unauthorized(message),
        "forbidden" => Error::Forbidden(message),
        "resource_exhausted" => Error::ResourceExhausted(message),
        "payload_too_large" => Error::PayloadTooLarge(message),
        "cancelled" => Error::Cancelled(message),
        "unavailable" => Error::Unavailable(message),
        _ => Error::Internal(message),
    }
}

fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn seal<T: Serialize>(cipher: &CipherRootKey, value: &T) -> Result<(Vec<u8>, Vec<u8>)> {
    let plain = serde_json::to_vec(value)
        .map_err(|error| Error::internal(format!("sso state encode: {error}")))?;

    cipher
        .seal(&plain)
        .map_err(|error| Error::internal(format!("sso state cannot be sealed: {error}")))
}

fn open<T: DeserializeOwned>(cipher: &CipherRootKey, nonce: &[u8], ciphertext: &[u8]) -> Result<T> {
    let plain = cipher
        .open(nonce, ciphertext)
        .map_err(|error| Error::internal(format!("sso state cannot be opened: {error}")))?;

    serde_json::from_slice(&plain)
        .map_err(|error| Error::internal(format!("sso state decode: {error}")))
}

fn ttl_micros(ttl_secs: u64) -> i64 {
    (ttl_secs as i64).saturating_mul(1_000_000)
}

pub struct SsoStateStore {
    pool: PgPool,
    cipher: CipherRootKey,
    login_ttl_us: i64,
}

impl SsoStateStore {
    /// `ttl_secs` 是登录发起后等待身份源回调的时间。常规取 600（10 分钟），够 IdP
    /// 重定向加用户输密码。
    pub fn new(pool: PgPool, cipher: CipherRootKey, ttl_secs: u64) -> Self {
        Self {
            pool,
            cipher,
            login_ttl_us: ttl_micros(ttl_secs),
        }
    }

    /// Login handler 生成 `state` 后调。
    pub async fn put(&self, state: &str, entry: &SsoStateEntry) -> Result<()> {
        self.insert(KIND_LOGIN, state, entry, self.login_ttl_us)
            .await
    }

    /// Callback handler 调；同一 `state` 只能消费一次。已过期返 None。
    pub async fn take(&self, state: &str) -> Result<Option<SsoStateEntry>> {
        self.remove(KIND_LOGIN, state).await
    }

    pub async fn put_handoff(&self, code: &str, handoff: SsoHandoff) -> Result<()> {
        let stored = StoredHandoff::from(handoff);

        self.insert(KIND_HANDOFF, code, &stored, ttl_micros(HANDOFF_TTL_SECS))
            .await
    }

    pub async fn take_handoff(&self, code: &str) -> Result<Option<SsoHandoff>> {
        let stored: Option<StoredHandoff> = self.remove(KIND_HANDOFF, code).await?;

        Ok(stored.map(SsoHandoff::from))
    }

    /// 一条语句存下条目、清掉过期的、挤掉超出上限的。别的节点正在清的行跳过，不用等。
    async fn insert<T: Serialize>(
        &self,
        kind: &str,
        token: &str,
        value: &T,
        ttl_us: i64,
    ) -> Result<()> {
        let (nonce, ciphertext) = seal(&self.cipher, value)?;
        let now = TimestampMicros::now().0;

        sqlx::query(
            "WITH inserted AS (
                INSERT INTO one_time_tokens
                    (token_hash, kind, secret_nonce, secret_ciphertext, created_at, expires_at)
                VALUES ($1, $2, $3, $4, $6, $5)
                RETURNING seq
             ),
             expired AS (
                SELECT token_hash FROM one_time_tokens
                WHERE kind IN ('sso_login', 'sso_handoff') AND expires_at <= $6
                ORDER BY expires_at LIMIT $8 FOR UPDATE SKIP LOCKED
             ),
             beyond_limit AS (
                SELECT token_hash FROM one_time_tokens
                WHERE kind IN ('sso_login', 'sso_handoff')
                  AND seq <= (SELECT seq FROM inserted) - $7
                ORDER BY seq LIMIT $8 FOR UPDATE SKIP LOCKED
             )
             DELETE FROM one_time_tokens
             WHERE token_hash = ANY(ARRAY(
                SELECT token_hash FROM expired UNION ALL SELECT token_hash FROM beyond_limit
             ))",
        )
        .bind(digest(token))
        .bind(kind)
        .bind(nonce)
        .bind(ciphertext)
        .bind(TimestampMicros(now.saturating_add(ttl_us)))
        .bind(TimestampMicros(now))
        .bind(MAX_ENTRIES)
        .bind(SWEEP_BATCH)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    /// 删掉了才算拿到：同一个条目被几个节点同时取，只有一个拿得到。
    ///
    /// 过期不写进条件，取出来再看：新写入的行总在 `expires_at` 的统计之外，写进条件的话优化器
    /// 会选它的索引，把没过期的行走一遍。
    async fn remove<T: DeserializeOwned>(&self, kind: &str, token: &str) -> Result<Option<T>> {
        let row = sqlx::query(
            "DELETE FROM one_time_tokens
             WHERE token_hash = $1 AND kind = $2
             RETURNING secret_nonce, secret_ciphertext, expires_at",
        )
        .bind(digest(token))
        .bind(kind)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let expires_at: TimestampMicros = row.try_get("expires_at").map_err(sqlx_err)?;

        if expires_at <= TimestampMicros::now() {
            return Ok(None);
        }

        let nonce: Vec<u8> = row.try_get("secret_nonce").map_err(sqlx_err)?;
        let ciphertext: Vec<u8> = row.try_get("secret_ciphertext").map_err(sqlx_err)?;

        open(&self.cipher, &nonce, &ciphertext).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use serde_json::json;

    use super::*;

    fn cipher(byte: u8) -> CipherRootKey {
        let key = base64::engine::general_purpose::STANDARD.encode([byte; 32]);

        CipherRootKey::from_base64(&key).expect("a 32-byte key")
    }

    fn entry() -> SsoStateEntry {
        SsoStateEntry {
            provider_id: "pid".into(),
            nonce: "n1".into(),
            next: Some("/logs".into()),
            test_id: None,
        }
    }

    fn stored(handoff: SsoHandoff) -> SsoHandoff {
        let key = cipher(1);
        let (nonce, ciphertext) = seal(&key, &StoredHandoff::from(handoff)).unwrap();

        open::<StoredHandoff>(&key, &nonce, &ciphertext)
            .unwrap()
            .into()
    }

    fn contains(haystack: &[u8], needle: &str) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    }

    #[test]
    fn a_token_is_kept_as_its_digest() {
        assert_eq!(
            digest("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(digest("abc"), digest("abd"));

        // A digest is what the column holds, whatever length of token a request brings.
        assert_eq!(digest(&"x".repeat(100_000)).len(), 64);
    }

    #[test]
    fn a_login_entry_comes_back_unchanged() {
        let key = cipher(1);
        let (nonce, ciphertext) = seal(&key, &entry()).unwrap();

        assert_eq!(
            open::<SsoStateEntry>(&key, &nonce, &ciphertext).unwrap(),
            entry()
        );
    }

    #[test]
    fn what_is_stored_cannot_be_read_without_the_root_key() {
        let key = cipher(1);

        let handoff = StoredHandoff::from(SsoHandoff {
            body: Ok(json!({
                "link": { "code": "the-link-code", "email_hint": "a***@example.com" },
            })),
            next: Some("/secret-page".into()),
        });

        let (nonce, ciphertext) = seal(&key, &handoff).unwrap();

        for plain in ["the-link-code", "example.com", "/secret-page", "link"] {
            assert!(!contains(&ciphertext, plain), "{plain} is readable");
        }

        assert!(open::<StoredHandoff>(&cipher(2), &nonce, &ciphertext).is_err());
    }

    #[test]
    fn a_session_comes_back_with_its_next_page() {
        let body = json!({
            "login": { "user_id": "u1", "email": "alice@example.com", "roles": [{ "id": "r1" }] },
        });

        let back = stored(SsoHandoff {
            body: Ok(body.clone()),
            next: Some("/logs?q=1".into()),
        });

        assert_eq!(back.body.unwrap(), body);
        assert_eq!(back.next.as_deref(), Some("/logs?q=1"));
    }

    #[test]
    fn a_failure_comes_back_as_the_same_error() {
        for error in [
            Error::not_found("message"),
            Error::conflict("message"),
            Error::invalid("message"),
            Error::validation("message", Vec::new()),
            Error::unauthorized("message"),
            Error::forbidden("message"),
            Error::internal("message"),
            Error::cancelled("message"),
            Error::resource_exhausted("message"),
            Error::payload_too_large("message"),
            Error::unavailable("message"),
            Error::Other(anyhow::anyhow!("message")),
        ] {
            let status = error.http_status_code();
            let code = error.http_error_code();
            let shown = error.to_string();

            let back = stored(SsoHandoff {
                body: Err(error),
                next: None,
            })
            .body
            .unwrap_err();

            assert_eq!(back.http_status_code(), status, "{shown}");
            assert_eq!(back.http_error_code(), code, "{shown}");
            assert!(back.to_string().ends_with("message"), "{back}");
            assert!(!back.to_string().contains("message: message"), "{back}");
        }
    }

    #[test]
    fn a_failure_keeps_its_exact_text_as_shown_to_the_browser() {
        let text = "the sign-in request has expired or was already used; sign in again";
        let original = Error::unauthorized(text).to_string();

        let back = stored(SsoHandoff {
            body: Err(Error::unauthorized(text)),
            next: None,
        })
        .body
        .unwrap_err();

        assert_eq!(back.to_string(), original);
    }

    #[test]
    fn a_long_failure_is_cut_on_a_character() {
        let back = stored(SsoHandoff {
            body: Err(Error::unauthorized("错".repeat(70_000))),
            next: None,
        })
        .body
        .unwrap_err();

        let Error::Unauthorized(message) = back else {
            panic!("the kind of error is kept");
        };

        assert_eq!(message.chars().count(), MAX_FAILURE_CHARS);
        assert!(message.chars().all(|character| character == '错'));
    }

    #[test]
    fn the_two_kinds_fit_their_column_and_differ() {
        // The column holds thirty-two characters.
        assert!(KIND_LOGIN.len() <= 32);
        assert!(KIND_HANDOFF.len() <= 32);
        assert_ne!(KIND_LOGIN, KIND_HANDOFF);
    }
}
