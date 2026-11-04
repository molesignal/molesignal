// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::{Arc, LazyLock};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use parking_lot::RwLock;
use tokio::sync::Semaphore;

use crate::{
    config::AuthSettings,
    domain::iam::{
        IamMembershipRepository, OrganizationRepository, User, UserRepository, UserStatus,
        sessions_always_valid,
    },
    shared::{Error, Result, ids::Id},
};

const JWT_ISSUER: &str = "molesignal";
/// 同时算几个 Argon2：每个约 19 MiB 内存、几十毫秒 CPU。
const MAX_CONCURRENT_PASSWORD_HASHES: usize = 8;

static PASSWORD_HASH_PERMITS: Semaphore = Semaphore::const_new(MAX_CONCURRENT_PASSWORD_HASHES);

/// 账号不存在或没有密码时拿来核对的哈希：参数和真的一样，耗时也就一样。
static DECOY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    hash_password("decoy password").expect("operating-system random source for the salt")
});

pub mod access;
mod jwt_secrets;
pub mod member;
pub mod mfa;
mod navigation;
mod scoped_token;
mod session;
pub mod signin_limit;
pub mod signin_policy;
#[cfg(test)]
pub(crate) mod test_support;
pub use access::{
    IamAccessRequest, IamAccessService, IamAttributes, IamCapabilitySnapshot, IamContextEnricher,
    IamDecision, IamDecisionReason, IamSubject, IamTarget, validate_iam_conditions,
};
pub use jwt_secrets::JwtSecretSource;
pub use navigation::resolve_route_access;
pub use session::{SessionTokens, SignInMethod, SignInSession, sessions_end_now};

pub use crate::domain::iam::IamContext;

pub struct IamService {
    pub users: Arc<dyn UserRepository>,
    pub orgs: Arc<dyn OrganizationRepository>,
    pub iam_memberships: Arc<dyn IamMembershipRepository>,
    auth: AuthSettings,
    /// JWT 签名 secret 集合（auth-hardening）：
    /// `[0]` 是 primary（用来签新 token）；其余是 24h grace 内的 retired secret
    /// （仅参与 verify）。启动时由 `bootstrap_or_load_jwt_secret` 注入，之后从数据库重读，
    /// 见 [`jwt_secrets`]。
    jwt_secrets: Arc<RwLock<Vec<Vec<u8>>>>,
    jwt_secret_reads: Option<jwt_secrets::JwtSecretReads>,
}

impl IamService {
    pub fn new(
        users: Arc<dyn UserRepository>,
        orgs: Arc<dyn OrganizationRepository>,
        iam_memberships: Arc<dyn IamMembershipRepository>,
        auth: AuthSettings,
        jwt_secrets: Vec<Vec<u8>>,
    ) -> Self {
        assert!(
            !jwt_secrets.is_empty(),
            "IamService requires at least one JWT signing secret (primary)"
        );

        Self {
            users,
            orgs,
            iam_memberships,
            auth,
            jwt_secrets: Arc::new(RwLock::new(jwt_secrets)),
            jwt_secret_reads: None,
        }
    }

    /// 用户登录：邮箱 + 密码 → User。不知道密码的人查不出账号在不在、停没停用：账号不存在时也算一遍
    /// Argon2，先验密码，对了才说账号的状态。
    pub async fn authenticate(&self, email: &str, password: &str) -> Result<User> {
        let user = match self.users.get_by_email(email).await {
            Ok(user) => user,
            Err(Error::NotFound(_)) => {
                check_password(password, "").await?;

                return Err(Error::unauthorized("invalid credentials"));
            }
            Err(error) => return Err(error),
        };

        check_password(password, &user.password_hash).await?;

        if let Some(reason) = user_access_denial(&user) {
            return Err(Error::forbidden(reason));
        }

        Ok(user)
    }

    /// Re-check the persisted user state after validating an already-issued
    /// credential. This makes disabling, rejecting, or deleting a user take
    /// effect immediately instead of waiting for JWT/API-token expiry.
    pub async fn ensure_user_access(&self, id: &Id) -> Result<()> {
        let user = match self.users.get(id).await {
            Ok(user) => user,
            Err(Error::NotFound(_)) => {
                return Err(Error::unauthorized("authenticated user no longer exists"));
            }
            Err(error) => return Err(error),
        };

        if let Some(reason) = user_access_denial(&user) {
            return Err(Error::unauthorized(reason));
        }

        Ok(())
    }

    /// Re-check the organization behind an already-issued credential so a
    /// platform suspension takes effect without waiting for token expiry.
    pub async fn ensure_organization_access(&self, id: &Id) -> Result<()> {
        let organization = match self.orgs.get(id).await {
            Ok(organization) => organization,
            Err(Error::NotFound(_)) => {
                return Err(Error::unauthorized(
                    "authenticated organization no longer exists",
                ));
            }
            Err(error) => return Err(error),
        };

        organization.ensure_enabled()
    }

    pub async fn current_user(&self, id: &Id) -> Result<User> {
        self.users.get(id).await
    }

    /// 创建用户：自动哈希明文密码。
    pub async fn create_user(
        &self,
        email: String,
        display_name: String,
        password_plain: &str,
    ) -> Result<User> {
        let hash = hash_password(password_plain)?;
        self.create_user_with_hash(email, display_name, hash).await
    }

    /// 单点登录新建的账号：没有密码（`password_hash` 为空），密码登录一律失败；本人用找回
    /// 密码设置之后才有。
    pub async fn create_external_user(&self, email: String, display_name: String) -> Result<User> {
        self.create_user_with_hash(email, display_name, String::new())
            .await
    }

    async fn create_user_with_hash(
        &self,
        email: String,
        display_name: String,
        password_hash: String,
    ) -> Result<User> {
        let user = User {
            id: Id::new(),
            email,
            display_name,
            avatar_url: None,
            bio: String::new(),
            password_hash,
            disabled: false,
            status: UserStatus::Active,
            created_at: crate::shared::time::TimestampMicros::now(),
            sessions_valid_after: sessions_always_valid(),
        };

        self.users.create(user).await
    }

    /// 仅当 `users` 表为空时，创建 user、默认 org、成员关系，
    /// 并绑定数据库中 `organization_bootstrap` purpose 对应的角色。
    /// 三步顺序执行；任一失败上抛（生产可换成 PG 事务，当前接受可重入）。
    pub async fn create_user_with_default_org(
        &self,
        email: String,
        display_name: String,
        password_plain: &str,
    ) -> Result<(User, crate::domain::iam::Organization)> {
        use crate::{
            domain::iam::{IamMembership, Organization},
            shared::time::TimestampMicros,
        };

        let count = self.users.count().await?;

        if count > 0 {
            return Err(Error::forbidden(
                "create_user_with_default_org allowed only when users table is empty",
            ));
        }

        let org = self
            .orgs
            .create(Organization {
                id: Id::new(),
                name: "default".into(),
                slug: "default".into(),
                system: false,
                disabled: false,
                created_at: TimestampMicros::now(),
            })
            .await?;

        let user = self
            .create_user(email, display_name, password_plain)
            .await?;

        let bootstrap_role_id = self
            .iam_memberships
            .role_id_for_purpose(&org.id, "organization_bootstrap")
            .await?;

        self.iam_memberships
            .upsert(
                IamMembership {
                    user_id: user.id.clone(),
                    org_id: org.id.clone(),
                    joined_at: TimestampMicros::now(),
                },
                &[bootstrap_role_id],
                &user.id,
            )
            .await?;

        Ok((user, org))
    }

    /// 自助注册：在已存在的默认 org（首个）创建用户与成员关系，并绑定
    /// 数据库中 `self_service_signup` purpose 对应的角色。
    /// `status` 由调用方按注册策略传入（active 直接可登录 / pending 待审批）。
    pub async fn signup(
        &self,
        email: String,
        display_name: String,
        password_hash: String,
        status: UserStatus,
    ) -> Result<User> {
        use crate::{domain::iam::IamMembership, shared::time::TimestampMicros};

        // 已存在邮箱：仅当此前被「拒绝」时允许重新申请（复用既有 user + membership，
        // 更新资料/密码并重置状态，重新走审批/激活）。其余状态一律拒绝重复注册。
        if let Ok(existing) = self.users.get_by_email(&email).await {
            if existing.status != UserStatus::Rejected {
                return Err(Error::invalid("email already registered"));
            }

            let org = self.enabled_tenant_organization().await?;
            let mut user = existing;
            user.display_name = display_name;
            user.password_hash = password_hash;
            user.status = status;
            let user = self.users.update(user).await?;
            self.users.set_status(&user.id, status).await?;

            let signup_role_id = self
                .iam_memberships
                .role_id_for_purpose(&org.id, "self_service_signup")
                .await?;

            self.iam_memberships
                .upsert(
                    IamMembership {
                        user_id: user.id.clone(),
                        org_id: org.id,
                        joined_at: TimestampMicros::now(),
                    },
                    &[signup_role_id],
                    &user.id,
                )
                .await?;

            return Ok(user);
        }

        let org = self.enabled_tenant_organization().await?;

        let user = User {
            id: Id::new(),
            email,
            display_name,
            avatar_url: None,
            bio: String::new(),
            password_hash,
            disabled: false,
            status,
            created_at: TimestampMicros::now(),
            sessions_valid_after: sessions_always_valid(),
        };

        let user = self.users.create(user).await?;

        let signup_role_id = self
            .iam_memberships
            .role_id_for_purpose(&org.id, "self_service_signup")
            .await?;

        self.iam_memberships
            .upsert(
                IamMembership {
                    user_id: user.id.clone(),
                    org_id: org.id,
                    joined_at: TimestampMicros::now(),
                },
                &[signup_role_id],
                &user.id,
            )
            .await?;

        Ok(user)
    }

    pub async fn enabled_tenant_organization(&self) -> Result<crate::domain::iam::Organization> {
        self.orgs
            .list()
            .await?
            .into_iter()
            .find(|organization| !organization.system && !organization.disabled)
            .ok_or_else(|| Error::internal("no enabled tenant organization available to join"))
    }

    pub fn auth_settings(&self) -> &AuthSettings {
        &self.auth
    }
}

fn user_access_denial(user: &User) -> Option<&'static str> {
    if user.disabled {
        return Some("user disabled");
    }

    match user.status {
        UserStatus::Pending => Some("account pending approval"),
        UserStatus::Rejected => Some("account registration was rejected"),
        UserStatus::Active => None,
    }
}

pub fn hash_password(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);

    let hash = Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| Error::internal(format!("argon2 hash: {e}")))?
        .to_string();

    Ok(hash)
}

/// 算一次 Argon2，不占 tokio 工作线程：登录洪水不该卡住同节点的写入和查询。
async fn on_password_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T> {
    let _permit = PASSWORD_HASH_PERMITS
        .acquire()
        .await
        .map_err(|error| Error::internal(format!("password hash permit: {error}")))?;

    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| Error::internal(format!("password hash task: {error}")))
}

/// 核对密码。账号没有密码时也算一遍，回答不会因此变快。
pub async fn check_password(plain: &str, hash: &str) -> Result<()> {
    let plain = plain.to_owned();
    let hash = hash.to_owned();

    on_password_thread(move || {
        if hash.is_empty() {
            let _ = verify_password(&plain, &DECOY_PASSWORD_HASH);
        }

        verify_password(&plain, &hash)
    })
    .await?
}

/// 算密码的哈希，见 [`on_password_thread`]。
pub async fn hash_password_off_thread(plain: &str) -> Result<String> {
    let plain = plain.to_owned();

    on_password_thread(move || hash_password(&plain)).await?
}

pub fn verify_password(plain: &str, hash: &str) -> Result<()> {
    // 单点登录新建的账号没有密码。
    if hash.is_empty() {
        return Err(Error::unauthorized("invalid credentials"));
    }

    let parsed =
        PasswordHash::new(hash).map_err(|e| Error::internal(format!("argon2 parse hash: {e}")))?;

    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .map_err(|_| Error::unauthorized("invalid credentials"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::iam::{IamAssignedRole, IamScope};

    #[test]
    fn hash_verify_roundtrip() {
        let h = hash_password("hunter2").unwrap();
        assert!(h.starts_with("$argon2"));
        assert!(verify_password("hunter2", &h).is_ok());
        assert!(verify_password("nope", &h).is_err());
    }

    #[tokio::test]
    async fn an_account_with_no_password_never_matches() {
        assert!(matches!(
            check_password("anything", "").await,
            Err(Error::Unauthorized(_))
        ));

        // 假哈希本身也不会被猜中：它的明文不是空的。
        assert!(check_password("", "").await.is_err());
        assert!(check_password("decoy password", "").await.is_err());
    }

    #[tokio::test]
    async fn the_decoy_costs_the_same_as_a_real_password() {
        let real = hash_password_off_thread("hunter2").await.unwrap();
        let params = |hash: &str| hash.rsplitn(3, '$').nth(2).unwrap().to_owned();

        assert_eq!(params(&DECOY_PASSWORD_HASH), params(&real));
        assert!(check_password("hunter2", &real).await.is_ok());
        assert!(check_password("nope", &real).await.is_err());
    }

    mod authenticate {
        use super::*;
        use crate::app::iam::test_support::{Accounts, iam_service_with};

        fn user(password_hash: String, disabled: bool, status: UserStatus) -> User {
            User {
                id: Id::from_string("user-1"),
                email: "alice@example.com".into(),
                display_name: "Alice".into(),
                avatar_url: None,
                bio: String::new(),
                password_hash,
                disabled,
                status,
                created_at: crate::shared::time::TimestampMicros(0),
                sessions_valid_after: crate::shared::time::TimestampMicros(0),
            }
        }

        fn service_with(user: User) -> IamService {
            iam_service_with(Arc::new(Accounts(vec![user])))
        }

        #[tokio::test]
        async fn an_unknown_account_and_a_wrong_password_answer_the_same() {
            let hash = hash_password("hunter2").unwrap();
            let service = service_with(user(hash, false, UserStatus::Active));

            let unknown = service.authenticate("bob@example.com", "nope").await;
            let wrong = service.authenticate("alice@example.com", "nope").await;

            let Err(Error::Unauthorized(unknown)) = unknown else {
                panic!("an unknown account is unauthorized");
            };

            let Err(Error::Unauthorized(wrong)) = wrong else {
                panic!("a wrong password is unauthorized");
            };

            assert_eq!(unknown, wrong);
        }

        #[tokio::test]
        async fn an_account_with_no_password_answers_like_a_wrong_password() {
            let service = service_with(user(String::new(), false, UserStatus::Active));

            assert!(matches!(
                service.authenticate("alice@example.com", "nope").await,
                Err(Error::Unauthorized(_))
            ));
        }

        #[tokio::test]
        async fn a_disabled_account_says_so_only_to_who_knows_the_password() {
            let hash = hash_password("hunter2").unwrap();
            let service = service_with(user(hash, true, UserStatus::Active));

            assert!(matches!(
                service.authenticate("alice@example.com", "nope").await,
                Err(Error::Unauthorized(_))
            ));

            assert!(matches!(
                service.authenticate("alice@example.com", "hunter2").await,
                Err(Error::Forbidden(_))
            ));
        }

        #[tokio::test]
        async fn a_pending_account_says_so_only_to_who_knows_the_password() {
            let hash = hash_password("hunter2").unwrap();
            let service = service_with(user(hash, false, UserStatus::Pending));

            assert!(matches!(
                service.authenticate("alice@example.com", "nope").await,
                Err(Error::Unauthorized(_))
            ));

            assert!(matches!(
                service.authenticate("alice@example.com", "hunter2").await,
                Err(Error::Forbidden(_))
            ));
        }

        #[tokio::test]
        async fn the_right_password_signs_in() {
            let hash = hash_password("hunter2").unwrap();
            let service = service_with(user(hash, false, UserStatus::Active));

            let signed_in = service
                .authenticate("alice@example.com", "hunter2")
                .await
                .unwrap();

            assert_eq!(signed_in.email, "alice@example.com");
        }
    }

    #[test]
    fn capability_snapshot_uses_exact_database_permission_keys() {
        let context = IamContext {
            user_id: Id::from_string("root"),
            org_id: Id::from_string("_sys"),
            display_role: "Platform Steward".into(),
            roles: vec![IamAssignedRole {
                id: Id::from_string("database-role"),
                key: "platform_steward".into(),
                name: "Platform Steward".into(),
                builtin: false,
            }],
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::System,
            permissions: ["sys.telemetry.read".to_string()].into_iter().collect(),
            policy_version: 1,
        };

        assert!(context.has_permission("sys.telemetry.read"));
        assert!(!context.has_permission("streams.query"));
        assert_eq!(context.organization_role_key(), "");
        assert!(!context.has_permission("org.preferences.manage"));
    }
}
