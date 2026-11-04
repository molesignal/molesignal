// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 已有账号第一次通过某个服务登录：本人确认后才关联。
//!
//! - 登录回调发现邮箱已有账号：[`request`] 存下请求，把 `code` 交给发起登录的浏览器。
//! - `POST /auth/sso/link/email`：浏览器带着 `code` 请求，把 6 位验证码发到账号邮箱。要配
//!   SMTP；同一账号 60 秒内只发一次。
//! - `POST /auth/sso/link/confirm`：浏览器带着 `code` 和邮件验证码（或账号密码）确认，对上就
//!   绑定、入组、登录；输错 5 次请求作废。输账号密码时和密码登录共用输错次数：别人只要在身份源
//!   上声明这个邮箱，就能走到这里，不能让这里成为不限次猜密码的入口。账号开了多因素认证时先回答
//!   `{"mfa": …}`，第二步过了才绑定（[`complete_link`]）。
//!
//! 验证码只在发起登录的页面有用：别人发起的关联，账号主人即使收到邮件，也不会被关联上。
//! 邮件验证码证明了邮箱归属，确认时一并作废账号上未经证明的绑定。

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    policy,
    provision::{self, FederatedIdentity},
    random_token,
};
use crate::{
    api::{
        AppState,
        http::{
            routes::{
                auth::SessionView,
                mfa::{self, AfterVerification},
            },
            session_cookie::SessionCookies,
            signin_limit::SigninSource,
        },
    },
    app::iam::{
        SignInMethod, check_password,
        signin_limit::{SigninAccount, SigninError},
        signin_policy::Admission,
    },
    domain::{
        i18n_language::LanguagePack,
        iam::{SsoProvider, SsoProviderKind, User},
    },
    infra::sso::{LinkedIdentity, SsoIdentity, SsoIdentityProof, SsoLinkRequest},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 关联请求的有效期。
const REQUEST_TTL_MICROS: i64 = 30 * 60 * 1_000_000;
/// 同一账号两次发验证码的最短间隔。
const EMAIL_COOLDOWN_MICROS: i64 = 60 * 1_000_000;
/// 输错几次请求作废。
const MAX_ATTEMPTS: i32 = 5;
/// 验证码邮件的主题与正文在 `i18n_language` 表里的键前缀。
const EMAIL_KEY: &str = "auth.email.sso_link";

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/sso/link/email", post(send_code))
        .route("/auth/sso/link/confirm", post(confirm))
}

/// 发起登录的浏览器要展示的确认信息。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LinkRequired {
    code: String,
    provider_name: String,
    organization_name: String,
    /// 账号邮箱，只露首字母和域名。
    email_hint: String,
    /// 实例能发邮件，可以用邮件验证码确认。
    email_available: bool,
    /// 账号设过密码，可以用密码确认。
    password_allowed: bool,
}

pub(super) async fn request(
    state: &AppState,
    provider: &SsoProvider,
    user: &User,
    identity: &FederatedIdentity,
) -> Result<LinkRequired> {
    let code = random_token();
    let now = TimestampMicros::now();

    let created = state
        .iam
        .sso_link_requests
        .create(SsoLinkRequest {
            id: Id::new(),
            provider_id: provider.id.clone(),
            subject: identity.subject.clone(),
            user_id: user.id.clone(),
            identity: identity.snapshot(),
            code_hash: digest(&code),
            email_code_hash: None,
            email_sent_at: None,
            attempts: 0,
            created_at: now,
            expires_at: TimestampMicros(now.0.saturating_add(REQUEST_TTL_MICROS)),
        })
        .await?;

    if !created {
        return Err(Error::resource_exhausted(
            "too many sign-in confirmations are waiting for this account; try again later",
        ));
    }

    let organization = state.iam.service.orgs.get(&provider.org_id).await?;

    Ok(LinkRequired {
        code,
        provider_name: provider.name.clone(),
        organization_name: organization.name,
        email_hint: mask_email(&user.email),
        email_available: state.iam.email_sender.is_some(),
        password_allowed: !user.password_hash.is_empty(),
    })
}

#[derive(Debug, Deserialize)]
struct SendCodeRequest {
    code: String,
    #[serde(default)]
    locale: Option<String>,
}

#[derive(Debug, Serialize)]
struct SendCodeResponse {
    sent: bool,
}

async fn send_code(
    State(state): State<AppState>,
    Json(request): Json<SendCodeRequest>,
) -> Result<Json<SendCodeResponse>> {
    let now = TimestampMicros::now();
    let pending = pending(&state, &request.code, now).await?;

    let sender = state.iam.email_sender.clone().ok_or_else(|| {
        Error::invalid("this instance cannot send email; confirm with the account password")
    })?;

    let last_sent = state
        .iam
        .sso_link_requests
        .last_email_sent(&pending.user_id)
        .await?;

    if last_sent.is_some_and(|sent| now.0.saturating_sub(sent.0) < EMAIL_COOLDOWN_MICROS) {
        return Err(Error::resource_exhausted(
            "a code was sent less than a minute ago; wait before asking for another",
        ));
    }

    let user = state.iam.service.users.get(&pending.user_id).await?;
    let provider = provision::enabled_provider(&state, &pending.provider_id.0).await?;
    let organization = state.iam.service.orgs.get(&provider.org_id).await?;

    let copy = state
        .platform
        .i18n_language
        .load_pack(EMAIL_KEY, email_language(request.locale.as_deref()))
        .await?;

    let email_code = six_digits();
    let minutes = (pending.expires_at.0.saturating_sub(now.0) / 60_000_000).max(1);

    let (subject, body) = link_email(
        &copy,
        &email_code,
        minutes,
        &provider.name,
        &organization.name,
    )?;

    state
        .iam
        .sso_link_requests
        .set_email_code(&pending.id, &code_digest(&pending.id, &email_code), now)
        .await?;

    crate::shared::trace_context::spawn_with_current_trace_context(async move {
        if let Err(error) = sender.send_text(&[user.email], &subject, &body).await {
            tracing::warn!(error = %error, "sso link: verification email send failed");
        }
    });

    Ok(Json(SendCodeResponse { sent: true }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmRequest {
    code: String,
    #[serde(default)]
    email_code: Option<String>,
    #[serde(default)]
    password: Option<String>,
    /// 记住我：开始单点登录时在登录页选的。
    #[serde(default)]
    remember: bool,
}

async fn confirm(
    State(state): State<AppState>,
    SigninSource(source): SigninSource,
    headers: HeaderMap,
    Json(request): Json<ConfirmRequest>,
) -> Result<Response, SigninError> {
    let now = TimestampMicros::now();
    let pending = pending(&state, &request.code, now).await?;
    let user = state.iam.service.users.get(&pending.user_id).await?;

    let email_code = request
        .email_code
        .as_deref()
        .map(str::trim)
        .filter(|code| !code.is_empty());

    let password = request
        .password
        .as_deref()
        .filter(|password| !password.is_empty());

    let proof = match (email_code, password) {
        (Some(code), _) => (pending.email_code_hash.as_deref()
            == Some(code_digest(&pending.id, code).as_str()))
        .then_some(SsoIdentityProof::Email),
        (None, Some(password)) => {
            let attempt = state
                .iam
                .signin_limiter
                .begin(&SigninAccount::Email(&user.email), source)
                .await?;

            let outcome = check_password(password, &user.password_hash).await;

            attempt.settle(&outcome).await;
            outcome.is_ok().then_some(SsoIdentityProof::Password)
        }
        (None, None) => {
            return Err(
                Error::invalid("enter the code from the email or the account password").into(),
            );
        }
    };

    let Some(proof) = proof else {
        let attempts = state
            .iam
            .sso_link_requests
            .record_failure(&pending.id)
            .await?;

        if attempts >= MAX_ATTEMPTS {
            state.iam.sso_link_requests.delete(&pending.id).await?;

            return Err(Error::unauthorized("too many wrong attempts; sign in again").into());
        }

        return Err(Error::unauthorized("the code or password is not correct").into());
    };

    state.iam.sso_link_requests.delete(&pending.id).await?;

    let link = PendingLink {
        provider_id: pending.provider_id.0,
        subject: pending.subject,
        identity: pending.identity,
        by_email: proof == SsoIdentityProof::Email,
        user,
    };

    // 关联上的身份是一条新的登录路径：先看组织放不放行，账号开了多因素认证时先过第二步再关联。
    let provider = provision::enabled_provider(&state, &link.provider_id).await?;
    let mfa_on = mfa::enabled(&state, &link.user.id).await?;
    let method = link_method(&provider, mfa_on);
    let user_id = link.user.id.clone();

    let admission = mfa::admission(&state, &provider.org_id, &user_id, &method).await?;

    if admission == Admission::NeedsSso {
        let organization = state.iam.service.orgs.get(&provider.org_id).await?;

        return Err(mfa::sso_only(&organization.name).into());
    }

    if mfa_on || admission == Admission::NeedsMfa {
        let organization = state.iam.service.orgs.get(&provider.org_id).await?;

        let then = AfterVerification::Link {
            provider_id: link.provider_id,
            org_id: provider.org_id.0,
            subject: link.subject,
            identity: link.identity,
            by_email: link.by_email,
        };

        let answer = if mfa_on {
            mfa::second_step(&state, &user_id, then, request.remember).await
        } else {
            mfa::begin_enrollment(&state, &user_id, organization.name, then, request.remember).await
        };

        return Ok(answer?);
    }

    let (cookies, login) = complete_link(&state, &headers, link, request.remember, false).await?;

    Ok((
        cookies,
        [(CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "login": login })),
    )
        .into_response())
}

/// 本人确认过的关联请求。
pub(crate) struct PendingLink {
    pub provider_id: String,
    pub subject: String,
    pub identity: LinkedIdentity,
    /// 本人输入了邮件验证码；否则输入的是账号密码。
    pub by_email: bool,
    pub user: User,
}

/// 完成关联：绑定身份、入组、登录。`verified` 表示这次过了第二步验证。
pub(crate) async fn complete_link(
    state: &AppState,
    headers: &HeaderMap,
    link: PendingLink,
    remember: bool,
    verified: bool,
) -> Result<(SessionCookies, SessionView)> {
    let provider = provision::enabled_provider(state, &link.provider_id).await?;
    let identity = FederatedIdentity::from_snapshot(link.subject, link.identity);
    policy::ensure_identity_allowed(state, &provider, &identity).await?;

    let proof = if link.by_email {
        SsoIdentityProof::Email
    } else {
        SsoIdentityProof::Password
    };

    state
        .iam
        .sso_identities
        .link(
            SsoIdentity::new(
                provider.id.clone(),
                identity.subject.clone(),
                link.user.id.clone(),
                proof,
            ),
            link.by_email,
        )
        .await?;

    let login = provision::finish(state, &provider, link.user, &identity).await?;

    let cookies = login.sign_in(state, headers, remember, link_method(&provider, verified))?;
    Ok((cookies, login))
}

/// 经这个服务的关联登录是什么样：经 LDAP 的只核对了目录里的密码，不算单点登录。
fn link_method(provider: &SsoProvider, verified: bool) -> SignInMethod {
    SignInMethod {
        mfa: verified,
        sso_org: (provider.kind != SsoProviderKind::Ldap).then(|| provider.org_id.clone()),
    }
}

/// 浏览器手里的 `code` 对应的未过期请求。
async fn pending(state: &AppState, code: &str, now: TimestampMicros) -> Result<SsoLinkRequest> {
    let pending = state
        .iam
        .sso_link_requests
        .find_by_code(&digest(code.trim()), now)
        .await?
        .ok_or_else(|| Error::unauthorized("this confirmation has expired; sign in again"))?;

    if pending.attempts >= MAX_ATTEMPTS {
        state.iam.sso_link_requests.delete(&pending.id).await?;

        return Err(Error::unauthorized(
            "too many wrong attempts; sign in again",
        ));
    }

    Ok(pending)
}

fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// 验证码只有 6 位：带上请求 id 再取摘要，库里的摘要不能拿去对照别的请求。
fn code_digest(request_id: &Id, code: &str) -> String {
    digest(&format!("{}:{code}", request_id.0))
}

fn six_digits() -> String {
    use rand::TryRng as _;

    let mut bytes = [0_u8; 4];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("operating-system RNG");

    format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000)
}

/// `alice@example.com` → `a***@example.com`。
pub(super) fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) => {
            let first = local.chars().next().map(String::from).unwrap_or_default();
            format!("{first}***@{domain}")
        }
        None => "***".to_owned(),
    }
}

/// 发起页面的语言：`zh` 开头用中文，其余用英文。
fn email_language(locale: Option<&str>) -> &'static str {
    if locale.is_some_and(|locale| locale.to_ascii_lowercase().starts_with("zh")) {
        "zh-cn"
    } else {
        "en-us"
    }
}

/// 验证码邮件的主题与正文。文案来自 `i18n_language` 表，这里只填入验证码、有效期和来源。
fn link_email(
    copy: &LanguagePack,
    code: &str,
    minutes: i64,
    provider: &str,
    organization: &str,
) -> Result<(String, String)> {
    let minutes = minutes.to_string();

    let fills = [
        ("code", code),
        ("minutes", minutes.as_str()),
        ("provider", provider),
        ("organization", organization),
    ];

    Ok((
        copy.render(&format!("{EMAIL_KEY}.subject"), &fills)?,
        copy.render(&format!("{EMAIL_KEY}.body"), &fills)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_codes_are_six_digits_and_bound_to_their_request() {
        let code = six_digits();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));

        let first = Id::from_string("r-1".to_owned());
        let second = Id::from_string("r-2".to_owned());
        assert_ne!(
            code_digest(&first, "123456"),
            code_digest(&second, "123456")
        );
    }

    #[test]
    fn email_addresses_are_masked() {
        assert_eq!(mask_email("alice@example.com"), "a***@example.com");
        assert_eq!(mask_email("broken"), "***");
    }

    #[test]
    fn the_email_is_written_in_the_page_language() {
        use crate::shared::i18n_language_seed::seeded_pack;

        assert_eq!(email_language(Some("zh-CN")), "zh-cn");
        assert_eq!(email_language(None), "en-us");

        let (zh_subject, zh) = link_email(
            &seeded_pack(EMAIL_KEY, "zh-cn"),
            "042137",
            30,
            "Keycloak",
            "acme",
        )
        .unwrap();
        let (en_subject, en) = link_email(
            &seeded_pack(EMAIL_KEY, "en-us"),
            "042137",
            30,
            "Keycloak",
            "acme",
        )
        .unwrap();

        assert_eq!(zh_subject, "MoleSignal：验证码 042137");
        assert!(zh.contains("「Keycloak」（acme）"));
        assert!(zh.contains("请在 30 分钟内"));
        assert_eq!(en_subject, "MoleSignal: your verification code is 042137");
        assert!(en.contains("Keycloak (acme)"));
        assert!(en.contains("within 30 minutes"));
    }
}
