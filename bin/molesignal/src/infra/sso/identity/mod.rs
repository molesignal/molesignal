// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 外部身份与账号的绑定（`sso_identities`）。
//!
//! 单点登录只按「服务 + 主体」找账号，不按邮箱：一个服务里的一个身份只对应一个账号，一个账号
//! 在一个服务里只绑一个身份。绑定记录它是怎么建立的（[`SsoIdentityProof`]），留有
//! [`SsoIdentityProof::Created`] 绑定的账号只能加入该服务所在的组织
//! （[`SsoIdentityRepository::creator_org`]）。
//!
//! 已有账号首次登录时等待本人确认的请求见 [`link_requests`]。

pub mod link_requests;

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::{
    domain::iam::{SsoPlatform, SsoProviderKind},
    infra::persistence::sqlx_err,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 绑定是怎么建立的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsoIdentityProof {
    /// 该服务登录时新建了账号，身份源给出的邮箱未经证明。
    Created,
    /// 公网身份源（Google、GitHub 等）确认过邮箱时新建了账号。
    VerifiedEmail,
    /// 本人输入了发到账号邮箱的验证码。
    Email,
    /// 本人输入了账号密码。
    Password,
}

impl SsoIdentityProof {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::VerifiedEmail => "verified_email",
            Self::Email => "email",
            Self::Password => "password",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "created" => Ok(Self::Created),
            "verified_email" => Ok(Self::VerifiedEmail),
            "email" => Ok(Self::Email),
            "password" => Ok(Self::Password),
            other => Err(Error::internal(format!(
                "unknown sso_identities.proof value: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SsoIdentity {
    pub id: Id,
    pub provider_id: Id,
    pub subject: String,
    pub user_id: Id,
    pub proof: SsoIdentityProof,
    pub created_at: TimestampMicros,
    pub last_login_at: TimestampMicros,
}

impl SsoIdentity {
    pub fn new(provider_id: Id, subject: String, user_id: Id, proof: SsoIdentityProof) -> Self {
        let now = TimestampMicros::now();

        Self {
            id: Id::new(),
            provider_id,
            subject,
            user_id,
            proof,
            created_at: now,
            last_login_at: now,
        }
    }
}

/// 账号页与成员列表看到的绑定：带上服务与组织的名字。
#[derive(Debug, Clone)]
pub struct SsoIdentityView {
    pub id: Id,
    pub provider_id: Id,
    pub provider_name: String,
    pub kind: SsoProviderKind,
    pub platform: SsoPlatform,
    pub org_id: Id,
    pub org_name: String,
    pub proof: SsoIdentityProof,
    pub created_at: TimestampMicros,
    pub last_login_at: TimestampMicros,
}

#[async_trait]
pub trait SsoIdentityRepository: Send + Sync {
    async fn find(&self, provider_id: &Id, subject: &str) -> Result<Option<SsoIdentity>>;
    async fn get(&self, id: &Id) -> Result<SsoIdentity>;
    /// 新账号的第一个绑定。
    async fn create(&self, identity: SsoIdentity) -> Result<()>;
    /// 本人确认后的绑定：替换账号在该服务里原有的绑定；`drop_created` 时一并作废账号上
    /// 所有邮箱未经证明的绑定（本人刚证明了邮箱）。
    async fn link(&self, identity: SsoIdentity, drop_created: bool) -> Result<()>;
    async fn touch(&self, id: &Id, at: TimestampMicros) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
    /// 最近登录的在前。
    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<SsoIdentityView>>;
    /// 账号由哪个组织的服务新建且邮箱未经证明；这样的账号只能加入该组织。
    async fn creator_org(&self, user_id: &Id) -> Result<Option<Id>>;
}

pub struct PgSsoIdentityRepository {
    pool: PgPool,
}

impl PgSsoIdentityRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, provider_id, subject, user_id, proof, created_at, last_login_at";

fn row_to_identity(row: sqlx::postgres::PgRow) -> Result<SsoIdentity> {
    let proof: String = row.try_get("proof").map_err(sqlx_err)?;

    Ok(SsoIdentity {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        provider_id: Id::from_string(row.try_get::<String, _>("provider_id").map_err(sqlx_err)?),
        subject: row.try_get("subject").map_err(sqlx_err)?,
        user_id: Id::from_string(row.try_get::<String, _>("user_id").map_err(sqlx_err)?),
        proof: SsoIdentityProof::parse(&proof)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        last_login_at: row
            .try_get::<TimestampMicros, _>("last_login_at")
            .map_err(sqlx_err)?,
    })
}

fn row_to_view(row: sqlx::postgres::PgRow) -> Result<SsoIdentityView> {
    let proof: String = row.try_get("proof").map_err(sqlx_err)?;
    let kind: String = row.try_get("provider").map_err(sqlx_err)?;
    let platform: String = row.try_get("platform").map_err(sqlx_err)?;

    let kind = match kind.as_str() {
        "oidc" => SsoProviderKind::Oidc,
        "oauth2" => SsoProviderKind::Oauth2,
        "saml" => SsoProviderKind::Saml,
        "ldap" => SsoProviderKind::Ldap,
        other => {
            return Err(Error::internal(format!(
                "unknown sso_providers.provider value: {other}"
            )));
        }
    };

    Ok(SsoIdentityView {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        provider_id: Id::from_string(row.try_get::<String, _>("provider_id").map_err(sqlx_err)?),
        provider_name: row.try_get("provider_name").map_err(sqlx_err)?,
        kind,
        platform: SsoPlatform::parse(&platform).unwrap_or_default(),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        org_name: row
            .try_get::<Option<String>, _>("org_name")
            .map_err(sqlx_err)?
            .unwrap_or_default(),
        proof: SsoIdentityProof::parse(&proof)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        last_login_at: row
            .try_get::<TimestampMicros, _>("last_login_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl SsoIdentityRepository for PgSsoIdentityRepository {
    async fn find(&self, provider_id: &Id, subject: &str) -> Result<Option<SsoIdentity>> {
        sqlx::query(&format!(
            "SELECT {COLS} FROM sso_identities WHERE provider_id = $1 AND subject = $2"
        ))
        .bind(&provider_id.0)
        .bind(subject)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(row_to_identity)
        .transpose()
    }

    async fn get(&self, id: &Id) -> Result<SsoIdentity> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM sso_identities WHERE id = $1"))
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .ok_or_else(|| Error::not_found(format!("sso identity {}", id.0)))?;

        row_to_identity(row)
    }

    async fn create(&self, identity: SsoIdentity) -> Result<()> {
        sqlx::query(
            "INSERT INTO sso_identities
                (id, provider_id, subject, user_id, proof, created_at, last_login_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&identity.id.0)
        .bind(&identity.provider_id.0)
        .bind(&identity.subject)
        .bind(&identity.user_id.0)
        .bind(identity.proof.as_str())
        .bind(identity.created_at)
        .bind(identity.last_login_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn link(&self, identity: SsoIdentity, drop_created: bool) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        // 账号在该服务里只留一个身份；同一个身份也只对应一个账号。
        sqlx::query(
            "DELETE FROM sso_identities
             WHERE provider_id = $1 AND (user_id = $2 OR subject = $3)",
        )
        .bind(&identity.provider_id.0)
        .bind(&identity.user_id.0)
        .bind(&identity.subject)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        if drop_created {
            sqlx::query("DELETE FROM sso_identities WHERE user_id = $1 AND proof = 'created'")
                .bind(&identity.user_id.0)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
        }

        sqlx::query(
            "INSERT INTO sso_identities
                (id, provider_id, subject, user_id, proof, created_at, last_login_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&identity.id.0)
        .bind(&identity.provider_id.0)
        .bind(&identity.subject)
        .bind(&identity.user_id.0)
        .bind(identity.proof.as_str())
        .bind(identity.created_at)
        .bind(identity.last_login_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)
    }

    async fn touch(&self, id: &Id, at: TimestampMicros) -> Result<()> {
        sqlx::query("UPDATE sso_identities SET last_login_at = $2 WHERE id = $1")
            .bind(&id.0)
            .bind(at)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let deleted = sqlx::query("DELETE FROM sso_identities WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        if deleted == 0 {
            return Err(Error::not_found(format!("sso identity {}", id.0)));
        }

        Ok(())
    }

    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<SsoIdentityView>> {
        let rows = sqlx::query(
            "SELECT i.id, i.provider_id, i.proof, i.created_at, i.last_login_at,
                    p.name AS provider_name, p.provider, p.platform, p.org_id,
                    o.name AS org_name
             FROM sso_identities i
             JOIN sso_providers p ON p.id = i.provider_id
             LEFT JOIN organizations o ON o.id = p.org_id
             WHERE i.user_id = $1
             ORDER BY i.last_login_at DESC",
        )
        .bind(&user_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter().map(row_to_view).collect()
    }

    async fn creator_org(&self, user_id: &Id) -> Result<Option<Id>> {
        let row = sqlx::query(
            "SELECT p.org_id
             FROM sso_identities i
             JOIN sso_providers p ON p.id = i.provider_id
             WHERE i.user_id = $1 AND i.proof = 'created'
             LIMIT 1",
        )
        .bind(&user_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        row.map(|row| {
            row.try_get::<String, _>("org_id")
                .map(Id::from_string)
                .map_err(sqlx_err)
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::SsoIdentityProof;

    #[test]
    fn proofs_round_trip() {
        for proof in [
            SsoIdentityProof::Created,
            SsoIdentityProof::VerifiedEmail,
            SsoIdentityProof::Email,
            SsoIdentityProof::Password,
        ] {
            assert_eq!(SsoIdentityProof::parse(proof.as_str()).unwrap(), proof);
        }

        assert!(SsoIdentityProof::parse("trusted").is_err());
    }
}
