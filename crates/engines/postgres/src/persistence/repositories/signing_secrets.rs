// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `signing_secrets` 表 Pg 实装（auth-hardening signing-secrets）。
//!
//! JWT signing key 启动期 bootstrap → DB primary；rotate 通过把老 primary
//! 标 retired + 插入新 primary 完成（24h 内老 secret 仍参与 verify）。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningSecret {
    pub id: Id,
    /// 'jwt' | 'cookie' | ... 按用途 namespace
    pub kind: String,
    /// 永不出库 — handler 必须 mask
    #[serde(skip_serializing)]
    pub secret: Vec<u8>,
    pub is_primary: bool,
    pub created_at: TimestampMicros,
    pub retired_at: Option<TimestampMicros>,
}

#[async_trait]
pub trait SigningSecretRepository: Send + Sync {
    /// 当前 primary（kind 仅一行 is_primary=TRUE）。
    async fn get_primary(&self, kind: &str) -> Result<Option<SigningSecret>>;
    /// 所有"active"行：primary + 未 retire 或 retired_at > now - window。
    async fn list_active(
        &self,
        kind: &str,
        window_micros: i64,
        now: TimestampMicros,
    ) -> Result<Vec<SigningSecret>>;
    /// 上架新 primary：把现有 primary 标 retired，插入新 primary。
    /// 返回新插入的 id。
    async fn rotate(&self, kind: &str, new_secret: &[u8]) -> Result<Id>;
    /// 首启动用：插入第一个 primary（要求 kind 表内无 primary 行）。
    async fn insert_primary_if_absent(&self, kind: &str, secret: &[u8]) -> Result<Id>;
    /// 强制上架 override（CI / 多副本锁同一 secret 场景）。等价 `rotate` 但 secret 来自外部。
    async fn upsert_override_primary(&self, kind: &str, secret: &[u8]) -> Result<Id>;
    async fn list_metadata(&self, kind: &str) -> Result<Vec<SigningSecret>>;
}

pub struct PgSigningSecretRepository {
    pool: PgPool,
}

impl PgSigningSecretRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, kind, secret, is_primary, created_at_micros, retired_at_micros";

fn row_to(r: sqlx::postgres::PgRow) -> SigningSecret {
    SigningSecret {
        id: Id(r.try_get::<String, _>("id").unwrap_or_default()),
        kind: r.try_get::<String, _>("kind").unwrap_or_default(),
        secret: r.try_get::<Vec<u8>, _>("secret").unwrap_or_default(),
        is_primary: r.try_get::<bool, _>("is_primary").unwrap_or(false),
        created_at: TimestampMicros(r.try_get::<i64, _>("created_at_micros").unwrap_or_default()),
        retired_at: r
            .try_get::<Option<i64>, _>("retired_at_micros")
            .unwrap_or_default()
            .map(TimestampMicros),
    }
}

#[async_trait]
impl SigningSecretRepository for PgSigningSecretRepository {
    async fn get_primary(&self, kind: &str) -> Result<Option<SigningSecret>> {
        let sql = format!(
            "SELECT {COLS} FROM signing_secrets WHERE kind = $1 AND is_primary = TRUE LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(kind)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(row.map(row_to))
    }

    async fn list_active(
        &self,
        kind: &str,
        window_micros: i64,
        now: TimestampMicros,
    ) -> Result<Vec<SigningSecret>> {
        let cutoff = now.0 - window_micros;
        let sql = format!(
            "SELECT {COLS} FROM signing_secrets
             WHERE kind = $1
               AND (retired_at_micros IS NULL OR retired_at_micros > $2)
             ORDER BY is_primary DESC, created_at_micros DESC"
        );
        let rows = sqlx::query(&sql)
            .bind(kind)
            .bind(cutoff)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "signing_secrets")
    )]
    async fn rotate(&self, kind: &str, new_secret: &[u8]) -> Result<Id> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let now = TimestampMicros::now();
        sqlx::query(
            "UPDATE signing_secrets
             SET is_primary = FALSE, retired_at_micros = $2
             WHERE kind = $1 AND is_primary = TRUE",
        )
        .bind(kind)
        .bind(now.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        let new_id = Id::new();
        sqlx::query(
            "INSERT INTO signing_secrets
                (id, kind, secret, is_primary, created_at_micros)
             VALUES ($1, $2, $3, TRUE, $4)",
        )
        .bind(&new_id.0)
        .bind(kind)
        .bind(new_secret)
        .bind(now.0)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(new_id)
    }

    async fn insert_primary_if_absent(&self, kind: &str, secret: &[u8]) -> Result<Id> {
        let id = Id::new();
        let now = TimestampMicros::now();
        // partial unique index uq_signing_primary 保护并发抢插
        let res = sqlx::query(
            "INSERT INTO signing_secrets
                (id, kind, secret, is_primary, created_at_micros)
             VALUES ($1, $2, $3, TRUE, $4)",
        )
        .bind(&id.0)
        .bind(kind)
        .bind(secret)
        .bind(now.0)
        .execute(&self.pool)
        .await;
        match res {
            Ok(_) => Ok(id),
            Err(e) => {
                // Postgres SQLSTATE 23505 = unique_violation
                if let Some(db_err) = e.as_database_error()
                    && db_err.code().as_deref() == Some("23505")
                {
                    return Err(Error::conflict(format!(
                        "signing_secret primary for kind={kind} already exists"
                    )));
                }
                Err(sqlx_err(e))
            }
        }
    }

    async fn upsert_override_primary(&self, kind: &str, secret: &[u8]) -> Result<Id> {
        // 若已有同 secret 的 primary 直接复用；否则 rotate 引入
        if let Some(existing) = self.get_primary(kind).await?
            && existing.secret == secret
        {
            return Ok(existing.id);
        }
        self.rotate(kind, secret).await
    }

    async fn list_metadata(&self, kind: &str) -> Result<Vec<SigningSecret>> {
        // 与 list_active 类似但**不返 secret**（handler 直接 to_resp 排除）
        let sql = format!(
            "SELECT {COLS} FROM signing_secrets WHERE kind = $1
             ORDER BY is_primary DESC, created_at_micros DESC"
        );
        let rows = sqlx::query(&sql)
            .bind(kind)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(rows.into_iter().map(row_to).collect())
    }
}

/// rotate window 默认 24h（spec auth-hardening Multi-secret verify）。
pub const DEFAULT_RETIRE_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

// ===========================================================================
// Bootstrap / rotate helpers（auth-hardening）
//
// 这两个函数本应放 `src/app/iam/signing.rs`（spec design 写法），
// 但当前 workspace 依赖方向是 infra → app（infra 反向引 app）；放 app 会引
// 反向 dep。折中：与 trait/PgImpl 同 file，调用方 wire / route 已经依赖 infra。
// ===========================================================================

/// 启动期解析 JWT signing secret，返回 `(active_secrets, primary_kid)`。
///
/// 顺序：
/// 1. `MS_AUTH_JWT_SECRET_OVERRIDE` env 有值 → upsert primary
/// 2. DB 已有 primary → 复用
/// 3. 都没 → `OsRng` 32B 生成 + insert_primary_if_absent + info 日志
///    （race：另一个 worker 抢先 insert 时，本端捕获 23505 后 re-read 复用）
///
/// `active_secrets[0]` 一定是 primary；后续是 retire window 内的 secret（仅 verify）。
pub async fn bootstrap_or_load_jwt_secret(
    repo: &dyn SigningSecretRepository,
    env_override_b64: Option<&str>,
) -> Result<(Vec<Vec<u8>>, String)> {
    // 1) env override
    if let Some(raw) = env_override_b64
        && !raw.trim().is_empty()
    {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw.trim())
            .map_err(|e| Error::invalid(format!("MS_AUTH_JWT_SECRET_OVERRIDE b64: {e}")))?;
        if bytes.len() < 32 {
            return Err(Error::invalid(
                "MS_AUTH_JWT_SECRET_OVERRIDE must be at least 32 bytes after base64 decode",
            ));
        }
        let id = repo.upsert_override_primary("jwt", &bytes).await?;
        tracing::info!(
            kid = %id.0,
            "jwt signing secret loaded from MS_AUTH_JWT_SECRET_OVERRIDE"
        );
        return load_active(repo, id.0).await;
    }

    // 2) DB primary
    if let Some(existing) = repo.get_primary("jwt").await? {
        tracing::debug!(kid = %existing.id.0, "jwt signing secret loaded from DB primary");
        return load_active(repo, existing.id.0).await;
    }

    // 3) 首启动生成
    let secret = generate_csprng_32()?;
    let res = repo.insert_primary_if_absent("jwt", &secret).await;
    let kid = match res {
        Ok(id) => {
            tracing::info!(
                kid = %id.0,
                "first-run: generated new JWT signing secret and persisted to DB"
            );
            id.0
        }
        Err(Error::Conflict(_)) => {
            tracing::warn!(
                "race during jwt bootstrap; another worker won the INSERT, re-reading primary"
            );
            let winner = repo
                .get_primary("jwt")
                .await?
                .ok_or_else(|| Error::internal("primary missing after unique-violation"))?;
            winner.id.0
        }
        Err(e) => return Err(e),
    };
    load_active(repo, kid).await
}

async fn load_active(
    repo: &dyn SigningSecretRepository,
    primary_kid: String,
) -> Result<(Vec<Vec<u8>>, String)> {
    let now = TimestampMicros::now();
    let mut all = repo
        .list_active("jwt", DEFAULT_RETIRE_WINDOW_MICROS, now)
        .await?;
    all.sort_by_key(|s| std::cmp::Reverse(s.is_primary));
    let secrets: Vec<Vec<u8>> = all.into_iter().map(|s| s.secret).collect();
    if secrets.is_empty() {
        return Err(Error::internal(
            "jwt active secret set empty after bootstrap",
        ));
    }
    Ok((secrets, primary_kid))
}

fn generate_csprng_32() -> Result<Vec<u8>> {
    use rand::TryRng as _;
    let mut buf = [0u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut buf)
        .map_err(|e| Error::internal(format!("csprng: {e}")))?;
    Ok(buf.to_vec())
}

/// rotate 路径：retire 当前 primary + insert 新 primary + 重新拉 active set。
/// 返回 `(new_active_secrets, new_kid, retired_kid)`。
pub async fn rotate_jwt_secret(
    repo: &dyn SigningSecretRepository,
) -> Result<(Vec<Vec<u8>>, String, Option<String>)> {
    let retired = repo.get_primary("jwt").await?.map(|s| s.id.0);
    let new_secret = generate_csprng_32()?;
    let new_id = repo.rotate("jwt", &new_secret).await?;
    let (active, primary_kid) = load_active(repo, new_id.0.clone()).await?;
    Ok((active, primary_kid, retired))
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use parking_lot::Mutex;

    use super::*;

    #[derive(Default)]
    struct MockRepo {
        primary: Mutex<Option<SigningSecret>>,
        retired: Mutex<Vec<SigningSecret>>,
    }

    #[async_trait]
    impl SigningSecretRepository for MockRepo {
        async fn get_primary(&self, _: &str) -> Result<Option<SigningSecret>> {
            Ok(self.primary.lock().clone())
        }
        async fn list_active(
            &self,
            _: &str,
            _: i64,
            _: TimestampMicros,
        ) -> Result<Vec<SigningSecret>> {
            let mut out = Vec::new();
            if let Some(p) = self.primary.lock().clone() {
                out.push(p);
            }
            out.extend(self.retired.lock().clone());
            Ok(out)
        }
        async fn rotate(&self, _: &str, new: &[u8]) -> Result<Id> {
            let mut primary = self.primary.lock();
            if let Some(old) = primary.take() {
                self.retired.lock().push(SigningSecret {
                    is_primary: false,
                    retired_at: Some(TimestampMicros::now()),
                    ..old
                });
            }
            let id = Id::new();
            *primary = Some(SigningSecret {
                id: id.clone(),
                kind: "jwt".into(),
                secret: new.to_vec(),
                is_primary: true,
                created_at: TimestampMicros::now(),
                retired_at: None,
            });
            Ok(id)
        }
        async fn insert_primary_if_absent(&self, _: &str, secret: &[u8]) -> Result<Id> {
            let mut primary = self.primary.lock();
            if primary.is_some() {
                return Err(Error::conflict("primary exists"));
            }
            let id = Id::new();
            *primary = Some(SigningSecret {
                id: id.clone(),
                kind: "jwt".into(),
                secret: secret.to_vec(),
                is_primary: true,
                created_at: TimestampMicros::now(),
                retired_at: None,
            });
            Ok(id)
        }
        async fn upsert_override_primary(&self, kind: &str, secret: &[u8]) -> Result<Id> {
            if let Some(p) = self.primary.lock().clone()
                && p.secret == secret
            {
                return Ok(p.id);
            }
            self.rotate(kind, secret).await
        }
        async fn list_metadata(&self, _: &str) -> Result<Vec<SigningSecret>> {
            self.list_active("jwt", DEFAULT_RETIRE_WINDOW_MICROS, TimestampMicros::now())
                .await
        }
    }

    #[tokio::test]
    async fn bootstrap_fresh_db_generates() {
        let repo = MockRepo::default();
        let (secrets, kid) = bootstrap_or_load_jwt_secret(&repo, None).await.unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].len(), 32);
        assert!(!kid.is_empty());
    }

    #[tokio::test]
    async fn bootstrap_existing_primary_reused() {
        let repo = MockRepo::default();
        let (_, kid1) = bootstrap_or_load_jwt_secret(&repo, None).await.unwrap();
        let (_, kid2) = bootstrap_or_load_jwt_secret(&repo, None).await.unwrap();
        assert_eq!(kid1, kid2, "second bootstrap must reuse primary");
    }

    #[tokio::test]
    async fn env_override_upserts() {
        use base64::Engine as _;
        let repo = MockRepo::default();
        let override_b64 = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
        let (secrets, _) = bootstrap_or_load_jwt_secret(&repo, Some(&override_b64))
            .await
            .unwrap();
        assert_eq!(secrets[0], vec![7u8; 32]);
    }

    #[tokio::test]
    async fn rotate_adds_retired_to_active_set() {
        let repo = MockRepo::default();
        bootstrap_or_load_jwt_secret(&repo, None).await.unwrap();
        let (active, _, retired) = rotate_jwt_secret(&repo).await.unwrap();
        assert_eq!(active.len(), 2); // primary + 1 retired in 24h window
        assert!(retired.is_some());
    }
}
