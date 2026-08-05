// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL adapter and credential helpers for API tokens.

use std::time::Duration;

use async_trait::async_trait;
use dashmap::DashMap;
use moka::future::Cache;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::iam::api_token::{ApiToken, ApiTokenKind, ApiTokenRepository, ManagedApiToken},
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod credential;

pub use credential::{
    assemble_rum_token, assemble_token, generate_token_parts, hash_rum_secret, hash_secret,
    split_rum_token, split_token, verify_rum_secret, verify_secret,
};

const TOKEN_CACHE_TTL_SECS: u64 = 15;
const LAST_USED_WRITE_INTERVAL_MICROS: i64 = 5 * 60 * 1_000_000;

pub struct PgApiTokenRepository {
    pool: PgPool,
    cipher: Option<CipherRootKey>,
    tokens: Cache<String, Option<ApiToken>>,
    last_used_writes: DashMap<String, i64>,
}

impl PgApiTokenRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            cipher: None,
            tokens: Cache::builder()
                .max_capacity(50_000)
                .time_to_live(Duration::from_secs(TOKEN_CACHE_TTL_SECS))
                .build(),
            last_used_writes: DashMap::new(),
        }
    }

    pub fn with_cipher(mut self, cipher: CipherRootKey) -> Self {
        self.cipher = Some(cipher);
        self
    }

    fn cipher(&self) -> Result<&CipherRootKey> {
        self.cipher
            .as_ref()
            .ok_or_else(|| Error::internal("managed API token requires a cipher root key"))
    }
}

const COLS: &str = "id, prefix, secret_hash, org_id, user_id, role_id, name,
                    expires_at_micros, last_used_at_micros, revoked, created_at_micros,
                    is_default, token_kind, application_id";

fn row_to(row: sqlx::postgres::PgRow) -> ApiToken {
    let token_kind = row
        .try_get::<String, _>("token_kind")
        .ok()
        .and_then(|value| ApiTokenKind::parse(&value))
        .unwrap_or(ApiTokenKind::Personal);
    ApiToken {
        id: Id(row.try_get::<String, _>("id").unwrap_or_default()),
        prefix: row.try_get::<String, _>("prefix").unwrap_or_default(),
        secret_hash: row.try_get::<String, _>("secret_hash").unwrap_or_default(),
        org_id: Id(row.try_get::<String, _>("org_id").unwrap_or_default()),
        user_id: Id(row.try_get::<String, _>("user_id").unwrap_or_default()),
        role_id: Id(row.try_get::<String, _>("role_id").unwrap_or_default()),
        name: row.try_get::<String, _>("name").unwrap_or_default(),
        expires_at: row
            .try_get::<Option<i64>, _>("expires_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
        last_used_at: row
            .try_get::<Option<i64>, _>("last_used_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
        revoked: row.try_get::<bool, _>("revoked").unwrap_or(false),
        created_at: TimestampMicros(
            row.try_get::<i64, _>("created_at_micros")
                .unwrap_or_default(),
        ),
        is_default: row.try_get::<bool, _>("is_default").unwrap_or(false),
        token_kind,
        application_id: row
            .try_get::<Option<String>, _>("application_id")
            .unwrap_or_default(),
    }
}

#[async_trait]
impl ApiTokenRepository for PgApiTokenRepository {
    async fn create(&self, token: ApiToken) -> Result<ApiToken> {
        sqlx::query(
            "INSERT INTO api_tokens
                (id, prefix, secret_hash, org_id, user_id, role_id, name,
                 expires_at_micros, last_used_at_micros, revoked, created_at_micros,
                 is_default, token_kind, application_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL, $9, $10, $11, $12, $13)",
        )
        .bind(&token.id.0)
        .bind(&token.prefix)
        .bind(&token.secret_hash)
        .bind(&token.org_id.0)
        .bind(&token.user_id.0)
        .bind(&token.role_id.0)
        .bind(&token.name)
        .bind(token.expires_at.map(|value| value.0))
        .bind(token.revoked)
        .bind(token.created_at.0)
        .bind(token.is_default)
        .bind(token.token_kind.as_str())
        .bind(&token.application_id)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        self.tokens
            .insert(token.prefix.clone(), Some(token.clone()))
            .await;
        Ok(token)
    }

    async fn find_by_prefix(&self, prefix: &str) -> Result<Option<ApiToken>> {
        if let Some(cached) = self.tokens.get(prefix).await {
            return Ok(cached);
        }
        let sql = format!("SELECT {COLS} FROM api_tokens WHERE prefix = $1");
        let token = sqlx::query(&sql)
            .bind(prefix)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?
            .map(row_to);
        self.tokens.insert(prefix.to_string(), token.clone()).await;
        Ok(token)
    }

    async fn list_by_org(&self, org_id: &Id) -> Result<Vec<ApiToken>> {
        let sql = format!(
            "SELECT {COLS} FROM api_tokens
             WHERE org_id = $1 ORDER BY created_at_micros DESC LIMIT 500"
        );
        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<ApiToken> {
        let sql = format!("SELECT {COLS} FROM api_tokens WHERE org_id = $1 AND id = $2");
        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&id.0)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row_to(row))
    }

    async fn mark_revoked(&self, org_id: &Id, id: &Id) -> Result<()> {
        let row = sqlx::query(
            "UPDATE api_tokens SET revoked = TRUE
             WHERE org_id = $1 AND id = $2 RETURNING prefix",
        )
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        if let Some(row) = row {
            let prefix = row.try_get::<String, _>("prefix").unwrap_or_default();
            self.tokens.invalidate(&prefix).await;
            self.last_used_writes.remove(&prefix);
        }
        Ok(())
    }

    async fn touch_last_used(&self, prefix: &str, at: TimestampMicros) -> Result<()> {
        if self
            .last_used_writes
            .get(prefix)
            .is_some_and(|last| at.0.saturating_sub(*last) < LAST_USED_WRITE_INTERVAL_MICROS)
        {
            return Ok(());
        }
        self.last_used_writes.insert(prefix.to_string(), at.0);
        sqlx::query(
            "UPDATE api_tokens SET last_used_at_micros = $2
             WHERE prefix = $1
               AND (last_used_at_micros IS NULL OR last_used_at_micros < $3)",
        )
        .bind(prefix)
        .bind(at.0)
        .bind(at.0.saturating_sub(LAST_USED_WRITE_INTERVAL_MICROS))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn ensure_default(
        &self,
        org_id: &Id,
        user_id: &Id,
        role_id: &Id,
    ) -> Result<ManagedApiToken> {
        let cipher = self.cipher()?;
        let lock_key = format!("default-ingestion-token:{}:{}", org_id.0, user_id.0);
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        let existing = sqlx::query(
            "SELECT id, prefix, role_id, created_at_micros, plaintext_sealed, plaintext_nonce
             FROM api_tokens
             WHERE org_id = $1 AND user_id = $2
               AND token_kind = 'default_ingestion' AND NOT revoked
             LIMIT 1",
        )
        .bind(&org_id.0)
        .bind(&user_id.0)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(sqlx_err)?;
        if let Some(row) = existing
            && let Some(token) =
                open_managed_token(cipher, row, role_id, ApiTokenKind::DefaultIngestion, None)?
        {
            sqlx::query("UPDATE api_tokens SET role_id = $2 WHERE id = $1")
                .bind(&token.id.0)
                .bind(&role_id.0)
                .execute(&mut *transaction)
                .await
                .map_err(sqlx_err)?;
            transaction.commit().await.map_err(sqlx_err)?;
            self.tokens.invalidate(&token.prefix).await;
            return Ok(token);
        }

        let (prefix, secret) = generate_token_parts();
        let plaintext = assemble_token(&prefix, &secret);
        let secret_hash = hash_secret(&secret)?;
        let (nonce, sealed) = cipher
            .seal(plaintext.as_bytes())
            .map_err(|error| Error::internal(format!("default ingestion token seal: {error}")))?;
        let id = Id::new();
        let now = TimestampMicros::now();
        let revoked_prefixes = sqlx::query_scalar::<String>(
            "UPDATE api_tokens SET revoked = TRUE
             WHERE org_id = $1 AND user_id = $2
               AND token_kind = 'default_ingestion' AND NOT revoked
             RETURNING prefix",
        )
        .bind(&org_id.0)
        .bind(&user_id.0)
        .fetch_all(&mut *transaction)
        .await
        .map_err(sqlx_err)?;
        sqlx::query(
            "INSERT INTO api_tokens
                (id, prefix, secret_hash, org_id, user_id, role_id, name,
                 expires_at_micros, last_used_at_micros, revoked, created_at_micros,
                 is_default, plaintext_sealed, plaintext_nonce, token_kind, application_id)
             VALUES ($1, $2, $3, $4, $5, $6, 'Default ingestion token',
                     NULL, NULL, FALSE, $7, TRUE, $8, $9, 'default_ingestion', NULL)",
        )
        .bind(&id.0)
        .bind(&prefix)
        .bind(&secret_hash)
        .bind(&org_id.0)
        .bind(&user_id.0)
        .bind(&role_id.0)
        .bind(now.0)
        .bind(&sealed)
        .bind(&nonce)
        .execute(&mut *transaction)
        .await
        .map_err(sqlx_err)?;
        transaction.commit().await.map_err(sqlx_err)?;
        self.invalidate_prefixes(revoked_prefixes).await;
        Ok(ManagedApiToken {
            id,
            prefix,
            token: plaintext,
            role_id: role_id.clone(),
            token_kind: ApiTokenKind::DefaultIngestion,
            application_id: None,
            created_at: now,
        })
    }

    async fn ensure_rum_client(
        &self,
        org_id: &Id,
        user_id: &Id,
        role_id: &Id,
        application_id: &str,
    ) -> Result<ManagedApiToken> {
        let cipher = self.cipher()?;
        let lock_key = format!("rum-client-token:{}:{application_id}", org_id.0);
        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        let existing = sqlx::query(
            "SELECT id, prefix, role_id, created_at_micros, plaintext_sealed, plaintext_nonce
             FROM api_tokens
             WHERE org_id = $1 AND application_id = $2
               AND token_kind = 'rum_client' AND NOT revoked
             LIMIT 1",
        )
        .bind(&org_id.0)
        .bind(application_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(sqlx_err)?;
        if let Some(row) = existing
            && let Some(token) = open_managed_token(
                cipher,
                row,
                role_id,
                ApiTokenKind::RumClient,
                Some(application_id.to_string()),
            )?
        {
            sqlx::query("UPDATE api_tokens SET role_id = $2 WHERE id = $1")
                .bind(&token.id.0)
                .bind(&role_id.0)
                .execute(&mut *transaction)
                .await
                .map_err(sqlx_err)?;
            transaction.commit().await.map_err(sqlx_err)?;
            self.tokens.invalidate(&token.prefix).await;
            return Ok(token);
        }

        let revoked_prefixes = sqlx::query_scalar::<String>(
            "UPDATE api_tokens SET revoked = TRUE
             WHERE org_id = $1 AND application_id = $2
               AND token_kind = 'rum_client' AND NOT revoked
             RETURNING prefix",
        )
        .bind(&org_id.0)
        .bind(application_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(sqlx_err)?;
        let (prefix, secret) = generate_token_parts();
        let plaintext = assemble_rum_token(&prefix, &secret);
        let secret_hash = hash_rum_secret(&secret);
        let (nonce, sealed) = cipher
            .seal(plaintext.as_bytes())
            .map_err(|error| Error::internal(format!("RUM client token seal: {error}")))?;
        let id = Id::new();
        let now = TimestampMicros::now();
        sqlx::query(
            "INSERT INTO api_tokens
                (id, prefix, secret_hash, org_id, user_id, role_id, name,
                 expires_at_micros, last_used_at_micros, revoked, created_at_micros,
                 is_default, plaintext_sealed, plaintext_nonce, token_kind, application_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7,
                     NULL, NULL, FALSE, $8, FALSE, $9, $10, 'rum_client', $11)",
        )
        .bind(&id.0)
        .bind(&prefix)
        .bind(&secret_hash)
        .bind(&org_id.0)
        .bind(&user_id.0)
        .bind(&role_id.0)
        .bind(format!("RUM client · {application_id}"))
        .bind(now.0)
        .bind(&sealed)
        .bind(&nonce)
        .bind(application_id)
        .execute(&mut *transaction)
        .await
        .map_err(sqlx_err)?;
        transaction.commit().await.map_err(sqlx_err)?;
        self.invalidate_prefixes(revoked_prefixes).await;
        Ok(ManagedApiToken {
            id,
            prefix,
            token: plaintext,
            role_id: role_id.clone(),
            token_kind: ApiTokenKind::RumClient,
            application_id: Some(application_id.to_string()),
            created_at: now,
        })
    }
}

impl PgApiTokenRepository {
    async fn invalidate_prefixes(&self, prefixes: Vec<String>) {
        for prefix in prefixes {
            self.tokens.invalidate(&prefix).await;
            self.last_used_writes.remove(&prefix);
        }
    }
}

fn open_managed_token(
    cipher: &CipherRootKey,
    row: sqlx::postgres::PgRow,
    role_id: &Id,
    token_kind: ApiTokenKind,
    application_id: Option<String>,
) -> Result<Option<ManagedApiToken>> {
    let sealed = row
        .try_get::<Option<Vec<u8>>, _>("plaintext_sealed")
        .ok()
        .flatten();
    let nonce = row
        .try_get::<Option<Vec<u8>>, _>("plaintext_nonce")
        .ok()
        .flatten();
    let (Some(sealed), Some(nonce)) = (sealed, nonce) else {
        return Ok(None);
    };
    let plaintext = cipher
        .open(&nonce, &sealed)
        .map_err(|_| Error::internal("managed API token decrypt failed"))?;
    let token = String::from_utf8(plaintext)
        .map_err(|_| Error::internal("managed API token is not UTF-8"))?;
    Ok(Some(ManagedApiToken {
        id: Id(row.try_get::<String, _>("id").unwrap_or_default()),
        prefix: row.try_get::<String, _>("prefix").unwrap_or_default(),
        token,
        role_id: role_id.clone(),
        token_kind,
        application_id,
        created_at: TimestampMicros(
            row.try_get::<i64, _>("created_at_micros")
                .unwrap_or_default(),
        ),
    }))
}
