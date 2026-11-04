// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 自助注册。
//!
//! 不需审批时新邮箱当场登录、已有的邮箱进不去，这个差别本身就能探测账号是否存在。只有先证明邮箱是
//! 本人的，两种情况才能回答一样：
//!
//! - 实例能发邮件：`POST /auth/signup {email, locale}` 一律回答 `verify_email`。新邮箱收到链接，已有
//!   账号的邮箱收到「你已经有账号了」。本人打开链接（`POST /auth/signup/verify` 取回邮箱）后才设名字
//!   和密码（`POST /auth/signup/complete`），账号这时才建好，所以别人替你发起的注册拿不到账号。
//! - 实例不能发邮件：证明不了邮箱归属，只能走审批。`POST /auth/signup {email, display_name, password}`
//!   一律回答 `pending`：新邮箱建待审批的账号，已有的不动。
//!
//! 查账号、建账号和发信都放到后台，回答的内容和耗时都不因账号存不存在而不同。发信和建待审批账号都
//! 占发信限额（[`SigninLimiter::take_mail_quota`]），超了回答不变，否则注册会变成向任意地址发信的
//! 工具。
//!
//! [`SigninLimiter::take_mail_quota`]: crate::app::iam::signin_limit::SigninLimiter::take_mail_quota

use std::net::IpAddr;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};

use super::{
    SessionView,
    password_reset::{
        generate_link_token, link_language, link_token_digest, validate_new_password,
    },
    signin_target,
};
use crate::{
    api::{
        AppState,
        http::{
            routes::{iam::directory::notify_admins_pending_user, mfa},
            signin_limit::SigninSource,
        },
    },
    app::iam::{SignInMethod, hash_password_off_thread},
    domain::{
        i18n_language::LanguagePack,
        iam::{IamAssignedRole, UserStatus},
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// 注册链接的有效期。
const SIGNUP_LINK_TTL_MICROS: i64 = 30 * 60 * 1_000_000;
/// 注册邮件的主题与正文在 `i18n_language` 表里的键前缀。
const SIGNUP_EMAIL_KEY: &str = "auth.email.signup";
const MAX_EMAIL_LENGTH: usize = 255;
const MAX_DISPLAY_NAME_LENGTH: usize = 255;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/signup", post(signup))
        .route("/auth/signup/verify", post(verify))
        .route("/auth/signup/complete", post(complete))
}

/// 实例能给申请人发验证邮件：和重置密码同样的条件。
pub(crate) fn can_verify_email(state: &AppState) -> bool {
    state.iam.email_sender.is_some() && !external_base(state).is_empty()
}

/// 邮件里的地址必须来自可信配置，不能采用请求的 Host / Origin。
fn external_base(state: &AppState) -> &str {
    state.platform.external_url.trim().trim_end_matches('/')
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignupReq {
    email: String,
    /// 实例不能发邮件时才要：能发邮件时，本人打开链接之后才设。
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    locale: Option<String>,
}

#[derive(Debug, Serialize)]
struct SignupAccepted {
    /// `verify_email`（去看邮箱）或 `pending`（等审批）。
    status: &'static str,
}

fn accepted(status: &'static str) -> (StatusCode, Json<SignupAccepted>) {
    (StatusCode::ACCEPTED, Json(SignupAccepted { status }))
}

/// 公开自助注册（免认证白名单）：策略关闭时返 403。这里的回答和耗时只看实例的配置和请求本身，不看
/// 账号存不存在。
async fn signup(
    State(state): State<AppState>,
    SigninSource(source): SigninSource,
    Json(req): Json<SignupReq>,
) -> Result<(StatusCode, Json<SignupAccepted>)> {
    let policy = state.iam.instance_settings.get().await?;

    if !policy.signup_enabled {
        return Err(Error::forbidden("self-service signup is disabled"));
    }

    // 注册进的组织只允许单点登录时，这样建出来的账号进不去。
    let organization = state.iam.service.enabled_tenant_organization().await?;
    let requirements = state.iam.signin_policies.get(&organization.id).await?;

    if requirements.sso_only {
        return Err(mfa::sso_only(&organization.name));
    }

    let email = checked_email(&req.email)?;

    if can_verify_email(&state) {
        if take_quota(&state, &email, source).await? {
            crate::shared::trace_context::spawn_with_current_trace_context(async move {
                offer_signup(&state, &email, req.locale.as_deref()).await;
            });
        }

        return Ok(accepted("verify_email"));
    }

    let display_name = checked_display_name(req.display_name.as_deref().unwrap_or_default())?;
    let password = req.password.unwrap_or_default();

    validate_new_password(&password)?;

    if take_quota(&state, &email, source).await? {
        // 已有的邮箱也算一遍，耗时才一样。
        let password_hash = hash_password_off_thread(&password).await?;

        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            apply_for_approval(&state, email, display_name, password_hash).await;
        });
    }

    Ok(accepted("pending"))
}

/// 占一次限额。用完了什么都不做，回答不变：先占再算 Argon2，一个来源刷不满同时算的名额。
async fn take_quota(state: &AppState, email: &str, source: Option<IpAddr>) -> Result<bool> {
    let taken = state
        .iam
        .signin_limiter
        .take_mail_quota(email, source)
        .await?;

    if !taken {
        tracing::info!("signup: quota used up; request ignored");
    }

    Ok(taken)
}

fn checked_email(email: &str) -> Result<String> {
    let email = email.trim();

    if email.len() > MAX_EMAIL_LENGTH || !email.contains('@') {
        return Err(Error::invalid("enter a valid email address"));
    }

    Ok(email.to_owned())
}

fn checked_display_name(display_name: &str) -> Result<String> {
    let display_name = display_name.trim();

    if display_name.is_empty() || display_name.len() > MAX_DISPLAY_NAME_LENGTH {
        return Err(Error::invalid(format!(
            "display_name must contain 1 to {MAX_DISPLAY_NAME_LENGTH} characters"
        )));
    }

    Ok(display_name.to_owned())
}

/// 给这个邮箱发信：新邮箱是注册链接，已有账号的邮箱是「你已经有账号了」。
async fn offer_signup(state: &AppState, email: &str, locale: Option<&str>) {
    let Some(sender) = state.iam.email_sender.as_ref() else {
        return;
    };

    let copy = match state
        .platform
        .i18n_language
        .load_pack(SIGNUP_EMAIL_KEY, link_language(locale))
        .await
    {
        Ok(copy) => copy,
        Err(error) => {
            tracing::warn!(error = %error, "signup: email copy could not be read");
            return;
        }
    };

    let existing = match state.iam.service.users.get_by_email(email).await {
        // 被拒绝过的可以重新申请。
        Ok(user) => user.status != UserStatus::Rejected,
        Err(Error::NotFound(_)) => false,
        Err(error) => {
            tracing::warn!(error = %error, "signup: user lookup failed");
            return;
        }
    };

    let base_url = external_base(state);

    let written = if existing {
        existing_account_email(&copy, &format!("{base_url}/signin"))
    } else {
        let token = generate_link_token();
        let now = TimestampMicros::now();
        let expires_at = TimestampMicros(now.0.saturating_add(SIGNUP_LINK_TTL_MICROS));

        let created = state
            .iam
            .signup_requests
            .create(&link_token_digest(&token), email, now, expires_at)
            .await;

        if let Err(error) = created {
            tracing::warn!(error = %error, "signup: request could not be stored");
            return;
        }

        // 放在 URL fragment 中，浏览器不会把原始令牌发送到 Web 服务器访问日志或 Referrer。
        verify_email(&copy, &format!("{base_url}/signup#signupToken={token}"))
    };

    let (subject, body) = match written {
        Ok(email) => email,
        Err(error) => {
            tracing::warn!(error = %error, "signup: email could not be written");
            return;
        }
    };

    if let Err(error) = sender.send_text(&[email.to_owned()], &subject, &body).await {
        tracing::warn!(error = %error, "signup: email send failed");
    }
}

/// 证明不了邮箱归属时：新邮箱建待审批的账号，已有的不动。
async fn apply_for_approval(
    state: &AppState,
    email: String,
    display_name: String,
    password_hash: String,
) {
    let created = state
        .iam
        .service
        .signup(email, display_name, password_hash, UserStatus::Pending)
        .await;

    let user = match created {
        Ok(user) => user,
        // 已有账号。
        Err(Error::InvalidArgument(_)) => return,
        Err(error) => {
            tracing::warn!(error = %error, "signup: pending account could not be created");
            return;
        }
    };

    match state.iam.service.enabled_tenant_organization().await {
        Ok(organization) => {
            notify_admins_pending_user(state, &organization.id, &user.email).await;
        }
        Err(error) => {
            tracing::warn!(error = %error, "signup: organization to notify could not be read");
        }
    }
}

#[derive(Deserialize)]
struct VerifyReq {
    token: String,
}

#[derive(Serialize)]
struct VerifyResp {
    email: String,
}

/// 打开链接：回答它证明的邮箱，令牌还能用。
async fn verify(
    State(state): State<AppState>,
    Json(req): Json<VerifyReq>,
) -> Result<Json<VerifyResp>> {
    let email = state
        .iam
        .signup_requests
        .email_of(&link_digest(&req.token)?, TimestampMicros::now())
        .await?
        .ok_or_else(invalid_link)?;

    Ok(Json(VerifyResp { email }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompleteReq {
    token: String,
    display_name: String,
    password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignupResp {
    /// "active"（已激活，已经登录）、"pending"（待审批，没有登录）或 "signin_required"（账号建好了，
    /// 去登录时现场开启多因素认证）。
    status: &'static str,
    user_id: String,
    email: String,
    display_name: String,
    org_id: Option<String>,
    org_name: Option<String>,
    display_role: Option<String>,
    roles: Vec<IamAssignedRole>,
}

/// 本人设好名字和密码，账号这时才建好。新用户进默认 org（首个）+ Viewer。
async fn complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CompleteReq>,
) -> Result<Response> {
    let policy = state.iam.instance_settings.get().await?;

    if !policy.signup_enabled {
        return Err(Error::forbidden("self-service signup is disabled"));
    }

    let organization = state.iam.service.enabled_tenant_organization().await?;
    let requirements = state.iam.signin_policies.get(&organization.id).await?;

    if requirements.sso_only {
        return Err(mfa::sso_only(&organization.name));
    }

    let display_name = checked_display_name(&req.display_name)?;
    validate_new_password(&req.password)?;

    // 链接不对就不必算 Argon2。都没问题才用掉链接：填错了可以再填。
    let token_hash = link_digest(&req.token)?;

    state
        .iam
        .signup_requests
        .email_of(&token_hash, TimestampMicros::now())
        .await?
        .ok_or_else(invalid_link)?;

    let password_hash = hash_password_off_thread(&req.password).await?;

    let email = state
        .iam
        .signup_requests
        .take(&token_hash, TimestampMicros::now())
        .await?
        .ok_or_else(invalid_link)?;

    let status = if policy.signup_require_approval {
        UserStatus::Pending
    } else {
        UserStatus::Active
    };

    let user = state
        .iam
        .service
        .signup(email, display_name, password_hash, status)
        .await?;

    if matches!(status, UserStatus::Pending) {
        // best-effort 通知该 org 管理员（无 SMTP / 失败仅 warn，绝不阻塞注册）。
        notify_admins_pending_user(&state, &organization.id, &user.email).await;

        return Ok(Json(SignupResp {
            status: "pending",
            user_id: user.id.0,
            email: user.email,
            display_name: user.display_name,
            org_id: None,
            org_name: None,
            display_role: None,
            roles: Vec::new(),
        })
        .into_response());
    }

    // 组织要求多因素认证：账号建好了，登录时现场开启。
    if requirements.require_mfa {
        return Ok(Json(SignupResp {
            status: "signin_required",
            user_id: user.id.0,
            email: user.email,
            display_name: user.display_name,
            org_id: None,
            org_name: None,
            display_role: None,
            roles: Vec::new(),
        })
        .into_response());
    }

    // active：取 membership 登录（与 signin 一致），不记住。新账号还没有第二因素。
    let method = SignInMethod::single_factor();
    let (org, roles, scope) = signin_target(&state, &user.id, &method).await?;
    let session = SessionView::new(user, org, roles, scope);
    let cookies = session.sign_in(&state, &headers, false, method)?;

    Ok((
        cookies,
        Json(SignupResp {
            status: "active",
            user_id: session.user_id,
            email: session.email,
            display_name: session.display_name,
            org_id: Some(session.org_id),
            org_name: Some(session.org_name),
            display_role: Some(session.display_role),
            roles: session.roles,
        }),
    )
        .into_response())
}

fn invalid_link() -> Error {
    Error::invalid("signup link is invalid or has expired")
}

fn link_digest(token: &str) -> Result<String> {
    let token = token.trim();

    if !(32..=128).contains(&token.len()) {
        return Err(invalid_link());
    }

    Ok(link_token_digest(token))
}

/// 注册链接的邮件。文案来自 `i18n_language` 表；请求方填的内容一律不进邮件。
fn verify_email(copy: &LanguagePack, signup_url: &str) -> Result<(String, String)> {
    let minutes = (SIGNUP_LINK_TTL_MICROS / 60_000_000).to_string();
    let fills = [("signup_url", signup_url), ("minutes", minutes.as_str())];

    Ok((
        copy.render(&format!("{SIGNUP_EMAIL_KEY}.verify.subject"), &fills)?,
        copy.render(&format!("{SIGNUP_EMAIL_KEY}.verify.body"), &fills)?,
    ))
}

/// 「你已经有账号了」的邮件。
fn existing_account_email(copy: &LanguagePack, signin_url: &str) -> Result<(String, String)> {
    let fills = [("signin_url", signin_url)];

    Ok((
        copy.render(&format!("{SIGNUP_EMAIL_KEY}.existing.subject"), &fills)?,
        copy.render(&format!("{SIGNUP_EMAIL_KEY}.existing.body"), &fills)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::i18n_language_seed::seeded_pack;

    #[test]
    fn an_address_is_trimmed_and_needs_an_at_sign() {
        assert_eq!(
            checked_email(" alice@example.com ").unwrap(),
            "alice@example.com"
        );

        assert!(checked_email("alice").is_err());
        assert!(checked_email("").is_err());
        assert!(checked_email(&format!("{}@example.com", "a".repeat(250))).is_err());
    }

    #[test]
    fn a_name_is_trimmed_and_bounded() {
        assert_eq!(checked_display_name(" Alice ").unwrap(), "Alice");
        assert!(checked_display_name("   ").is_err());
        assert!(checked_display_name(&"a".repeat(256)).is_err());
    }

    #[test]
    fn a_link_token_of_the_wrong_length_is_invalid_without_a_lookup() {
        assert!(link_digest("short").is_err());
        assert!(link_digest(&"x".repeat(129)).is_err());
        assert_eq!(link_digest(&"x".repeat(43)).unwrap().len(), 64);
    }

    #[test]
    fn both_emails_are_written_in_both_languages() {
        let link = "https://example.test/signup#signupToken=abc";
        let signin = "https://example.test/signin";

        let (subject, body) = verify_email(&seeded_pack(SIGNUP_EMAIL_KEY, "en-us"), link).unwrap();
        assert_eq!(subject, "MoleSignal: finish creating your account");
        assert!(body.contains(&format!(
            "within 30 minutes to choose a name and a password:\n{link}\n"
        )));

        let (subject, body) = verify_email(&seeded_pack(SIGNUP_EMAIL_KEY, "zh-cn"), link).unwrap();
        assert_eq!(subject, "MoleSignal：完成注册");
        assert!(body.contains(&format!(
            "请在 30 分钟内打开以下链接，设置名字和密码：\n{link}\n"
        )));

        let (subject, body) =
            existing_account_email(&seeded_pack(SIGNUP_EMAIL_KEY, "en-us"), signin).unwrap();
        assert_eq!(subject, "MoleSignal: you already have an account");
        assert!(body.contains(&format!("Sign in: {signin}\n")));

        let (subject, body) =
            existing_account_email(&seeded_pack(SIGNUP_EMAIL_KEY, "zh-cn"), signin).unwrap();
        assert_eq!(subject, "MoleSignal：你已经有账号了");
        assert!(body.contains(&format!("登录地址：{signin}\n")));
    }

    #[test]
    fn the_answer_to_a_signup_says_nothing_about_the_account() {
        let verify = serde_json::to_value(SignupAccepted {
            status: "verify_email",
        })
        .unwrap();

        assert_eq!(verify, serde_json::json!({ "status": "verify_email" }));
    }
}
