// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::BTreeSet, sync::Arc};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::{
    config::AuthSettings,
    domain::iam::{
        IamAssignedRole, IamMembershipRepository, IamScope, OrganizationRepository, User,
        UserRepository, UserStatus,
    },
    shared::{Error, Result, ids::Id},
};

const JWT_ISSUER: &str = "molesignal";

pub mod access;
mod navigation;
mod scoped_token;
pub use access::{
    IamAccessRequest, IamAccessService, IamAttributes, IamCapabilitySnapshot, IamContextEnricher,
    IamDecision, IamDecisionReason, IamSubject, IamTarget, validate_iam_conditions,
};
pub use navigation::resolve_route_access;

pub struct IamService {
    pub users: Arc<dyn UserRepository>,
    pub orgs: Arc<dyn OrganizationRepository>,
    pub iam_memberships: Arc<dyn IamMembershipRepository>,
    auth: AuthSettings,
    /// JWT 签名 secret 集合（auth-hardening）：
    /// `[0]` 是 primary（用来签新 token）；其余是 24h grace 内的 retired secret
    /// （仅参与 verify）。由 `bootstrap::build_state` 通过 `bootstrap_or_load_jwt_secret` 注入。
    jwt_secrets: Arc<RwLock<Vec<Vec<u8>>>>,
}

/// JWT 载荷。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,    // user_id
    pub org_id: String, // 当前激活的 org
    #[serde(default)]
    pub scope: IamScope,
    #[serde(default)]
    pub iat: usize,
    pub exp: usize,
    pub iss: String,
}

/// 被注入到 axum 请求扩展的认证上下文。
#[derive(Debug, Clone)]
pub struct IamContext {
    pub user_id: Id,
    pub org_id: Id,
    /// Server-resolved display metadata. Authorization never reads this field.
    pub display_role: String,
    pub roles: Vec<IamAssignedRole>,
    /// API tokens are directly scoped to one database IAM role.
    pub credential_role_id: Option<Id>,
    /// Public RUM credentials are bound to one application and never inherit user access.
    pub credential_application_id: Option<String>,
    pub scope: IamScope,
    /// Canonical, server-resolved capability keys for this request.
    pub permissions: BTreeSet<String>,
    /// Active product features included in the capability snapshot.
    pub features: BTreeSet<String>,
    /// Monotonic organization policy version used to resolve `permissions`.
    pub policy_version: u64,
}

impl IamContext {
    pub fn is_system_scope(&self) -> bool {
        self.scope == IamScope::System
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }

    /// Dynamic role key used only for query admission work-group selection.
    /// Authorization itself is based on `permissions`.
    pub fn organization_role_key(&self) -> &str {
        if self.is_system_scope() {
            ""
        } else {
            self.roles.first().map_or("", |role| role.key.as_str())
        }
    }
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
        }
    }

    /// rotate 路径调用：刷新内存里的 active secrets。`new_primary` 放第 0 位。
    pub fn replace_jwt_secrets(&self, new_active: Vec<Vec<u8>>) {
        assert!(!new_active.is_empty());
        *self.jwt_secrets.write() = new_active;
    }

    /// 用户登录：邮箱 + 密码 → User（成功）/ Error::unauthorized（任一失败）。
    /// 邮箱不存在与密码错都返同样错误，避免 user-enumeration。
    pub async fn authenticate(&self, email: &str, password: &str) -> Result<User> {
        let user = match self.users.get_by_email(email).await {
            Ok(u) => u,
            Err(_) => return Err(Error::unauthorized("invalid credentials")),
        };
        if let Some(reason) = user_access_denial(&user) {
            return Err(Error::forbidden(reason));
        }
        verify_password(password, &user.password_hash)?;
        Ok(user)
    }

    /// 签发 JWT；用当前 primary secret。
    pub fn issue_token(&self, user_id: &Id, org_id: &Id) -> Result<String> {
        let now = chrono::Utc::now().timestamp() as usize;
        let claims = Claims {
            sub: user_id.0.clone(),
            org_id: org_id.0.clone(),
            scope: IamScope::Organization,
            iat: now,
            exp: now + self.auth.token_ttl_secs as usize,
            iss: JWT_ISSUER.to_owned(),
        };
        let secrets = self.jwt_secrets.read();
        let primary = secrets.first().expect("primary jwt secret");
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(primary),
        )
        .map_err(|e| Error::internal(format!("jwt encode: {e}")))
    }

    /// 平台管理员切换 `_sys` 后签发的短期 token。调用方必须先查询持久化
    /// platform-administrator assignment；本函数不接受 API token。
    pub fn issue_system_token(&self, user_id: &Id, system_org_id: &Id) -> Result<String> {
        let now = chrono::Utc::now().timestamp() as usize;
        let claims = Claims {
            sub: user_id.0.clone(),
            org_id: system_org_id.0.clone(),
            scope: IamScope::System,
            iat: now,
            exp: now + (self.auth.token_ttl_secs as usize).min(3_600),
            iss: JWT_ISSUER.to_owned(),
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

    /// 校验 JWT，返回 [`IamContext`]。多 secret 试验，命中 primary 或 retire window 内任意一个即通过。
    pub fn verify_token(&self, token: &str) -> Result<IamContext> {
        let mut validation = Validation::default();
        validation.set_issuer(&[JWT_ISSUER]);
        let secrets = self.jwt_secrets.read();
        let mut last_err: Option<String> = None;
        for s in secrets.iter() {
            match decode::<Claims>(token, &DecodingKey::from_secret(s), &validation) {
                Ok(data) => {
                    return Ok(IamContext {
                        user_id: Id::from_string(data.claims.sub),
                        org_id: Id::from_string(data.claims.org_id),
                        display_role: String::new(),
                        roles: Vec::new(),
                        credential_role_id: None,
                        credential_application_id: None,
                        scope: data.claims.scope,
                        permissions: BTreeSet::new(),
                        features: BTreeSet::new(),
                        policy_version: 0,
                    });
                }
                Err(e) => {
                    last_err = Some(e.to_string());
                }
            }
        }
        Err(Error::unauthorized(format!(
            "jwt verify failed against all {} active secret(s): {}",
            secrets.len(),
            last_err.unwrap_or_else(|| "no secrets configured".into())
        )))
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
        let user = User {
            id: Id::new(),
            email,
            display_name,
            avatar_url: None,
            bio: String::new(),
            password_hash: hash,
            disabled: false,
            status: UserStatus::Active,
            created_at: crate::shared::time::TimestampMicros::now(),
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
        password_plain: &str,
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
            user.password_hash = hash_password(password_plain)?;
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
        let hash = hash_password(password_plain)?;
        let user = User {
            id: Id::new(),
            email,
            display_name,
            avatar_url: None,
            bio: String::new(),
            password_hash: hash,
            disabled: false,
            status,
            created_at: TimestampMicros::now(),
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

    async fn enabled_tenant_organization(&self) -> Result<crate::domain::iam::Organization> {
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

pub fn verify_password(plain: &str, hash: &str) -> Result<()> {
    let parsed =
        PasswordHash::new(hash).map_err(|e| Error::internal(format!("argon2 parse hash: {e}")))?;
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .map_err(|_| Error::unauthorized("invalid credentials"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_verify_roundtrip() {
        let h = hash_password("hunter2").unwrap();
        assert!(h.starts_with("$argon2"));
        assert!(verify_password("hunter2", &h).is_ok());
        assert!(verify_password("nope", &h).is_err());
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
            credential_role_id: None,
            credential_application_id: None,
            scope: IamScope::System,
            permissions: ["sys.telemetry.read".to_string()].into_iter().collect(),
            features: BTreeSet::new(),
            policy_version: 1,
        };

        assert!(context.has_permission("sys.telemetry.read"));
        assert!(!context.has_permission("streams.query"));
        assert_eq!(context.organization_role_key(), "");
        assert!(!context.has_permission("org.settings.manage"));
    }
}
