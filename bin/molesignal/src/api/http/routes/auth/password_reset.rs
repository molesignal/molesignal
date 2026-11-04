// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 忘记密码：邮件里的一次性链接，凭它设置新密码。登录锁定不挡这条路，重置成功立即解开账号。

use std::net::IpAddr;

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    api::{AppState, http::signin_limit::SigninSource},
    app::iam::hash_password_off_thread,
    domain::{i18n_language::LanguagePack, iam::UserStatus},
    shared::{Error, Result, time::TimestampMicros},
};

const PASSWORD_RESET_TTL_MICROS: i64 = 30 * 60 * 1_000_000;
/// 重置邮件的主题与正文在 `i18n_language` 表里的键前缀。
const RESET_EMAIL_KEY: &str = "auth.email.password_reset";
const PASSWORD_RESET_COOLDOWN_MICROS: i64 = 60 * 1_000_000;
const MIN_PASSWORD_CHARS: usize = 8;
const MAX_PASSWORD_CHARS: usize = 256;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/forgot-password", post(forgot_password))
        .route("/auth/reset-password", post(reset_password))
}

#[derive(Deserialize)]
pub struct ForgotPasswordReq {
    pub email: String,
    #[serde(default)]
    pub locale: Option<String>,
}

#[derive(Serialize)]
pub struct ForgotPasswordResp {
    pub accepted: bool,
}

/// 申请密码重置。
///
/// 无论邮箱是否存在、账号是否可用、SMTP 是否配置，都返回相同响应，耗时也一样：查账号和发信都放到
/// 后台。真实故障只写服务端日志，不把差异暴露给调用方。
async fn forgot_password(
    State(state): State<AppState>,
    SigninSource(source): SigninSource,
    Json(req): Json<ForgotPasswordReq>,
) -> (StatusCode, Json<ForgotPasswordResp>) {
    let email = req.email.trim().to_owned();

    if !email.is_empty() {
        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            send_reset_email(&state, &email, req.locale.as_deref(), source).await;
        });
    }

    (
        StatusCode::ACCEPTED,
        Json(ForgotPasswordResp { accepted: true }),
    )
}

async fn send_reset_email(
    state: &AppState,
    email: &str,
    locale: Option<&str>,
    source: Option<IpAddr>,
) {
    let Some(sender) = state.iam.email_sender.as_ref() else {
        tracing::warn!("password-reset: request accepted but SMTP is not configured");
        return;
    };

    // 邮件中的目标地址必须来自可信配置，不能采用请求的 Host / Origin，避免
    // password-reset poisoning。生产环境启用 SMTP 时应同时配置 http.external_url。
    let base_url = state.platform.external_url.trim().trim_end_matches('/');

    if base_url.is_empty() {
        tracing::warn!(
            "password-reset: request accepted but http.external_url is empty; email not sent"
        );
        return;
    }

    // 新的链接会作废上一封。限额用完之后不再发，最后一封的链接就能用满有效期。
    match state
        .iam
        .signin_limiter
        .take_mail_quota(email, source)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            tracing::info!("password-reset: email quota used up; email not sent");
            return;
        }
        Err(error) => {
            tracing::warn!(error = %error, "password-reset: email quota could not be read");
            return;
        }
    }

    let user = match state.iam.service.users.get_by_email(email).await {
        Ok(user) if !user.disabled && matches!(user.status, UserStatus::Active) => user,
        Ok(_) | Err(Error::NotFound(_)) => return,
        Err(error) => {
            tracing::warn!(error = %error, "password-reset: user lookup failed");
            return;
        }
    };

    // 先取文案：取不到就没有邮件可发，不必签发一个只会占用冷却时间的令牌。
    let copy = match state
        .platform
        .i18n_language
        .load_pack(RESET_EMAIL_KEY, link_language(locale))
        .await
    {
        Ok(copy) => copy,
        Err(error) => {
            tracing::warn!(error = %error, "password-reset: email copy could not be read");
            return;
        }
    };

    let raw_token = generate_link_token();
    let token_hash = link_token_digest(&raw_token);
    let now = TimestampMicros::now();
    let expires_at = TimestampMicros(now.0.saturating_add(PASSWORD_RESET_TTL_MICROS));

    let issued = match state
        .iam
        .password_resets
        .issue(
            &user.id,
            &token_hash,
            now,
            expires_at,
            PASSWORD_RESET_COOLDOWN_MICROS,
        )
        .await
    {
        Ok(issued) => issued,
        Err(error) => {
            tracing::warn!(error = %error, "password-reset: token issue failed");
            return;
        }
    };

    if !issued {
        return;
    }

    // 放在 URL fragment 中，浏览器不会把原始令牌发送到 Web 服务器访问日志或 Referrer。
    let reset_url = format!("{base_url}/signin#resetToken={raw_token}");

    let (subject, body) = match reset_email(&copy, &reset_url) {
        Ok(email) => email,
        Err(error) => {
            tracing::warn!(error = %error, "password-reset: email could not be written");
            return;
        }
    };

    if let Err(error) = sender.send_text(&[user.email], &subject, &body).await {
        tracing::warn!(error = %error, "password-reset: email send failed");
    }
}

#[derive(Deserialize)]
pub struct ResetPasswordReq {
    pub token: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct ResetPasswordResp {
    pub reset: bool,
}

async fn reset_password(
    State(state): State<AppState>,
    SigninSource(source): SigninSource,
    Json(req): Json<ResetPasswordReq>,
) -> Result<Json<ResetPasswordResp>> {
    validate_new_password(&req.password)?;

    let token = req.token.trim();

    if !(32..=128).contains(&token.len()) {
        return Err(Error::invalid("reset link is invalid or has expired"));
    }

    let token_hash = link_token_digest(token);

    if !state
        .iam
        .password_resets
        .is_usable(&token_hash, TimestampMicros::now())
        .await?
    {
        return Err(Error::invalid("reset link is invalid or has expired"));
    }

    let password_hash = hash_password_off_thread(&req.password).await?;

    let email = state
        .iam
        .password_resets
        .consume_and_update_password(&token_hash, &password_hash, TimestampMicros::now())
        .await?
        .ok_or_else(|| Error::invalid("reset link is invalid or has expired"))?;

    // 本人证明了邮箱：别人输错密码加的锁解开，这个来源也不再受账号限速。
    if let Err(error) = state.iam.signin_limiter.forgive(&email, source).await {
        tracing::warn!(error = %error, "password-reset: sign-in failures could not be cleared");
    }

    Ok(Json(ResetPasswordResp { reset: true }))
}

pub(super) fn validate_new_password(password: &str) -> Result<()> {
    let count = password.chars().count();

    if count < MIN_PASSWORD_CHARS {
        return Err(Error::invalid(format!(
            "password must contain at least {MIN_PASSWORD_CHARS} characters"
        )));
    }

    if count > MAX_PASSWORD_CHARS {
        return Err(Error::invalid(format!(
            "password must contain at most {MAX_PASSWORD_CHARS} characters"
        )));
    }

    Ok(())
}

/// 一次性链接里的令牌：注册和重置密码共用。
pub(super) fn generate_link_token() -> String {
    use rand::TryRng as _;

    let mut bytes = [0_u8; 32];

    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("operating-system random source");

    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub(super) fn link_token_digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// 请求里的 locale 归一为语言表里的语言代码：`zh` 开头用中文，其余用英文。
pub(super) fn link_language(locale: Option<&str>) -> &'static str {
    if locale.is_some_and(|locale| locale.to_ascii_lowercase().starts_with("zh")) {
        "zh-cn"
    } else {
        "en-us"
    }
}

/// 重置邮件的主题与正文。文案来自 `i18n_language` 表，这里只填入链接和有效期。
fn reset_email(copy: &LanguagePack, reset_url: &str) -> Result<(String, String)> {
    let minutes = (PASSWORD_RESET_TTL_MICROS / 60_000_000).to_string();
    let fills = [("reset_url", reset_url), ("minutes", minutes.as_str())];

    Ok((
        copy.render(&format!("{RESET_EMAIL_KEY}.subject"), &fills)?,
        copy.render(&format!("{RESET_EMAIL_KEY}.body"), &fills)?,
    ))
}

#[cfg(test)]
mod password_reset_tests {
    use super::*;

    #[test]
    fn generated_tokens_are_url_safe_and_not_stored_verbatim() {
        let first = generate_link_token();
        let second = generate_link_token();
        assert_eq!(first.len(), 43);
        assert_ne!(first, second);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        );
        let digest = link_token_digest(&first);
        assert_eq!(digest.len(), 64);
        assert!(!digest.contains(&first));
    }

    #[test]
    fn reset_password_length_is_bounded() {
        assert!(validate_new_password("1234567").is_err());
        assert!(validate_new_password("12345678").is_ok());
        assert!(validate_new_password(&"x".repeat(MAX_PASSWORD_CHARS + 1)).is_err());
    }

    #[test]
    fn reset_email_is_written_in_the_requested_language() {
        use crate::shared::i18n_language_seed::seeded_pack;

        assert_eq!(link_language(Some("zh-CN")), "zh-cn");
        assert_eq!(link_language(Some("en-US")), "en-us");
        assert_eq!(link_language(None), "en-us");

        let url = "https://example.test/signin#resetToken=abc";
        let (zh_subject, zh) = reset_email(&seeded_pack(RESET_EMAIL_KEY, "zh-cn"), url).unwrap();
        let (en_subject, en) = reset_email(&seeded_pack(RESET_EMAIL_KEY, "en-us"), url).unwrap();

        assert_eq!(zh_subject, "MoleSignal：重置你的密码");
        assert_eq!(en_subject, "MoleSignal: reset your password");
        assert!(zh.contains(
            "请在 30 分钟内打开以下链接设置新密码：\nhttps://example.test/signin#resetToken=abc\n"
        ));
        assert!(en.contains("within 30 minutes to choose a new password:\nhttps://example.test/signin#resetToken=abc\n"));
    }
}
