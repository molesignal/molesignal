// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `signing_secrets` 表 Pg 实装（auth-hardening signing-secrets）。
//!
//! JWT signing key 启动期 bootstrap → DB primary；rotate 通过把老 primary
//! 标 retired + 插入新 primary 完成（24h 内老 secret 仍参与 verify）。
//!
//! 库里只有根密钥（[`CipherRootKey`]）加密后的密文，读写时在这里加解密，调用方拿到的都是原文。
//! 换根密钥时 `kek_rewrap` 会把它们一起重包。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    cipher: CipherRootKey,
}

impl PgSigningSecretRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }
}

const COLS: &str = "id, kind, secret_nonce, secret_ciphertext, is_primary, created_at, retired_at";

fn row_to(r: sqlx::postgres::PgRow, cipher: &CipherRootKey) -> Result<SigningSecret> {
    let id = Id(r.try_get::<String, _>("id").unwrap_or_default());
    let nonce: Vec<u8> = r.try_get("secret_nonce").map_err(sqlx_err)?;
    let ciphertext: Vec<u8> = r.try_get("secret_ciphertext").map_err(sqlx_err)?;
    let secret = open_secret(cipher, &id, &nonce, &ciphertext)?;

    Ok(SigningSecret {
        id,
        kind: r.try_get::<String, _>("kind").unwrap_or_default(),
        secret,
        is_primary: r.try_get::<bool, _>("is_primary").unwrap_or(false),
        created_at: r
            .try_get::<TimestampMicros, _>("created_at")
            .unwrap_or_default(),
        retired_at: r
            .try_get::<Option<TimestampMicros>, _>("retired_at")
            .unwrap_or_default(),
    })
}

/// 用根密钥加密，返回 `(nonce, ciphertext)`。
fn seal_secret(cipher: &CipherRootKey, secret: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    cipher
        .seal(secret)
        .map_err(|e| Error::internal(format!("seal signing secret: {e}")))
}

/// 解不开时报错只点名这一行，不带密文。多半是这个节点的 `MS_CIPHER_KEY` 不是加密时用的那个。
fn open_secret(
    cipher: &CipherRootKey,
    id: &Id,
    nonce: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>> {
    cipher.open(nonce, ciphertext).map_err(|e| {
        Error::internal(format!(
            "open signing secret {} with the cipher root key (MS_CIPHER_KEY): {e}",
            id.0
        ))
    })
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

        row.map(|r| row_to(r, &self.cipher)).transpose()
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
               AND (retired_at IS NULL OR retired_at > $2)
             ORDER BY is_primary DESC, created_at DESC"
        );

        let rows = sqlx::query(&sql)
            .bind(kind)
            .bind(TimestampMicros(cutoff))
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        rows.into_iter().map(|r| row_to(r, &self.cipher)).collect()
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "signing_secrets")
    )]
    async fn rotate(&self, kind: &str, new_secret: &[u8]) -> Result<Id> {
        let (nonce, ciphertext) = seal_secret(&self.cipher, new_secret)?;
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let now = TimestampMicros::now();

        sqlx::query(
            "UPDATE signing_secrets
             SET is_primary = FALSE, retired_at = $2
             WHERE kind = $1 AND is_primary = TRUE",
        )
        .bind(kind)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let new_id = Id::new();

        sqlx::query(
            "INSERT INTO signing_secrets
                (id, kind, secret_nonce, secret_ciphertext, is_primary, created_at)
             VALUES ($1, $2, $3, $4, TRUE, $5)",
        )
        .bind(&new_id.0)
        .bind(kind)
        .bind(&nonce)
        .bind(&ciphertext)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(new_id)
    }

    async fn insert_primary_if_absent(&self, kind: &str, secret: &[u8]) -> Result<Id> {
        let (nonce, ciphertext) = seal_secret(&self.cipher, secret)?;
        let id = Id::new();
        let now = TimestampMicros::now();

        // partial unique index uq_signing_primary 保护并发抢插
        let res = sqlx::query(
            "INSERT INTO signing_secrets
                (id, kind, secret_nonce, secret_ciphertext, is_primary, created_at)
             VALUES ($1, $2, $3, $4, TRUE, $5)",
        )
        .bind(&id.0)
        .bind(kind)
        .bind(&nonce)
        .bind(&ciphertext)
        .bind(now)
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
             ORDER BY is_primary DESC, created_at DESC"
        );

        let rows = sqlx::query(&sql)
            .bind(kind)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        rows.into_iter().map(|r| row_to(r, &self.cipher)).collect()
    }
}

/// rotate window 默认 24h（spec auth-hardening Multi-secret verify）。
pub const DEFAULT_RETIRE_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

/// 宽限期到了之后再多留的时间。别的节点读到轮换之前还会用旧 secret 签发，那些 token 比宽限期晚
/// 结束这段延迟，校验时还有 60 秒的时钟容差，节点之间的时钟也可能有偏差。留够这些，有效期不超过
/// 24 小时的 token 都能用到自己过期。
const RETIRE_WINDOW_MARGIN_MICROS: i64 = 5 * 60 * 1_000_000;

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
    Ok((load_active_jwt_secrets(repo).await?, primary_kid))
}

/// 现在参与校验的 JWT secret：`[0]` 是 primary，其余是宽限期内的 retired secret。节点读到就换上，
/// 所以没有 primary 或者 secret 是空的都报错，不让节点拿它去签发。
pub async fn load_active_jwt_secrets(repo: &dyn SigningSecretRepository) -> Result<Vec<Vec<u8>>> {
    let mut all = repo
        .list_active(
            "jwt",
            DEFAULT_RETIRE_WINDOW_MICROS + RETIRE_WINDOW_MARGIN_MICROS,
            TimestampMicros::now(),
        )
        .await?;

    all.sort_by_key(|s| std::cmp::Reverse(s.is_primary));

    if !all.first().is_some_and(|s| s.is_primary) {
        return Err(Error::internal("jwt active secret set has no primary"));
    }

    if all.iter().any(|s| s.secret.is_empty()) {
        return Err(Error::internal("jwt active secret set has an empty secret"));
    }

    Ok(all.into_iter().map(|s| s.secret).collect())
}

fn generate_csprng_32() -> Result<Vec<u8>> {
    use rand::TryRng as _;
    let mut buf = [0u8; 32];

    rand::rngs::SysRng
        .try_fill_bytes(&mut buf)
        .map_err(|e| Error::internal(format!("csprng: {e}")))?;

    Ok(buf.to_vec())
}

/// rotate 路径：retire 当前 primary + insert 新 primary。返回 `(new_kid, retired_kid)`。
pub async fn rotate_jwt_secret(
    repo: &dyn SigningSecretRepository,
) -> Result<(String, Option<String>)> {
    let retired = repo.get_primary("jwt").await?.map(|s| s.id.0);
    let new_secret = generate_csprng_32()?;
    let new_id = repo.rotate("jwt", &new_secret).await?;

    Ok((new_id.0, retired))
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
            window_micros: i64,
            now: TimestampMicros,
        ) -> Result<Vec<SigningSecret>> {
            let cutoff = now.0 - window_micros;
            let mut out = Vec::new();
            if let Some(p) = self.primary.lock().clone() {
                out.push(p);
            }
            out.extend(
                self.retired
                    .lock()
                    .iter()
                    .filter(|s| s.retired_at.is_some_and(|at| at.0 > cutoff))
                    .cloned(),
            );
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
        let (before, first_kid) = bootstrap_or_load_jwt_secret(&repo, None).await.unwrap();
        let (new_kid, retired) = rotate_jwt_secret(&repo).await.unwrap();
        let active = load_active_jwt_secrets(&repo).await.unwrap();

        assert_eq!(active.len(), 2); // primary + 1 retired in 24h window
        assert_eq!(active[1], before[0]);
        assert_ne!(active[0], before[0], "the new primary comes first");
        assert_ne!(new_kid, first_kid);
        assert_eq!(retired, Some(first_kid));
    }

    /// A retired secret that retired `age_micros` ago, beside the primary.
    async fn with_retired_secret_of_age(age_micros: i64) -> MockRepo {
        let repo = MockRepo::default();
        repo.insert_primary_if_absent("jwt", &[1u8; 32])
            .await
            .unwrap();
        repo.retired.lock().push(SigningSecret {
            id: Id::new(),
            kind: "jwt".into(),
            secret: vec![2u8; 32],
            is_primary: false,
            created_at: TimestampMicros(0),
            retired_at: Some(TimestampMicros(TimestampMicros::now().0 - age_micros)),
        });
        repo
    }

    #[tokio::test]
    async fn a_retired_secret_outlasts_every_token_a_lagging_node_can_sign_with_it() {
        // The longest token a node signs lasts a day from now, however late it read the rotation,
        // and `jsonwebtoken` accepts it for another minute. A node re-reads every ten seconds.
        const LAG_AND_LEEWAY_MICROS: i64 = (10 + 60) * 1_000_000;

        let repo =
            with_retired_secret_of_age(DEFAULT_RETIRE_WINDOW_MICROS + LAG_AND_LEEWAY_MICROS).await;

        assert_eq!(load_active_jwt_secrets(&repo).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_retired_secret_leaves_when_the_window_and_its_margin_are_over() {
        let repo = with_retired_secret_of_age(
            DEFAULT_RETIRE_WINDOW_MICROS + RETIRE_WINDOW_MARGIN_MICROS + 1_000_000,
        )
        .await;

        assert_eq!(
            load_active_jwt_secrets(&repo).await.unwrap(),
            vec![vec![1u8; 32]]
        );
    }

    #[tokio::test]
    async fn no_node_takes_a_set_without_a_primary() {
        let repo = with_retired_secret_of_age(1_000_000).await;
        *repo.primary.lock() = None;

        assert!(load_active_jwt_secrets(&repo).await.is_err());
    }

    #[tokio::test]
    async fn no_node_takes_a_set_with_an_empty_secret() {
        let repo = MockRepo::default();
        repo.insert_primary_if_absent("jwt", &[]).await.unwrap();

        assert!(load_active_jwt_secrets(&repo).await.is_err());
    }

    fn root_key(byte: u8) -> CipherRootKey {
        use base64::Engine as _;

        let key = base64::engine::general_purpose::STANDARD.encode([byte; 32]);

        CipherRootKey::from_base64(&key).unwrap()
    }

    #[test]
    fn a_stored_secret_opens_only_with_the_root_key_that_sealed_it() {
        let secret = [7u8; 32];
        let id = Id::from_string("kid-1");
        let (nonce, ciphertext) = seal_secret(&root_key(1), &secret).unwrap();

        assert_eq!(
            open_secret(&root_key(1), &id, &nonce, &ciphertext).unwrap(),
            secret
        );

        let error = open_secret(&root_key(2), &id, &nonce, &ciphertext)
            .unwrap_err()
            .to_string();

        assert!(error.contains("kid-1"), "{error}");
        assert!(error.contains("MS_CIPHER_KEY"), "{error}");
    }
}
