// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 登录会话的 access token 与 refresh token。
//!
//! 登录后浏览器拿到两个无状态 JWT：access token 只有 15 分钟，随每个请求发送；refresh token
//! 在这次登录结束前一直有效，只用来换新的 access token。服务端不保存任何 token：登录到何时
//! 结束、是否「记住我」、这次是怎么登录的（[`SignInMethod`]）都签在 token 里，校验只看签名、
//! 用途和过期时间。用户被停用或删除，或者账号让此前开始的登录作废时，每个请求都会经过的
//! [`IamService::admit_session`] 让它立即失效。

use std::collections::BTreeSet;

use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use super::{IamService, JWT_ISSUER, user_access_denial};
use crate::{
    domain::iam::{IamContext, IamScope},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// access token 的有效期。
const ACCESS_TOKEN_TTL_SECS: i64 = 15 * 60;

/// 进入平台组织 `_sys` 的登录最长一小时。
const SYSTEM_SESSION_MAX_SECS: i64 = 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TokenUse {
    /// 访问 API。
    Access,
    /// 只用来换新的 access token。
    Refresh,
}

/// JWT 载荷。
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    org_id: String,
    scope: IamScope,
    typ: TokenUse,
    iat: i64,
    exp: i64,
    iss: String,
    /// 登录结束的时间，即 refresh token 的 `exp`。
    session_exp: i64,
    remember: bool,
    /// 这次登录过了第二步验证。
    #[serde(default)]
    mfa: bool,
    /// 经这个组织的单点登录。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sso_org: Option<String>,
    /// 登录或最近一次重新确认的时间（微秒）。
    #[serde(default)]
    auth_micros: i64,
    /// 最近一次证明是本人的时间（微秒）。
    #[serde(default)]
    verified_micros: i64,
}

/// 这次登录是怎么完成的。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignInMethod {
    /// 过了多因素认证：第二步验证（身份验证器、Passkey 或恢复码），或者用 Passkey 登录。
    pub mfa: bool,
    /// 经这个组织的 OIDC / OAuth 2.0 / SAML 服务商登录；LDAP 只是核对目录里的密码，不算。
    pub sso_org: Option<Id>,
}

impl SignInMethod {
    /// 只过了一关：密码或 LDAP。
    pub fn single_factor() -> Self {
        Self::default()
    }

    /// 第一关之后又过了第二步验证。
    pub fn second_factor() -> Self {
        Self {
            mfa: true,
            sso_org: None,
        }
    }

    /// 用 Passkey 免密码登录：Passkey 要求用户验证，本身就算过了多因素认证。
    pub fn passkey() -> Self {
        Self::second_factor()
    }

    /// 经 `org_id` 的单点登录。
    pub fn single_sign_on(org_id: Id) -> Self {
        Self {
            mfa: false,
            sso_org: Some(org_id),
        }
    }
}

/// 一次登录：谁、在哪个组织、以什么作用域，到何时结束。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInSession {
    pub user_id: Id,
    pub org_id: Id,
    pub scope: IamScope,
    /// 登录结束的时间（Unix 秒）。
    pub expires_at: i64,
    /// 登录时选了「记住我」：关掉浏览器后会话仍在。
    pub remember: bool,
    pub method: SignInMethod,
    /// 登录或最近一次重新确认的时间。账号让在这之前开始的登录作废时，会话随之结束。
    pub auth_time: TimestampMicros,
    /// 最近一次证明是本人的时间：登录，或者当场过了第二因素。只是留在当前浏览器的重新确认不算：
    /// 拿到会话的人不能靠「退出其他设备」把这个时间往后推。
    pub verified_at: TimestampMicros,
}

/// 签给浏览器的一对 token。
#[derive(Debug, Clone)]
pub struct SessionTokens {
    pub access_token: String,
    /// access token 过期的时间（Unix 秒）。
    pub access_expires_at: i64,
    pub refresh_token: String,
}

impl IamService {
    /// 开始一次登录，持续配置的会话时长；进入平台组织的登录最长一小时。
    pub fn begin_session(
        &self,
        user_id: &Id,
        org_id: &Id,
        scope: IamScope,
        remember: bool,
        method: SignInMethod,
    ) -> SignInSession {
        let mut session = self.session_from(now_secs(), user_id, org_id, scope, remember);
        session.method = method;
        session
    }

    /// 同一次登录换到另一个组织：结束时间不变，进入平台组织时最多再留一小时。
    pub fn switch_session(
        &self,
        session: &SignInSession,
        org_id: &Id,
        scope: IamScope,
    ) -> SignInSession {
        SignInSession {
            user_id: session.user_id.clone(),
            org_id: org_id.clone(),
            scope,
            expires_at: within_scope(scope, now_secs(), session.expires_at),
            remember: session.remember,
            method: session.method.clone(),
            auth_time: session.auth_time,
            verified_at: session.verified_at,
        }
    }

    /// 当前浏览器的这次登录重新确认一次：账号让别的会话作废时，它带着新的确认时间留下来；
    /// `verified` 表示这次确认过了第二步验证。
    pub fn reconfirm_session(&self, session: &SignInSession, verified: bool) -> SignInSession {
        let mut session = session.clone();
        session.auth_time = TimestampMicros::now();

        if verified {
            session.verified_at = session.auth_time;
            session.method.mfa = true;
        }

        session
    }

    /// 已签发的会话还能不能用：账号停用、删除，或者让这次登录之前开始的会话作废了，都不能。
    pub async fn admit_session(&self, session: &SignInSession) -> Result<()> {
        let user = match self.users.get(&session.user_id).await {
            Ok(user) => user,
            Err(Error::NotFound(_)) => {
                return Err(Error::unauthorized("authenticated user no longer exists"));
            }
            Err(error) => return Err(error),
        };

        if let Some(reason) = user_access_denial(&user) {
            return Err(Error::unauthorized(reason));
        }

        if !started_after(session.auth_time, user.sessions_valid_after) {
            return Err(Error::unauthorized(
                "this sign-in ended when the account's security settings changed; sign in again",
            ));
        }

        Ok(())
    }

    /// 给一次登录签发 access token 与 refresh token。
    pub fn issue_session_tokens(&self, session: &SignInSession) -> Result<SessionTokens> {
        let (access_token, access_expires_at) = self.issue_access_token(session)?;
        let refresh_token =
            self.sign(session, TokenUse::Refresh, now_secs(), session.expires_at)?;

        Ok(SessionTokens {
            access_token,
            access_expires_at,
            refresh_token,
        })
    }

    /// 给一次登录签发新的 access token，不晚于登录结束。
    pub fn issue_access_token(&self, session: &SignInSession) -> Result<(String, i64)> {
        let now = now_secs();
        let expires_at = now
            .saturating_add(ACCESS_TOKEN_TTL_SECS)
            .min(session.expires_at);

        Ok((
            self.sign(session, TokenUse::Access, now, expires_at)?,
            expires_at,
        ))
    }

    /// 不能刷新的客户端（Flight SQL 的登录握手）用的 access token：一张用到登录结束。
    pub fn issue_token(&self, user_id: &Id, org_id: &Id) -> Result<String> {
        let now = now_secs();
        let session = self.session_from(now, user_id, org_id, IamScope::Organization, false);

        self.sign(&session, TokenUse::Access, now, session.expires_at)
    }

    /// 只在 `ttl_secs` 内有效的 access token，给不需要长会话的用途（例如交给无头浏览器渲染
    /// 一份报表）。有效期不会超过配置的会话时长。
    pub fn issue_short_lived_token(
        &self,
        user_id: &Id,
        org_id: &Id,
        ttl_secs: u64,
    ) -> Result<String> {
        let now = now_secs();
        let mut session = self.session_from(now, user_id, org_id, IamScope::Organization, false);

        session.expires_at = session
            .expires_at
            .min(now.saturating_add(i64::try_from(ttl_secs).unwrap_or(i64::MAX)));

        self.sign(&session, TokenUse::Access, now, session.expires_at)
    }

    /// 校验 access token，返回请求的身份和它所属的登录。
    pub fn verify_access_token(&self, token: &str) -> Result<(IamContext, SignInSession)> {
        let session = self.verify(token, TokenUse::Access)?;

        let context = IamContext {
            user_id: session.user_id.clone(),
            org_id: session.org_id.clone(),
            display_role: String::new(),
            roles: Vec::new(),
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: session.scope,
            permissions: BTreeSet::new(),
            policy_version: 0,
        };

        Ok((context, session))
    }

    /// 校验 access token，返回 [`IamContext`]。
    pub fn verify_token(&self, token: &str) -> Result<IamContext> {
        self.verify_access_token(token).map(|(context, _)| context)
    }

    /// 校验 refresh token，返回它所属的登录。
    pub fn verify_refresh_token(&self, token: &str) -> Result<SignInSession> {
        self.verify(token, TokenUse::Refresh)
    }

    /// 从 `now` 开始、持续配置的会话时长的一次登录。
    fn session_from(
        &self,
        now: i64,
        user_id: &Id,
        org_id: &Id,
        scope: IamScope,
        remember: bool,
    ) -> SignInSession {
        let ttl = i64::try_from(self.auth.session_ttl_secs).unwrap_or(i64::MAX);
        let started = TimestampMicros::now();

        SignInSession {
            user_id: user_id.clone(),
            org_id: org_id.clone(),
            scope,
            expires_at: within_scope(scope, now, now.saturating_add(ttl)),
            remember,
            method: SignInMethod::single_factor(),
            auth_time: started,
            verified_at: started,
        }
    }

    /// 用当前 primary secret 签名。
    fn sign(
        &self,
        session: &SignInSession,
        typ: TokenUse,
        issued_at: i64,
        expires_at: i64,
    ) -> Result<String> {
        let claims = Claims {
            sub: session.user_id.0.clone(),
            org_id: session.org_id.0.clone(),
            scope: session.scope,
            typ,
            iat: issued_at,
            exp: expires_at,
            iss: JWT_ISSUER.to_owned(),
            session_exp: session.expires_at,
            remember: session.remember,
            mfa: session.method.mfa,
            sso_org: session
                .method
                .sso_org
                .as_ref()
                .map(|org_id| org_id.0.clone()),
            auth_micros: session.auth_time.0,
            verified_micros: session.verified_at.0,
        };

        let secrets = self.jwt_secrets.read();
        let primary = secrets.first().expect("primary jwt secret");

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(primary),
        )
        .map_err(|error| Error::internal(format!("jwt encode: {error}")))
    }

    /// 依次用每个 secret 校验，命中 primary 或轮换宽限期内的任意一个即通过。
    fn verify(&self, token: &str, expected: TokenUse) -> Result<SignInSession> {
        let mut validation = Validation::default();
        validation.set_issuer(&[JWT_ISSUER]);
        let secrets = self.jwt_secrets.read();
        let mut last_err: Option<String> = None;

        for secret in secrets.iter() {
            match decode::<Claims>(token, &DecodingKey::from_secret(secret), &validation) {
                Ok(data) if data.claims.typ == expected => {
                    return Ok(SignInSession {
                        user_id: Id::from_string(data.claims.sub),
                        org_id: Id::from_string(data.claims.org_id),
                        scope: data.claims.scope,
                        expires_at: data.claims.session_exp,
                        remember: data.claims.remember,
                        method: SignInMethod {
                            mfa: data.claims.mfa,
                            sso_org: data.claims.sso_org.map(Id::from_string),
                        },
                        auth_time: TimestampMicros(data.claims.auth_micros),
                        verified_at: TimestampMicros(data.claims.verified_micros),
                    });
                }
                Ok(_) => {
                    return Err(Error::unauthorized(match expected {
                        TokenUse::Access => "a refresh token cannot authenticate requests",
                        TokenUse::Refresh => "an access token cannot renew a session",
                    }));
                }
                Err(error) => last_err = Some(error.to_string()),
            }
        }

        Err(Error::unauthorized(format!(
            "jwt verify failed against all {} active secret(s): {}",
            secrets.len(),
            last_err.unwrap_or_else(|| "no secrets configured".into())
        )))
    }
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

/// 让此前开始的登录作废时写进账号的时间。当前浏览器随后重新确认的会话晚于它，留了下来。
pub fn sessions_end_now() -> TimestampMicros {
    TimestampMicros::now()
}

/// 在 `auth_time` 开始的登录，在账号的作废时间 `valid_after` 之后还能不能用。
fn started_after(auth_time: TimestampMicros, valid_after: TimestampMicros) -> bool {
    auth_time >= valid_after
}

/// 进入平台组织的登录从现在起最多一小时，其余作用域保持 `expires_at`。
fn within_scope(scope: IamScope, now: i64, expires_at: i64) -> i64 {
    if scope == IamScope::System {
        expires_at.min(now.saturating_add(SYSTEM_SESSION_MAX_SECS))
    } else {
        expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::iam::test_support::{iam_service, token_lifetime_secs};

    fn ids() -> (Id, Id) {
        (Id::from_string("user-1"), Id::from_string("org-1"))
    }

    #[test]
    fn a_sign_in_gets_a_short_access_token_and_a_refresh_token_for_the_whole_session() {
        let iam = iam_service(86_400);
        let (user_id, org_id) = ids();
        let session = iam.begin_session(
            &user_id,
            &org_id,
            IamScope::Organization,
            true,
            SignInMethod::single_factor(),
        );
        let tokens = iam.issue_session_tokens(&session).unwrap();

        assert_eq!(
            token_lifetime_secs(&tokens.access_token),
            ACCESS_TOKEN_TTL_SECS as u64
        );
        // The session began a moment before the tokens were signed.
        assert!((86_399..=86_400).contains(&token_lifetime_secs(&tokens.refresh_token)));

        let (context, from_access) = iam.verify_access_token(&tokens.access_token).unwrap();
        assert_eq!(context.user_id, user_id);
        assert_eq!(context.org_id, org_id);
        assert_eq!(from_access, session);
        assert_eq!(
            iam.verify_refresh_token(&tokens.refresh_token).unwrap(),
            session
        );
    }

    #[test]
    fn each_token_only_does_its_own_job() {
        let iam = iam_service(86_400);
        let (user_id, org_id) = ids();
        let session = iam.begin_session(
            &user_id,
            &org_id,
            IamScope::Organization,
            false,
            SignInMethod::single_factor(),
        );
        let tokens = iam.issue_session_tokens(&session).unwrap();

        assert!(matches!(
            iam.verify_token(&tokens.refresh_token),
            Err(Error::Unauthorized(_))
        ));
        assert!(matches!(
            iam.verify_refresh_token(&tokens.access_token),
            Err(Error::Unauthorized(_))
        ));
    }

    #[test]
    fn an_access_token_never_outlives_its_session() {
        let iam = iam_service(120);
        let (user_id, org_id) = ids();
        let session = iam.begin_session(
            &user_id,
            &org_id,
            IamScope::Organization,
            false,
            SignInMethod::single_factor(),
        );
        let (access, expires_at) = iam.issue_access_token(&session).unwrap();

        assert_eq!(expires_at, session.expires_at);
        assert!((119..=120).contains(&token_lifetime_secs(&access)));
    }

    #[test]
    fn the_platform_organization_is_entered_for_an_hour_at_most() {
        let iam = iam_service(86_400);
        let (user_id, org_id) = ids();
        let system_org_id = Id::from_string("_sys");

        let signed_in = iam.begin_session(
            &user_id,
            &system_org_id,
            IamScope::System,
            false,
            SignInMethod::single_factor(),
        );
        assert!(signed_in.expires_at <= now_secs() + SYSTEM_SESSION_MAX_SECS);

        let tenant = iam.begin_session(
            &user_id,
            &org_id,
            IamScope::Organization,
            true,
            SignInMethod::single_factor(),
        );
        let entered = iam.switch_session(&tenant, &system_org_id, IamScope::System);
        assert!(entered.expires_at <= now_secs() + SYSTEM_SESSION_MAX_SECS);
        assert!(entered.remember);

        // Leaving it does not give the hour back: the sign-in ends when it was going to.
        let left = iam.switch_session(&entered, &org_id, IamScope::Organization);
        assert_eq!(left.expires_at, entered.expires_at);
        assert_eq!(left.org_id, org_id);
    }

    #[test]
    fn switching_organization_keeps_the_end_of_the_sign_in() {
        let iam = iam_service(86_400);
        let (user_id, org_id) = ids();
        let session = iam.begin_session(
            &user_id,
            &org_id,
            IamScope::Organization,
            false,
            SignInMethod::single_factor(),
        );
        let other = iam.switch_session(&session, &Id::from_string("org-2"), IamScope::Organization);

        assert_eq!(other.expires_at, session.expires_at);
        assert_eq!(other.user_id, user_id);
        assert!(!other.remember);
    }

    #[test]
    fn the_way_of_signing_in_travels_with_the_tokens_and_survives_a_switch() {
        let iam = iam_service(86_400);
        let (user_id, org_id) = ids();

        let method = SignInMethod {
            mfa: true,
            sso_org: Some(org_id.clone()),
        };

        let session = iam.begin_session(
            &user_id,
            &org_id,
            IamScope::Organization,
            false,
            method.clone(),
        );
        let tokens = iam.issue_session_tokens(&session).unwrap();

        let (_, from_access) = iam.verify_access_token(&tokens.access_token).unwrap();
        assert_eq!(from_access.method, method);
        assert_eq!(from_access.auth_time, session.auth_time);
        assert_eq!(
            iam.verify_refresh_token(&tokens.refresh_token)
                .unwrap()
                .method,
            method
        );

        let switched =
            iam.switch_session(&session, &Id::from_string("org-2"), IamScope::Organization);
        assert_eq!(switched.method, method);
        assert_eq!(switched.auth_time, session.auth_time);
    }

    #[test]
    fn reconfirming_moves_the_time_forward_and_keeps_a_verification() {
        let iam = iam_service(86_400);
        let (user_id, org_id) = ids();

        let mut session = iam.begin_session(
            &user_id,
            &org_id,
            IamScope::Organization,
            false,
            SignInMethod::single_factor(),
        );
        session.auth_time = TimestampMicros(session.auth_time.0 - 600_000_000);
        session.verified_at = session.auth_time;

        // Staying signed in after signing out the other devices proves nothing.
        let stayed = iam.reconfirm_session(&session, false);
        assert!(stayed.auth_time.0 >= session.auth_time.0 + 600_000_000);
        assert_eq!(stayed.verified_at, session.verified_at);
        assert!(!stayed.method.mfa);

        let verified = iam.reconfirm_session(&session, true);
        assert!(verified.method.mfa);
        assert!(verified.auth_time.0 >= session.auth_time.0 + 600_000_000);
        assert_eq!(verified.verified_at, verified.auth_time);
        assert_eq!(verified.expires_at, session.expires_at);

        let kept = iam.reconfirm_session(&verified, false);
        assert!(kept.method.mfa);
    }

    #[test]
    fn a_sign_in_ends_when_the_account_ends_the_sign_ins_before_it() {
        assert!(started_after(TimestampMicros(100), TimestampMicros(0)));
        assert!(started_after(TimestampMicros(100), TimestampMicros(100)));
        assert!(!started_after(TimestampMicros(99), TimestampMicros(100)));

        // The browser that changed the setting is reconfirmed after this point and stays.
        let end = sessions_end_now();
        assert!(started_after(TimestampMicros::now(), end));
    }

    #[test]
    fn tokens_signed_before_the_way_of_signing_in_was_recorded_read_as_single_factor() {
        #[derive(Serialize)]
        struct EarlierClaims<'a> {
            sub: &'a str,
            org_id: &'a str,
            scope: IamScope,
            typ: TokenUse,
            iat: i64,
            exp: i64,
            iss: &'a str,
            session_exp: i64,
            remember: bool,
        }

        let iam = iam_service(86_400);
        let now = now_secs();

        let claims = EarlierClaims {
            sub: "user-1",
            org_id: "org-1",
            scope: IamScope::Organization,
            typ: TokenUse::Access,
            iat: now,
            exp: now + 60,
            iss: JWT_ISSUER,
            session_exp: now + 60,
            remember: false,
        };

        let secret = iam.jwt_secrets.read().first().cloned().unwrap();
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(&secret),
        )
        .unwrap();

        let (_, session) = iam.verify_access_token(&token).unwrap();
        assert_eq!(session.method, SignInMethod::single_factor());
        assert_eq!(session.auth_time, TimestampMicros(0));
    }
}
