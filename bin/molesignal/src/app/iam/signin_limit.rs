// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 密码登录限速，挡住撞库和暴力破解，又不让别人靠输错密码把账号锁在外面。
//!
//! 每个核对账号密码的入口都这样走：
//!
//! ```text
//! let attempt = limiter.begin(&account, source).await?;   // 拒绝时不核对密码
//! let outcome = authenticate(...).await;
//! attempt.settle(&outcome).await;
//! ```
//!
//! 不存在的账号一样计数，回答也一样。计数的规则在 [`crate::infra::signin`]，默认值和设置页的参数
//! 在 [`SigninLimits`]。Passkey 猜不出来，OIDC、OAuth 2.0、SAML 不经过这里的密码，都不计数也不受限。
//!
//! 公开分享链接的访问密码也走这里：一个链接算一个账号。来源的次数和登录算在一起，换着目标猜不会多出
//! 名额。

use std::{net::IpAddr, sync::Arc};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    domain::iam::{InstanceSettingsRepository, SigninLimits, UserRepository},
    infra::{
        persistence::repositories::audit_events::{AuditEvent, AuditEventRepository},
        signin::{AttemptKeys, Quota, Settlement, SigninAttemptRepository, Tripped},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 发信限额：每个邮箱每小时 5 封，每个来源每小时 20 次。注册和找回密码共用。
const MAIL_QUOTA: Quota = Quota {
    per_account: 5,
    per_source: 20,
    window_micros: 60 * 60 * 1_000_000,
};

/// 这次尝试的账号。
#[derive(Debug, Clone, Copy)]
pub enum SigninAccount<'a> {
    /// 账号密码：账号的邮箱。
    Email(&'a str),
    /// LDAP：服务商和目录里的用户名。
    Directory {
        provider_id: &'a str,
        username: &'a str,
    },
    /// 公开分享链接的访问密码。按分享 ID 计数，`org_id` 只决定审计记在哪个组织。
    ResourceShare { share_id: &'a Id, org_id: &'a Id },
}

/// 尝试太多次，这次不核对密码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SigninRefused {
    pub retry_after_secs: u64,
}

impl SigninRefused {
    fn new(until: TimestampMicros, now: TimestampMicros) -> Self {
        let remaining = until.0.saturating_sub(now.0).max(0) as u64;

        Self {
            retry_after_secs: remaining.div_ceil(1_000_000).max(1),
        }
    }

    pub fn message(&self) -> String {
        let seconds = self.retry_after_secs;

        if seconds < 60 {
            format!("too many failed password attempts; try again in {seconds} seconds")
        } else {
            let minutes = seconds.div_ceil(60);
            format!("too many failed password attempts; try again in {minutes} minutes")
        }
    }
}

/// 核对账号密码的入口的错误。
#[derive(Debug)]
pub enum SigninError {
    Refused(SigninRefused),
    Failed(Error),
}

impl From<Error> for SigninError {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}

impl From<SigninRefused> for SigninError {
    fn from(refused: SigninRefused) -> Self {
        Self::Refused(refused)
    }
}

pub struct SigninLimiter {
    attempts: Arc<dyn SigninAttemptRepository>,
    settings: Arc<dyn InstanceSettingsRepository>,
    users: Arc<dyn UserRepository>,
    audit_events: Arc<dyn AuditEventRepository>,
    system_org_id: Id,
}

/// 记上了、还在核对密码的一次尝试。
pub struct SigninAttempt<'a> {
    limiter: &'a SigninLimiter,
    keys: AttemptKeys,
    limits: SigninLimits,
    lockout: Lockout,
}

/// 刚好到限时要记进审计的事。
#[derive(Debug)]
struct Lockout {
    target: LockoutTarget,
    source: Option<IpAddr>,
    tripped: Tripped,
}

/// 到限的是谁。
#[derive(Debug)]
enum LockoutTarget {
    /// 账号密码的邮箱：账号存在才写进审计，不存在时可能是别人输错的密码。
    Email(String),
    Directory {
        provider_id: String,
    },
    ResourceShare {
        share_id: Id,
        org_id: Id,
    },
}

impl From<&SigninAccount<'_>> for LockoutTarget {
    fn from(account: &SigninAccount<'_>) -> Self {
        match *account {
            SigninAccount::Email(email) => Self::Email(email.to_owned()),
            SigninAccount::Directory { provider_id, .. } => Self::Directory {
                provider_id: provider_id.to_owned(),
            },
            SigninAccount::ResourceShare { share_id, org_id } => Self::ResourceShare {
                share_id: share_id.clone(),
                org_id: org_id.clone(),
            },
        }
    }
}

impl SigninLimiter {
    pub fn new(
        attempts: Arc<dyn SigninAttemptRepository>,
        settings: Arc<dyn InstanceSettingsRepository>,
        users: Arc<dyn UserRepository>,
        audit_events: Arc<dyn AuditEventRepository>,
        system_org_id: Id,
    ) -> Self {
        Self {
            attempts,
            settings,
            users,
            audit_events,
            system_org_id,
        }
    }

    /// 开始一次尝试。拒绝时不用核对密码。
    pub async fn begin(
        &self,
        account: &SigninAccount<'_>,
        source: Option<IpAddr>,
    ) -> Result<SigninAttempt<'_>, SigninError> {
        let limits = self.settings.get().await?.signin_limits;
        let now = TimestampMicros::now();

        let keys = AttemptKeys {
            account: account_key(account),
            source: source.map(source_key),
        };

        // 没有要记的就不写库，也不排队等行锁。
        if let Some(refusal) = self.attempts.refusal(&keys, &limits, now).await?
            && (!refusal.paced || keys.source.is_none())
        {
            return Err(SigninRefused::new(refusal.until, now).into());
        }

        let begun = self.attempts.begin(&keys, &limits, now).await?;

        let lockout = Lockout {
            target: account.into(),
            source,
            tripped: begun.tripped,
        };

        if let Some(refusal) = begun.refusal {
            self.record_lockout(&lockout).await;

            return Err(SigninRefused::new(refusal.until, now).into());
        }

        Ok(SigninAttempt {
            limiter: self,
            keys,
            limits,
            lockout,
        })
    }

    /// 占一次发信限额，用完了回答 `false`。
    pub async fn take_mail_quota(&self, email: &str, source: Option<IpAddr>) -> Result<bool> {
        let keys = AttemptKeys {
            account: digest(&["mail", &normalized(email)]),
            source: source.map(|ip| digest(&["mail", &source_bucket(ip)])),
        };

        self.attempts
            .take_quota(&keys, &MAIL_QUOTA, TimestampMicros::now())
            .await
    }

    /// 本人重置了密码：解开这个账号，`source` 不再受账号限速。
    pub async fn forgive(&self, email: &str, source: Option<IpAddr>) -> Result<()> {
        let keys = AttemptKeys {
            account: account_key(&SigninAccount::Email(email)),
            source: source.map(source_key),
        };

        self.attempts.forgive(&keys, TimestampMicros::now()).await
    }

    async fn record_lockout(&self, lockout: &Lockout) {
        let tripped = lockout.tripped;

        let dimensions: Vec<&str> = [
            (tripped.pair, "account_source"),
            (tripped.source, "source"),
            (tripped.account, "account"),
        ]
        .into_iter()
        .filter_map(|(tripped, name)| tripped.then_some(name))
        .collect();

        if dimensions.is_empty() {
            return;
        }

        let event = |org_id: &Id, action: &str, target: Option<(&str, &str)>, payload| AuditEvent {
            id: Id::new(),
            org_id: org_id.clone(),
            actor_kind: "anonymous".into(),
            actor_id: String::new(),
            action: action.into(),
            target_kind: target.map(|(kind, _)| kind.to_owned()),
            target_id: target.map(|(_, id)| id.to_owned()),
            ip: lockout.source.map(|ip| ip.to_string()),
            user_agent: None,
            payload,
            ts: TimestampMicros::now(),
        };

        match &lockout.target {
            LockoutTarget::Email(email) => {
                let user = self.users.get_by_email(email).await.ok();
                let target = user.as_ref().map(|user| ("user", user.id.as_str()));

                self.record_audit(event(
                    &self.system_org_id,
                    "signin.lockout",
                    target,
                    json!({"dimensions": dimensions}),
                ))
                .await;
            }
            LockoutTarget::Directory { provider_id } => {
                self.record_audit(event(
                    &self.system_org_id,
                    "signin.lockout",
                    None,
                    json!({"dimensions": dimensions, "providerId": provider_id}),
                ))
                .await;
            }
            LockoutTarget::ResourceShare { share_id, org_id } => {
                self.record_audit(event(
                    org_id,
                    "resource_share.lockout",
                    Some(("resource_share", share_id.as_str())),
                    json!({"dimensions": dimensions}),
                ))
                .await;

                // 来源锁挡的还有这个地址的密码登录，平台审计里也要看得到。
                if tripped.source {
                    self.record_audit(event(
                        &self.system_org_id,
                        "signin.lockout",
                        None,
                        json!({"dimensions": ["source"]}),
                    ))
                    .await;
                }
            }
        }
    }

    async fn record_audit(&self, event: AuditEvent) {
        if let Err(error) = self.audit_events.record(event).await {
            tracing::warn!(%error, "sign-in lockout could not be audited");
        }
    }
}

impl SigninAttempt<'_> {
    /// 核对完了。失败的次数已经记上；密码对了或者没核对出结果，就退回这一次。
    pub async fn settle<T>(self, outcome: &Result<T>) {
        let settlement = match outcome {
            Err(Error::Unauthorized(_)) => {
                self.limiter.record_lockout(&self.lockout).await;
                return;
            }
            // 403 只在密码对了之后才回答，例如账号被停用。
            Ok(_) | Err(Error::Forbidden(_)) => Settlement::Succeeded,
            Err(_) => Settlement::Inconclusive,
        };

        let settled = self
            .limiter
            .attempts
            .settle(&self.keys, &self.limits, settlement, TimestampMicros::now())
            .await;

        // 记不下来不该挡住已经对上的密码。
        if let Err(error) = settled {
            tracing::warn!(%error, "sign-in attempt could not be settled");
        }
    }
}

fn digest(parts: &[&str]) -> String {
    hex::encode(Sha256::digest(parts.join("\0").as_bytes()))
}

fn normalized(identifier: &str) -> String {
    identifier.trim().to_lowercase()
}

fn account_key(account: &SigninAccount<'_>) -> String {
    match *account {
        SigninAccount::Email(email) => digest(&["email", &normalized(email)]),
        SigninAccount::Directory {
            provider_id,
            username,
        } => digest(&["directory", provider_id, &normalized(username)]),
        SigninAccount::ResourceShare { share_id, .. } => digest(&["share", share_id.as_str()]),
    }
}

fn source_key(ip: IpAddr) -> String {
    digest(&["source", &source_bucket(ip)])
}

/// 计数用的来源：IPv4 按单个地址，IPv6 按 /64，否则换个地址就绕过了。
fn source_bucket(ip: IpAddr) -> String {
    match ip.to_canonical() {
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) => {
            let [a, b, c, d, ..] = ip.segments();

            format!("{a:x}:{b:x}:{c:x}:{d:x}::/64")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_account_counts_once_however_it_is_typed() {
        assert_eq!(
            account_key(&SigninAccount::Email("Alice@Example.com ")),
            account_key(&SigninAccount::Email("alice@example.com")),
        );

        assert_ne!(
            account_key(&SigninAccount::Email("alice@example.com")),
            account_key(&SigninAccount::Email("bob@example.com")),
        );
    }

    #[test]
    fn a_directory_account_is_kept_apart_per_provider_and_from_passwords() {
        let of = |provider_id, username| {
            account_key(&SigninAccount::Directory {
                provider_id,
                username,
            })
        };

        assert_eq!(of("p1", "Alice"), of("p1", " alice"));
        assert_ne!(of("p1", "alice"), of("p2", "alice"));
        assert_ne!(
            of("p1", "alice"),
            account_key(&SigninAccount::Email("alice"))
        );

        // 分隔符不能让两个不同的组合撞在一起。
        assert_ne!(of("p1", "2alice"), of("p12", "alice"));
    }

    #[test]
    fn a_share_is_counted_by_its_id_alone_and_kept_apart_from_accounts() {
        fn of(share_id: &str, org_id: &str) -> String {
            account_key(&SigninAccount::ResourceShare {
                share_id: &Id::from_string(share_id),
                org_id: &Id::from_string(org_id),
            })
        }

        assert_eq!(of("Share-A", "o1"), of("Share-A", "o2"));
        assert_ne!(of("Share-A", "o1"), of("Share-B", "o1"));

        // ID 不是人输入的，大小写不同就是两个分享。
        assert_ne!(of("Share-A", "o1"), of("share-a", "o1"));

        assert_ne!(
            of("Share-A", "o1"),
            account_key(&SigninAccount::Email("Share-A"))
        );
    }

    #[test]
    fn stored_keys_never_hold_what_was_typed() {
        let key = account_key(&SigninAccount::Email("hunter2"));

        assert_eq!(key.len(), 64);
        assert!(!key.contains("hunter2"));
    }

    #[test]
    fn an_ipv6_source_is_counted_per_64() {
        let same_network = source_key("2001:db8:1:2:aaaa::1".parse().unwrap());

        assert_eq!(
            same_network,
            source_key("2001:db8:1:2:bbbb::2".parse().unwrap())
        );

        assert_ne!(same_network, source_key("2001:db8:1:3::1".parse().unwrap()));
    }

    #[test]
    fn an_ipv4_mapped_address_is_the_ipv4_address() {
        assert_eq!(
            source_key("::ffff:203.0.113.7".parse().unwrap()),
            source_key("203.0.113.7".parse().unwrap()),
        );

        assert_ne!(
            source_key("203.0.113.7".parse().unwrap()),
            source_key("203.0.113.8".parse().unwrap()),
        );
    }

    #[test]
    fn the_mail_quota_is_kept_apart_from_failed_passwords() {
        let email = digest(&["mail", &normalized("alice@example.com")]);

        assert_ne!(
            email,
            account_key(&SigninAccount::Email("alice@example.com"))
        );
    }

    #[test]
    fn the_wait_is_said_in_seconds_or_rounded_up_to_minutes() {
        let at = TimestampMicros(1_000_000_000);

        let short = SigninRefused::new(TimestampMicros(at.0 + 42_500_000), at);
        assert_eq!(short.retry_after_secs, 43);
        assert!(short.message().ends_with("try again in 43 seconds"));

        let long = SigninRefused::new(TimestampMicros(at.0 + 14 * 60_000_000 + 1), at);
        assert_eq!(long.retry_after_secs, 14 * 60 + 1);
        assert!(long.message().ends_with("try again in 15 minutes"));

        // 刚好到期也至少等一秒。
        assert_eq!(SigninRefused::new(at, at).retry_after_secs, 1);
    }

    /// 尝试到限的审计。次数记在内存里，走真实的规则。
    mod lockout_audit {
        use super::*;
        use crate::{
            app::iam::test_support::{AuditLog, SYSTEM_ORG, signin_limiter},
            domain::iam::{SigninLimits, User, UserStatus},
        };

        fn source() -> Option<IpAddr> {
            Some(IpAddr::from([8, 8, 8, 8]))
        }

        fn wrong() -> Result<()> {
            Err(Error::unauthorized("invalid credentials"))
        }

        /// 输错一次；已经到限、被拒绝的不算。
        async fn fail(limiter: &SigninLimiter, account: &SigninAccount<'_>) {
            if let Ok(attempt) = limiter.begin(account, source()).await {
                attempt.settle(&wrong()).await;
            }
        }

        fn share<'a>(share_id: &'a Id, org_id: &'a Id) -> SigninAccount<'a> {
            SigninAccount::ResourceShare { share_id, org_id }
        }

        fn one_event(audit: &AuditLog) -> AuditEvent {
            let events = audit.events();
            assert_eq!(events.len(), 1, "{events:?}");

            events.into_iter().next().unwrap()
        }

        #[tokio::test]
        async fn a_share_is_audited_on_itself_in_its_own_organization() {
            let (limiter, audit) = signin_limiter(SigninLimits::default(), Vec::new());
            let (share_id, org_id) = (Id::from_string("share-1"), Id::from_string("org-1"));

            for _ in 0..4 {
                fail(&limiter, &share(&share_id, &org_id)).await;
            }

            assert!(audit.events().is_empty());

            fail(&limiter, &share(&share_id, &org_id)).await;

            let event = one_event(&audit);

            assert_eq!(event.action, "resource_share.lockout");
            assert_eq!(event.org_id, org_id);
            assert_eq!(event.target_kind.as_deref(), Some("resource_share"));
            assert_eq!(event.target_id.as_deref(), Some("share-1"));
            assert_eq!(event.ip.as_deref(), Some("8.8.8.8"));
            assert_eq!(event.payload, json!({"dimensions": ["account_source"]}));

            // 被拒绝的请求不再记。
            fail(&limiter, &share(&share_id, &org_id)).await;
            assert_eq!(audit.events().len(), 1);
        }

        #[tokio::test]
        async fn the_right_password_on_the_last_attempt_is_not_a_lockout() {
            let (limiter, audit) = signin_limiter(SigninLimits::default(), Vec::new());
            let (share_id, org_id) = (Id::from_string("share-1"), Id::from_string("org-1"));

            for _ in 0..4 {
                fail(&limiter, &share(&share_id, &org_id)).await;
            }

            let Ok(attempt) = limiter.begin(&share(&share_id, &org_id), source()).await else {
                panic!("the fifth attempt is still checked");
            };

            attempt.settle(&Ok(())).await;

            assert!(audit.events().is_empty());
        }

        #[tokio::test]
        async fn locking_a_source_through_a_share_also_reaches_the_platform_audit() {
            let limits = SigninLimits {
                source_failures: 3,
                ..SigninLimits::default()
            };

            let (limiter, audit) = signin_limiter(limits, Vec::new());
            let (share_id, org_id) = (Id::from_string("share-1"), Id::from_string("org-1"));

            for _ in 0..3 {
                fail(&limiter, &share(&share_id, &org_id)).await;
            }

            let events = audit.events();
            assert_eq!(events.len(), 2, "{events:?}");

            assert_eq!(events[0].action, "resource_share.lockout");
            assert_eq!(events[0].org_id, org_id);
            assert_eq!(events[0].payload, json!({"dimensions": ["source"]}));

            assert_eq!(events[1].action, "signin.lockout");
            assert_eq!(events[1].org_id.as_str(), SYSTEM_ORG);
            assert_eq!(events[1].target_id, None);
            assert_eq!(events[1].ip.as_deref(), Some("8.8.8.8"));
            assert_eq!(events[1].payload, json!({"dimensions": ["source"]}));
        }

        #[tokio::test]
        async fn failures_on_a_share_count_against_the_source_for_signing_in() {
            let limits = SigninLimits {
                source_failures: 3,
                ..SigninLimits::default()
            };

            let (limiter, _) = signin_limiter(limits, Vec::new());
            let (share_id, org_id) = (Id::from_string("share-1"), Id::from_string("org-1"));

            for _ in 0..3 {
                fail(&limiter, &share(&share_id, &org_id)).await;
            }

            let signing_in = limiter
                .begin(&SigninAccount::Email("alice@example.com"), source())
                .await;

            assert!(matches!(signing_in, Err(SigninError::Refused(_))));
        }

        #[tokio::test]
        async fn an_account_is_audited_on_the_platform_and_named_only_if_it_exists() {
            let alice = User {
                id: Id::from_string("user-1"),
                email: "alice@example.com".into(),
                display_name: "Alice".into(),
                avatar_url: None,
                bio: String::new(),
                password_hash: String::new(),
                disabled: false,
                status: UserStatus::Active,
                created_at: TimestampMicros(0),
                sessions_valid_after: TimestampMicros(0),
            };

            let (limiter, audit) = signin_limiter(SigninLimits::default(), vec![alice]);

            for _ in 0..5 {
                fail(&limiter, &SigninAccount::Email("alice@example.com")).await;
            }

            let event = one_event(&audit);

            assert_eq!(event.action, "signin.lockout");
            assert_eq!(event.org_id.as_str(), SYSTEM_ORG);
            assert_eq!(event.target_kind.as_deref(), Some("user"));
            assert_eq!(event.target_id.as_deref(), Some("user-1"));
            assert_eq!(event.payload, json!({"dimensions": ["account_source"]}));

            let (limiter, audit) = signin_limiter(SigninLimits::default(), Vec::new());

            for _ in 0..5 {
                fail(&limiter, &SigninAccount::Email("nobody@example.com")).await;
            }

            let event = one_event(&audit);

            assert_eq!(event.target_kind, None);
            assert_eq!(event.target_id, None);
        }

        #[tokio::test]
        async fn a_directory_account_is_audited_with_its_provider() {
            let (limiter, audit) = signin_limiter(SigninLimits::default(), Vec::new());

            let account = SigninAccount::Directory {
                provider_id: "ldap-1",
                username: "alice",
            };

            for _ in 0..5 {
                fail(&limiter, &account).await;
            }

            let event = one_event(&audit);

            assert_eq!(event.action, "signin.lockout");
            assert_eq!(event.target_id, None);

            assert_eq!(
                event.payload,
                json!({"dimensions": ["account_source"], "providerId": "ldap-1"})
            );
        }
    }
}
