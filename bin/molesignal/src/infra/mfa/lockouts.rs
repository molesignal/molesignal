// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 第二步验证输错的次数，和密码登录的次数存在同一张 `signin_attempts`：一个账号一行，键是 `mfa`
//! 加用户 id 的摘要，来源一栏是空串。
//!
//! 知道密码的人可以一次次重新登录，每次再试几个验证码。所以按账号累计输错次数：一个时间窗里
//! 错满就锁一段时间，所有节点看到的是同一份计数。锁上时计数清零，锁过了重新开始一个时间窗。

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

use crate::{
    infra::persistence::sqlx_err,
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 错几次、在多长时间里错，锁多久。
#[derive(Debug, Clone, Copy)]
pub struct LockoutPolicy {
    pub window_micros: i64,
    pub limit: i32,
    pub lock_micros: i64,
}

/// 记下一次输错之后的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// 还没错满。
    Counted,
    /// 这一次刚好错满，锁到 `until`。
    Locked { until: TimestampMicros },
}

/// 一个账号的计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tally {
    failures: i32,
    window_started_at: TimestampMicros,
    locked_until: Option<TimestampMicros>,
}

/// 在 `previous` 之上再错一次。
fn after_failure(
    previous: Option<Tally>,
    now: TimestampMicros,
    policy: &LockoutPolicy,
) -> (Tally, Failure) {
    let open = previous.filter(|tally| tally.window_started_at.0 > now.0 - policy.window_micros);

    let mut tally = match open {
        Some(tally) => Tally {
            failures: tally.failures + 1,
            ..tally
        },
        None => Tally {
            failures: 1,
            window_started_at: now,
            locked_until: previous.and_then(|tally| tally.locked_until),
        },
    };

    if tally.failures < policy.limit {
        return (tally, Failure::Counted);
    }

    let until = TimestampMicros(now.0 + policy.lock_micros);

    tally = Tally {
        failures: 0,
        window_started_at: now,
        locked_until: Some(until),
    };

    (tally, Failure::Locked { until })
}

/// `signin_attempts` 里这个账号那一行的键。前缀和密码登录、发信限额的不同，互不相撞。
pub(super) fn row_key(user_id: &Id) -> String {
    hex::encode(Sha256::digest(
        ["mfa", user_id.as_str()].join("\0").as_bytes(),
    ))
}

/// 时间窗和锁都过去之后，这一行就可以清掉了。
fn expires_at(tally: &Tally, policy: &LockoutPolicy) -> TimestampMicros {
    let window_end = tally
        .window_started_at
        .0
        .saturating_add(policy.window_micros);
    TimestampMicros(
        tally
            .locked_until
            .map_or(window_end, |until| until.0.max(window_end)),
    )
}

#[async_trait]
pub trait MfaLockoutRepository: Send + Sync {
    /// 锁着时返回锁到什么时候。
    async fn locked_until(
        &self,
        user_id: &Id,
        now: TimestampMicros,
    ) -> Result<Option<TimestampMicros>>;
    async fn record_failure(
        &self,
        user_id: &Id,
        now: TimestampMicros,
        policy: &LockoutPolicy,
    ) -> Result<Failure>;
    /// 验证通过后清掉计数。
    async fn clear(&self, user_id: &Id) -> Result<()>;
}

pub struct PgMfaLockoutRepository {
    pool: PgPool,
}

impl PgMfaLockoutRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MfaLockoutRepository for PgMfaLockoutRepository {
    async fn locked_until(
        &self,
        user_id: &Id,
        now: TimestampMicros,
    ) -> Result<Option<TimestampMicros>> {
        sqlx::query_scalar::<TimestampMicros>(
            "SELECT locked_until FROM signin_attempts
             WHERE account_hash = $1 AND source_hash = '' AND locked_until > $2",
        )
        .bind(row_key(user_id))
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)
    }

    async fn record_failure(
        &self,
        user_id: &Id,
        now: TimestampMicros,
        policy: &LockoutPolicy,
    ) -> Result<Failure> {
        let key = row_key(user_id);
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        // 还没有这一行时先占位，并发的第一次输错也不会丢计数。
        sqlx::query(
            "INSERT INTO signin_attempts
                (account_hash, source_hash, failures, window_started_at, expires_at)
             VALUES ($1, '', 0, $2, $2)
             ON CONFLICT (account_hash, source_hash) DO NOTHING",
        )
        .bind(&key)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let previous = sqlx::query(
            "SELECT failures, window_started_at, locked_until
             FROM signin_attempts
             WHERE account_hash = $1 AND source_hash = '' FOR UPDATE",
        )
        .bind(&key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .map(|row| -> Result<Tally> {
            Ok(Tally {
                failures: row.try_get("failures").map_err(sqlx_err)?,
                window_started_at: row.try_get("window_started_at").map_err(sqlx_err)?,
                locked_until: row.try_get("locked_until").map_err(sqlx_err)?,
            })
        })
        .transpose()?;

        let (tally, failure) = after_failure(previous, now, policy);

        sqlx::query(
            "INSERT INTO signin_attempts
                (account_hash, source_hash, failures, window_started_at, locked_until, expires_at)
             VALUES ($1, '', $2, $3, $4, $5)
             ON CONFLICT (account_hash, source_hash) DO UPDATE SET
                failures = EXCLUDED.failures,
                window_started_at = EXCLUDED.window_started_at,
                locked_until = EXCLUDED.locked_until,
                expires_at = EXCLUDED.expires_at",
        )
        .bind(&key)
        .bind(tally.failures)
        .bind(tally.window_started_at)
        .bind(tally.locked_until)
        .bind(expires_at(&tally, policy))
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(failure)
    }

    async fn clear(&self, user_id: &Id) -> Result<()> {
        sqlx::query(
            "DELETE FROM signin_attempts
             WHERE account_hash = $1 AND source_hash = ''
               AND (locked_until IS NULL OR locked_until <= $2)",
        )
        .bind(row_key(user_id))
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: LockoutPolicy = LockoutPolicy {
        window_micros: 100,
        limit: 3,
        lock_micros: 50,
    };

    fn tally(failures: i32, window_started_at: i64) -> Tally {
        Tally {
            failures,
            window_started_at: TimestampMicros(window_started_at),
            locked_until: None,
        }
    }

    #[test]
    fn failures_add_up_within_the_window_and_lock_on_the_limit() {
        let (first, outcome) = after_failure(None, TimestampMicros(1_000), &POLICY);
        assert_eq!(first, tally(1, 1_000));
        assert_eq!(outcome, Failure::Counted);

        let (second, outcome) = after_failure(Some(first), TimestampMicros(1_010), &POLICY);
        assert_eq!(second, tally(2, 1_000));
        assert_eq!(outcome, Failure::Counted);

        let (locked, outcome) = after_failure(Some(second), TimestampMicros(1_020), &POLICY);
        assert_eq!(
            outcome,
            Failure::Locked {
                until: TimestampMicros(1_070)
            }
        );
        assert_eq!(locked.failures, 0);
        assert_eq!(locked.locked_until, Some(TimestampMicros(1_070)));
    }

    #[test]
    fn an_old_window_starts_over() {
        let (fresh, outcome) =
            after_failure(Some(tally(2, 1_000)), TimestampMicros(1_100), &POLICY);

        assert_eq!(fresh, tally(1, 1_100));
        assert_eq!(outcome, Failure::Counted);
    }
}
