// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Auditable, resource-scoped sharing persistence.
//!
//! Share tokens keep a BLAKE3 digest for lookup and an AES-GCM envelope for
//! authorized repeat display. Session tokens remain hash-only.
//!
//! Each organization's sharing policy is in [`policy`].

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod policy;

pub use policy::ResourceSharePolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceShareMode {
    Authenticated,
    CrossOrg,
    PublicLink,
}

impl ResourceShareMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Authenticated => "authenticated",
            Self::CrossOrg => "cross_org",
            Self::PublicLink => "public_link",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "authenticated" => Ok(Self::Authenticated),
            "cross_org" => Ok(Self::CrossOrg),
            "public_link" => Ok(Self::PublicLink),
            _ => Err(Error::internal(format!(
                "invalid persisted resource share mode: {value}"
            ))),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceShare {
    pub id: Id,
    pub organization_id: Id,
    pub resource_type: String,
    pub resource_id: Id,
    pub resource_version_id: Option<String>,
    pub share_mode: ResourceShareMode,
    #[serde(skip_serializing)]
    pub token_hash: String,
    /// Decrypted only inside the authenticated sharing control plane.
    #[serde(skip_serializing)]
    pub raw_token: Option<String>,
    pub permissions: Value,
    pub constraints: Value,
    pub expires_at: Option<TimestampMicros>,
    #[serde(skip_serializing)]
    pub password_hash: Option<String>,
    pub max_views: Option<i64>,
    pub view_count: i64,
    pub allow_download: bool,
    pub enabled: bool,
    pub cross_org_grant_id: Option<Id>,
    #[serde(skip_serializing)]
    pub snapshot_object_key: Option<String>,
    pub snapshot_content_type: Option<String>,
    pub snapshot_filename: Option<String>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub last_accessed_at: Option<TimestampMicros>,
    pub revoked_at: Option<TimestampMicros>,
}

#[derive(Debug, Clone)]
pub struct ResourceShareSession {
    pub id: Id,
    pub share_id: Id,
    pub session_token_hash: String,
    pub password_verified: bool,
    pub created_at: TimestampMicros,
    pub expires_at: TimestampMicros,
    pub last_seen_at: TimestampMicros,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

#[async_trait]
pub trait ResourceShareRepository: Send + Sync {
    async fn create(&self, share: ResourceShare) -> Result<ResourceShare>;
    async fn get(&self, organization_id: &Id, id: &Id) -> Result<ResourceShare>;
    async fn get_by_id(&self, id: &Id) -> Result<ResourceShare>;
    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<ResourceShare>>;
    async fn list(
        &self,
        organization_id: &Id,
        resource_type: Option<&str>,
        resource_id: Option<&Id>,
    ) -> Result<Vec<ResourceShare>>;
    async fn rotate_token(
        &self,
        organization_id: &Id,
        id: &Id,
        token_hash: &str,
        raw_token: &str,
    ) -> Result<ResourceShare>;
    async fn revoke(
        &self,
        organization_id: &Id,
        id: &Id,
        now: TimestampMicros,
    ) -> Result<ResourceShare>;
    async fn create_session(
        &self,
        session: ResourceShareSession,
        now: TimestampMicros,
    ) -> Result<ResourceShare>;
    async fn find_session(
        &self,
        session_token_hash: &str,
        now: TimestampMicros,
    ) -> Result<Option<ResourceShareSession>>;
    async fn mark_password_verified(&self, session_id: &Id) -> Result<()>;
    async fn touch_session(&self, session_id: &Id, now: TimestampMicros) -> Result<()>;
    async fn get_policy(&self, organization_id: &Id) -> Result<ResourceSharePolicy>;
    async fn upsert_policy(&self, policy: ResourceSharePolicy) -> Result<ResourceSharePolicy>;
}

pub struct PgResourceShareRepository {
    pool: PgPool,
    cipher: CipherRootKey,
}

impl PgResourceShareRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }
}

const SHARE_COLS: &str = "id, organization_id, resource_type, resource_id,
    resource_version_id, share_mode, token_hash, token_ciphertext, token_nonce,
    permissions_json, constraints_json, expires_at, password_hash, max_views,
    view_count, allow_download, enabled, cross_org_grant_id, snapshot_object_key,
    snapshot_content_type, snapshot_filename, created_by, created_at,
    last_accessed_at, revoked_at";

fn row_to_share(row: sqlx::postgres::PgRow, cipher: &CipherRootKey) -> Result<ResourceShare> {
    let mode: String = row.try_get("share_mode").map_err(sqlx_err)?;
    let permissions: Json<Value> = row.try_get("permissions_json").map_err(sqlx_err)?;
    let constraints: Json<Value> = row.try_get("constraints_json").map_err(sqlx_err)?;
    let token_ciphertext: Option<Vec<u8>> = row.try_get("token_ciphertext").map_err(sqlx_err)?;
    let token_nonce: Option<Vec<u8>> = row.try_get("token_nonce").map_err(sqlx_err)?;

    let raw_token = match (token_ciphertext, token_nonce) {
        (Some(ciphertext), Some(nonce)) => {
            let plaintext = cipher
                .open(&nonce, &ciphertext)
                .map_err(|_| Error::internal("resource share token decrypt failed"))?;

            Some(
                String::from_utf8(plaintext)
                    .map_err(|_| Error::internal("resource share token is not UTF-8"))?,
            )
        }
        (None, None) => None,
        _ => {
            return Err(Error::internal(
                "resource share token envelope is incomplete",
            ));
        }
    };

    Ok(ResourceShare {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        organization_id: Id::from_string(
            row.try_get::<String, _>("organization_id")
                .map_err(sqlx_err)?,
        ),
        resource_type: row.try_get("resource_type").map_err(sqlx_err)?,
        resource_id: Id::from_string(row.try_get::<String, _>("resource_id").map_err(sqlx_err)?),
        resource_version_id: row.try_get("resource_version_id").map_err(sqlx_err)?,
        share_mode: ResourceShareMode::parse(&mode)?,
        token_hash: row.try_get("token_hash").map_err(sqlx_err)?,
        raw_token,
        permissions: permissions.0,
        constraints: constraints.0,
        expires_at: row
            .try_get::<Option<TimestampMicros>, _>("expires_at")
            .map_err(sqlx_err)?,
        password_hash: row.try_get("password_hash").map_err(sqlx_err)?,
        max_views: row.try_get("max_views").map_err(sqlx_err)?,
        view_count: row.try_get("view_count").map_err(sqlx_err)?,
        allow_download: row.try_get("allow_download").map_err(sqlx_err)?,
        enabled: row.try_get("enabled").map_err(sqlx_err)?,
        cross_org_grant_id: row
            .try_get::<Option<String>, _>("cross_org_grant_id")
            .map_err(sqlx_err)?
            .map(Id::from_string),
        snapshot_object_key: row.try_get("snapshot_object_key").map_err(sqlx_err)?,
        snapshot_content_type: row.try_get("snapshot_content_type").map_err(sqlx_err)?,
        snapshot_filename: row.try_get("snapshot_filename").map_err(sqlx_err)?,
        created_by: Id::from_string(row.try_get::<String, _>("created_by").map_err(sqlx_err)?),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        last_accessed_at: row
            .try_get::<Option<TimestampMicros>, _>("last_accessed_at")
            .map_err(sqlx_err)?,
        revoked_at: row
            .try_get::<Option<TimestampMicros>, _>("revoked_at")
            .map_err(sqlx_err)?,
    })
}

fn row_to_session(row: sqlx::postgres::PgRow) -> Result<ResourceShareSession> {
    Ok(ResourceShareSession {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        share_id: Id::from_string(row.try_get::<String, _>("share_id").map_err(sqlx_err)?),
        session_token_hash: row.try_get("session_token_hash").map_err(sqlx_err)?,
        password_verified: row.try_get("password_verified").map_err(sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        expires_at: row
            .try_get::<TimestampMicros, _>("expires_at")
            .map_err(sqlx_err)?,
        last_seen_at: row
            .try_get::<TimestampMicros, _>("last_seen_at")
            .map_err(sqlx_err)?,
        ip: row.try_get("ip").map_err(sqlx_err)?,
        user_agent: row.try_get("user_agent").map_err(sqlx_err)?,
    })
}

#[async_trait]
impl ResourceShareRepository for PgResourceShareRepository {
    async fn create(&self, share: ResourceShare) -> Result<ResourceShare> {
        let raw_token = share
            .raw_token
            .as_deref()
            .ok_or_else(|| Error::internal("resource share token is required"))?;

        let (token_nonce, token_ciphertext) = self
            .cipher
            .seal(raw_token.as_bytes())
            .map_err(|e| Error::internal(format!("resource share token seal failed: {e}")))?;

        sqlx::query(
            "INSERT INTO resource_shares (
                id, organization_id, resource_type, resource_id, resource_version_id,
                share_mode, token_hash, token_ciphertext, token_nonce, permissions_json,
                constraints_json, expires_at, password_hash, max_views, view_count,
                allow_download, enabled, cross_org_grant_id, snapshot_object_key,
                snapshot_content_type, snapshot_filename, created_by, created_at,
                last_accessed_at, revoked_at
             ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25
             )",
        )
        .bind(&share.id.0)
        .bind(&share.organization_id.0)
        .bind(&share.resource_type)
        .bind(&share.resource_id.0)
        .bind(&share.resource_version_id)
        .bind(share.share_mode.as_str())
        .bind(&share.token_hash)
        .bind(&token_ciphertext)
        .bind(&token_nonce)
        .bind(Json(&share.permissions))
        .bind(Json(&share.constraints))
        .bind(share.expires_at)
        .bind(&share.password_hash)
        .bind(share.max_views)
        .bind(share.view_count)
        .bind(share.allow_download)
        .bind(share.enabled)
        .bind(share.cross_org_grant_id.as_ref().map(|value| &value.0))
        .bind(&share.snapshot_object_key)
        .bind(&share.snapshot_content_type)
        .bind(&share.snapshot_filename)
        .bind(&share.created_by.0)
        .bind(share.created_at)
        .bind(share.last_accessed_at)
        .bind(share.revoked_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(share)
    }

    async fn get(&self, organization_id: &Id, id: &Id) -> Result<ResourceShare> {
        let sql = format!(
            "SELECT {SHARE_COLS} FROM resource_shares
             WHERE organization_id = $1 AND id = $2"
        );

        sqlx::query(&sql)
            .bind(&organization_id.0)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .map(|row| row_to_share(row, &self.cipher))
            .transpose()?
            .ok_or_else(|| Error::not_found("resource share not found"))
    }

    async fn get_by_id(&self, id: &Id) -> Result<ResourceShare> {
        let sql = format!("SELECT {SHARE_COLS} FROM resource_shares WHERE id = $1");

        sqlx::query(&sql)
            .bind(&id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .map(|row| row_to_share(row, &self.cipher))
            .transpose()?
            .ok_or_else(|| Error::not_found("resource share not found"))
    }

    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<ResourceShare>> {
        let sql = format!("SELECT {SHARE_COLS} FROM resource_shares WHERE token_hash = $1");

        sqlx::query(&sql)
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .map(|row| row_to_share(row, &self.cipher))
            .transpose()
    }

    async fn list(
        &self,
        organization_id: &Id,
        resource_type: Option<&str>,
        resource_id: Option<&Id>,
    ) -> Result<Vec<ResourceShare>> {
        let sql = format!(
            "SELECT {SHARE_COLS} FROM resource_shares
             WHERE organization_id = $1
               AND ($2::text IS NULL OR resource_type = $2)
               AND ($3::text IS NULL OR resource_id = $3)
             ORDER BY created_at DESC
             LIMIT 500"
        );

        sqlx::query(&sql)
            .bind(&organization_id.0)
            .bind(resource_type)
            .bind(resource_id.map(|value| value.0.as_str()))
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?
            .into_iter()
            .map(|row| row_to_share(row, &self.cipher))
            .collect()
    }

    async fn rotate_token(
        &self,
        organization_id: &Id,
        id: &Id,
        token_hash: &str,
        raw_token: &str,
    ) -> Result<ResourceShare> {
        let (token_nonce, token_ciphertext) = self
            .cipher
            .seal(raw_token.as_bytes())
            .map_err(|e| Error::internal(format!("resource share token seal failed: {e}")))?;

        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let sql = format!(
            "UPDATE resource_shares
             SET token_hash = $3, token_ciphertext = $4, token_nonce = $5,
                 enabled = TRUE, revoked_at = NULL
             WHERE organization_id = $1 AND id = $2
             RETURNING {SHARE_COLS}"
        );

        let share = sqlx::query(&sql)
            .bind(&organization_id.0)
            .bind(&id.0)
            .bind(token_hash)
            .bind(&token_ciphertext)
            .bind(&token_nonce)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .map(|row| row_to_share(row, &self.cipher))
            .transpose()?
            .ok_or_else(|| Error::not_found("resource share not found"))?;

        sqlx::query("DELETE FROM resource_share_sessions WHERE share_id = $1")
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(share)
    }

    async fn revoke(
        &self,
        organization_id: &Id,
        id: &Id,
        now: TimestampMicros,
    ) -> Result<ResourceShare> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let sql = format!(
            "UPDATE resource_shares
             SET enabled = FALSE, revoked_at = $3
             WHERE organization_id = $1 AND id = $2
             RETURNING {SHARE_COLS}"
        );

        let share = sqlx::query(&sql)
            .bind(&organization_id.0)
            .bind(&id.0)
            .bind(now)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .map(|row| row_to_share(row, &self.cipher))
            .transpose()?
            .ok_or_else(|| Error::not_found("resource share not found"))?;

        sqlx::query("DELETE FROM resource_share_sessions WHERE share_id = $1")
            .bind(&id.0)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(share)
    }

    async fn create_session(
        &self,
        session: ResourceShareSession,
        now: TimestampMicros,
    ) -> Result<ResourceShare> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        let sql = format!(
            "UPDATE resource_shares
             SET view_count = view_count + 1, last_accessed_at = $2
             WHERE id = $1
               AND enabled
               AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > $2)
               AND (max_views IS NULL OR view_count < max_views)
             RETURNING {SHARE_COLS}"
        );

        let share = sqlx::query(&sql)
            .bind(&session.share_id.0)
            .bind(now)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .map(|row| row_to_share(row, &self.cipher))
            .transpose()?
            .ok_or_else(|| Error::not_found("resource share unavailable"))?;

        sqlx::query(
            "INSERT INTO resource_share_sessions (
                id, share_id, session_token_hash, password_verified,
                created_at, expires_at, last_seen_at, ip, user_agent
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&session.id.0)
        .bind(&session.share_id.0)
        .bind(&session.session_token_hash)
        .bind(session.password_verified)
        .bind(session.created_at)
        .bind(session.expires_at)
        .bind(session.last_seen_at)
        .bind(&session.ip)
        .bind(&session.user_agent)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(share)
    }

    async fn find_session(
        &self,
        session_token_hash: &str,
        now: TimestampMicros,
    ) -> Result<Option<ResourceShareSession>> {
        sqlx::query(
            "SELECT id, share_id, session_token_hash, password_verified,
                    created_at, expires_at, last_seen_at, ip, user_agent
             FROM resource_share_sessions
             WHERE session_token_hash = $1 AND expires_at > $2",
        )
        .bind(session_token_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(row_to_session)
        .transpose()
    }

    async fn mark_password_verified(&self, session_id: &Id) -> Result<()> {
        sqlx::query("UPDATE resource_share_sessions SET password_verified = TRUE WHERE id = $1")
            .bind(&session_id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn touch_session(&self, session_id: &Id, now: TimestampMicros) -> Result<()> {
        sqlx::query("UPDATE resource_share_sessions SET last_seen_at = $2 WHERE id = $1")
            .bind(&session_id.0)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(())
    }

    async fn get_policy(&self, organization_id: &Id) -> Result<ResourceSharePolicy> {
        policy::get(&self.pool, organization_id).await
    }

    async fn upsert_policy(&self, policy: ResourceSharePolicy) -> Result<ResourceSharePolicy> {
        policy::put(&self.pool, policy).await
    }
}
