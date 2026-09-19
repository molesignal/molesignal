// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    api::AppState,
    app::iam::hash_password,
    domain::iam::{IamAssignedRole, Organization, PLATFORM_ADMINISTRATOR_ROLE_PURPOSE, UserStatus},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const PASSWORD_RESET_TTL_MICROS: i64 = 30 * 60 * 1_000_000;
const PASSWORD_RESET_COOLDOWN_MICROS: i64 = 60 * 1_000_000;
const MIN_PASSWORD_CHARS: usize = 8;
const MAX_PASSWORD_CHARS: usize = 256;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/signin", post(signin))
        .route("/auth/signup", post(signup))
        .route("/auth/forgot-password", post(forgot_password))
        .route("/auth/reset-password", post(reset_password))
}

#[derive(Deserialize)]
pub struct SigninReq {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct SigninResp {
    pub token: String,
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub org_id: String,
    pub org_name: String,
    pub display_role: String,
    pub roles: Vec<IamAssignedRole>,
}

async fn signin(
    State(state): State<AppState>,
    Json(req): Json<SigninReq>,
) -> Result<Json<SigninResp>> {
    let user = state
        .iam
        .service
        .authenticate(&req.email, &req.password)
        .await?;
    let (org, roles, system_scope) = signin_target(&state, &user.id).await?;
    let display_role = roles
        .iter()
        .map(|role| role.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let token = if system_scope {
        state.iam.service.issue_system_token(&user.id, &org.id)?
    } else {
        state.iam.service.issue_token(&user.id, &org.id)?
    };
    Ok(Json(SigninResp {
        token,
        user_id: user.id.0,
        email: user.email,
        display_name: user.display_name,
        org_id: org.id.0,
        org_name: org.name,
        display_role,
        roles,
    }))
}

async fn signin_target(
    state: &AppState,
    user_id: &Id,
) -> Result<(Organization, Vec<IamAssignedRole>, bool)> {
    for membership in state
        .iam
        .service
        .iam_memberships
        .list_for_user(user_id)
        .await?
    {
        let organization = state.iam.service.orgs.get(&membership.org_id).await?;
        if organization.system || organization.disabled {
            continue;
        }
        let roles = state
            .iam
            .service
            .iam_memberships
            .assigned_roles(user_id, &organization.id)
            .await?;
        return Ok((organization, roles, false));
    }

    if state.iam.platform_administrators.is_active(user_id).await? {
        let organization = state.iam.service.orgs.get(&state.iam.system_org_id).await?;
        let role = state
            .iam
            .access
            .repository()
            .role_for_purpose(
                &state.iam.system_org_id,
                PLATFORM_ADMINISTRATOR_ROLE_PURPOSE,
            )
            .await?
            .ok_or_else(|| {
                Error::internal("platform administrator IAM role is not materialized for `_sys`")
            })?;
        return Ok((organization, vec![role], true));
    }

    Err(Error::forbidden(
        "user has no enabled organization membership",
    ))
}

#[derive(Deserialize)]
pub struct SignupReq {
    pub email: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct SignupResp {
    /// "active"（已激活，附 token 直接登录）或 "pending"（待审批，无 token）。
    pub status: String,
    pub token: Option<String>,
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub org_id: Option<String>,
    pub org_name: Option<String>,
    pub display_role: Option<String>,
    pub roles: Vec<IamAssignedRole>,
}

/// 公开自助注册（免认证白名单）：策略关闭时返 403；需审批 → pending（无 token），
/// 否则 active → 直接签发 token 登录。新用户进默认 org（首个）+ Viewer。
async fn signup(
    State(state): State<AppState>,
    Json(req): Json<SignupReq>,
) -> Result<Json<SignupResp>> {
    use crate::domain::iam::UserStatus;
    let policy = state.iam.instance_settings.get().await?;
    if !policy.signup_enabled {
        return Err(Error::forbidden("self-service signup is disabled"));
    }
    let status = if policy.signup_require_approval {
        UserStatus::Pending
    } else {
        UserStatus::Active
    };
    let user = state
        .iam
        .service
        .signup(req.email, req.display_name, &req.password, status)
        .await?;
    if matches!(status, UserStatus::Pending) {
        // best-effort 通知该 org 管理员（无 SMTP / 失败仅 warn，绝不阻塞注册）。
        if let Ok((organization, _, _)) = signin_target(&state, &user.id).await {
            crate::api::http::routes::iam::directory::notify_admins_pending_user(
                &state,
                &organization.id,
                &user.email,
            )
            .await;
        }
        return Ok(Json(SignupResp {
            status: "pending".into(),
            token: None,
            user_id: user.id.0,
            email: user.email,
            display_name: user.display_name,
            org_id: None,
            org_name: None,
            display_role: None,
            roles: Vec::new(),
        }));
    }
    // active：取 membership 签发 token（与 signin 一致）。
    let (org, roles, system_scope) = signin_target(&state, &user.id).await?;
    let display_role = roles
        .iter()
        .map(|role| role.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let token = if system_scope {
        state.iam.service.issue_system_token(&user.id, &org.id)?
    } else {
        state.iam.service.issue_token(&user.id, &org.id)?
    };
    Ok(Json(SignupResp {
        status: "active".into(),
        token: Some(token),
        user_id: user.id.0,
        email: user.email,
        display_name: user.display_name,
        org_id: Some(org.id.0),
        org_name: Some(org.name),
        display_role: Some(display_role),
        roles,
    }))
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
/// 无论邮箱是否存在、账号是否可用、SMTP 是否配置，都返回相同响应，避免账号枚举。
/// 真实故障只写服务端日志，不把差异暴露给调用方。
async fn forgot_password(
    State(state): State<AppState>,
    Json(req): Json<ForgotPasswordReq>,
) -> (StatusCode, Json<ForgotPasswordResp>) {
    let accepted = || {
        (
            StatusCode::ACCEPTED,
            Json(ForgotPasswordResp { accepted: true }),
        )
    };

    let email = req.email.trim();
    if email.is_empty() {
        return accepted();
    }

    let Some(sender) = state.iam.email_sender.as_ref() else {
        tracing::warn!("password-reset: request accepted but SMTP is not configured");
        return accepted();
    };

    // 邮件中的目标地址必须来自可信配置，不能采用请求的 Host / Origin，避免
    // password-reset poisoning。生产环境启用 SMTP 时应同时配置 http.external_url。
    let base_url = state.platform.external_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        tracing::warn!(
            "password-reset: request accepted but http.external_url is empty; email not sent"
        );
        return accepted();
    }

    let user = match state.iam.service.users.get_by_email(email).await {
        Ok(user) if !user.disabled && matches!(user.status, UserStatus::Active) => user,
        Ok(_) | Err(Error::NotFound(_)) => return accepted(),
        Err(error) => {
            tracing::warn!(error = %error, "password-reset: user lookup failed");
            return accepted();
        }
    };

    let raw_token = generate_reset_token();
    let token_hash = hash_reset_token(&raw_token);
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
            return accepted();
        }
    };
    if !issued {
        return accepted();
    }

    // 放在 URL fragment 中，浏览器不会把原始令牌发送到 Web 服务器访问日志或 Referrer。
    let reset_url = format!("{base_url}/signin#reset_token={raw_token}");
    let (subject, body) = reset_email(req.locale.as_deref(), &reset_url);
    let sender = sender.clone();
    crate::shared::trace_context::spawn_with_current_trace_context(async move {
        if let Err(error) = sender.send_text(&[user.email], subject, &body).await {
            tracing::warn!(error = %error, "password-reset: email send failed");
        }
    });

    accepted()
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
    Json(req): Json<ResetPasswordReq>,
) -> Result<Json<ResetPasswordResp>> {
    validate_new_password(&req.password)?;

    let token = req.token.trim();
    if !(32..=128).contains(&token.len()) {
        return Err(Error::invalid("reset link is invalid or has expired"));
    }
    let token_hash = hash_reset_token(token);
    let password_hash = hash_password(&req.password)?;
    let reset = state
        .iam
        .password_resets
        .consume_and_update_password(&token_hash, &password_hash, TimestampMicros::now())
        .await?;
    if !reset {
        return Err(Error::invalid("reset link is invalid or has expired"));
    }
    Ok(Json(ResetPasswordResp { reset: true }))
}

fn validate_new_password(password: &str) -> Result<()> {
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

fn generate_reset_token() -> String {
    use rand::TryRng as _;

    let mut bytes = [0_u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("operating-system random source");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_reset_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn reset_email(locale: Option<&str>, reset_url: &str) -> (&'static str, String) {
    if locale.is_some_and(|locale| locale.to_ascii_lowercase().starts_with("zh")) {
        (
            "MoleSignal：重置你的密码",
            format!(
                "我们收到了你的密码重置请求。\n\n请在 30 分钟内打开以下链接设置新密码：\n{reset_url}\n\n如果这不是你的操作，请忽略本邮件；你的密码不会发生变化。\n"
            ),
        )
    } else {
        (
            "MoleSignal: reset your password",
            format!(
                "We received a request to reset your password.\n\nOpen the link below within 30 minutes to choose a new password:\n{reset_url}\n\nIf you did not request this, you can ignore this email. Your password will not change.\n"
            ),
        )
    }
}

#[cfg(test)]
mod password_reset_tests {
    use super::*;

    #[test]
    fn generated_tokens_are_url_safe_and_not_stored_verbatim() {
        let first = generate_reset_token();
        let second = generate_reset_token();
        assert_eq!(first.len(), 43);
        assert_ne!(first, second);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        );
        let digest = hash_reset_token(&first);
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
    fn reset_email_is_localized() {
        let (_, zh) = reset_email(Some("zh-CN"), "https://example.test/reset");
        let (_, en) = reset_email(Some("en-US"), "https://example.test/reset");
        assert!(zh.contains("30 分钟"));
        assert!(en.contains("30 minutes"));
    }
}
