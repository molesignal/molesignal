// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 密码登录的次数存在 `signin_attempts`，所有节点看到同一份。规则在 [`super::rules`]。
//!
//! 发信限额（[`SigninAttemptRepository::take_quota`]）用同一张表，键的摘要不同。

use async_trait::async_trait;
use sqlx::{PgPool, Row, TracedTransaction};

use super::rules::{
    self, AttemptKeys, Begun, KNOWN_SOURCE_MICROS, Quota, Refusal, RowKey, Rules, Settlement,
    Tallies, Tally, Written, begin, quota_taken, settled, succeeded_pair, written,
};
use crate::{
    domain::iam::SigninLimits,
    infra::persistence::sqlx_err,
    shared::{Result, time::TimestampMicros},
};

/// 一次顺带清理的过期行数上限。
const PURGE_BATCH: i64 = 100;

#[async_trait]
pub trait SigninAttemptRepository: Send + Sync {
    /// 不加锁看一眼现在拦不拦。来源锁住时不写库，也不排队等行锁。
    async fn refusal(
        &self,
        keys: &AttemptKeys,
        limits: &SigninLimits,
        now: TimestampMicros,
    ) -> Result<Option<Refusal>>;
    /// 记上这次尝试，或者说明为什么拒绝。
    async fn begin(
        &self,
        keys: &AttemptKeys,
        limits: &SigninLimits,
        now: TimestampMicros,
    ) -> Result<Begun>;
    /// 这次尝试不算失败。
    async fn settle(
        &self,
        keys: &AttemptKeys,
        limits: &SigninLimits,
        settlement: Settlement,
        now: TimestampMicros,
    ) -> Result<()>;
    /// 本人重置了密码：清掉这个账号的失败次数和锁，记下这个来源成功过。
    async fn forgive(&self, keys: &AttemptKeys, now: TimestampMicros) -> Result<()>;
    /// 占一次发信限额。用完了回答 `false`，什么都不写。
    async fn take_quota(
        &self,
        keys: &AttemptKeys,
        quota: &Quota,
        now: TimestampMicros,
    ) -> Result<bool>;
}

pub struct PgSigninAttemptRepository {
    pool: PgPool,
}

impl PgSigninAttemptRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 顺带清理过期的行。跳过别人正锁着的行，不和在线请求抢。
    async fn purge_expired(&self, now: TimestampMicros) -> Result<()> {
        sqlx::query(
            "DELETE FROM signin_attempts WHERE (account_hash, source_hash) IN (
                SELECT account_hash, source_hash FROM signin_attempts
                WHERE expires_at <= $1
                ORDER BY expires_at LIMIT $2
                FOR UPDATE SKIP LOCKED)",
        )
        .bind(now)
        .bind(PURGE_BATCH)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}

fn split(rows: &[RowKey]) -> (Vec<&str>, Vec<&str>) {
    rows.iter()
        .map(|(account, source)| (account.as_str(), source.as_str()))
        .unzip()
}

fn tallies_of(keys: &AttemptKeys, rows: Vec<(RowKey, Tally)>) -> Tallies {
    let find = |key: Option<RowKey>| {
        let key = key?;

        rows.iter()
            .find(|(row, _)| *row == key)
            .map(|(_, tally)| *tally)
    };

    Tallies {
        account: find(Some(keys.account_row())),
        source: find(keys.source_row()),
        pair: find(keys.pair_row()),
    }
}

fn read_row(row: &sqlx::postgres::PgRow) -> Result<(RowKey, Tally)> {
    Ok((
        (
            row.try_get("account_hash").map_err(sqlx_err)?,
            row.try_get("source_hash").map_err(sqlx_err)?,
        ),
        Tally {
            failures: row.try_get("failures").map_err(sqlx_err)?,
            window_started_at: row
                .try_get::<TimestampMicros, _>("window_started_at")
                .map_err(sqlx_err)?,
            locked_until: row
                .try_get::<Option<TimestampMicros>, _>("locked_until")
                .map_err(sqlx_err)?,
            last_success_at: row
                .try_get::<Option<TimestampMicros>, _>("last_success_at")
                .map_err(sqlx_err)?,
        },
    ))
}

/// 把这些行锁住读出来。还不存在的行先占位，并发的第一次尝试也不会丢计数。
async fn lock_rows(
    tx: &mut TracedTransaction<'_>,
    keys: &[RowKey],
    now: TimestampMicros,
) -> Result<Vec<(RowKey, Tally)>> {
    let (accounts, sources) = split(keys);

    sqlx::query(
        "INSERT INTO signin_attempts
            (account_hash, source_hash, failures, window_started_at, expires_at)
         SELECT account_hash, source_hash, 0, $3::TIMESTAMPTZ, $3::TIMESTAMPTZ
         FROM UNNEST($1::text[], $2::text[]) AS row_key(account_hash, source_hash)
         ORDER BY account_hash, source_hash
         ON CONFLICT (account_hash, source_hash) DO NOTHING",
    )
    .bind(&accounts)
    .bind(&sources)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    let rows = sqlx::query(
        "SELECT account_hash, source_hash, failures, window_started_at,
                locked_until, last_success_at
         FROM signin_attempts
         WHERE (account_hash, source_hash) IN (SELECT * FROM UNNEST($1::text[], $2::text[]))
         ORDER BY account_hash, source_hash
         FOR UPDATE OF signin_attempts",
    )
    .bind(&accounts)
    .bind(&sources)
    .fetch_all(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    rows.iter().map(read_row).collect()
}

async fn write_rows(tx: &mut TracedTransaction<'_>, rows: &[Written]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let keys: Vec<RowKey> = rows.iter().map(|row| row.key.clone()).collect();
    let (accounts, sources) = split(&keys);
    let failures: Vec<i32> = rows.iter().map(|row| row.tally.failures).collect();

    let windows: Vec<TimestampMicros> =
        rows.iter().map(|row| row.tally.window_started_at).collect();
    let locks: Vec<Option<TimestampMicros>> =
        rows.iter().map(|row| row.tally.locked_until).collect();

    let successes: Vec<Option<TimestampMicros>> =
        rows.iter().map(|row| row.tally.last_success_at).collect();

    let expiries: Vec<TimestampMicros> = rows.iter().map(|row| row.expires_at).collect();

    sqlx::query(
        "INSERT INTO signin_attempts
            (account_hash, source_hash, failures, window_started_at, locked_until,
             last_success_at, expires_at)
         SELECT * FROM UNNEST($1::text[], $2::text[], $3::int4[], $4::timestamptz[],
                              $5::timestamptz[], $6::timestamptz[], $7::timestamptz[])
         ON CONFLICT (account_hash, source_hash) DO UPDATE SET
            failures = EXCLUDED.failures,
            window_started_at = EXCLUDED.window_started_at,
            locked_until = EXCLUDED.locked_until,
            last_success_at = EXCLUDED.last_success_at,
            expires_at = EXCLUDED.expires_at",
    )
    .bind(&accounts)
    .bind(&sources)
    .bind(&failures)
    .bind(&windows)
    .bind(&locks)
    .bind(&successes)
    .bind(&expiries)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

#[async_trait]
impl SigninAttemptRepository for PgSigninAttemptRepository {
    async fn refusal(
        &self,
        keys: &AttemptKeys,
        limits: &SigninLimits,
        now: TimestampMicros,
    ) -> Result<Option<Refusal>> {
        let rows = keys.rows();
        let (accounts, sources) = split(&rows);

        let rows = sqlx::query(
            "SELECT account_hash, source_hash, failures, window_started_at,
                    locked_until, last_success_at
             FROM signin_attempts
             WHERE (account_hash, source_hash) IN (SELECT * FROM UNNEST($1::text[], $2::text[]))",
        )
        .bind(&accounts)
        .bind(&sources)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let rows = rows.iter().map(read_row).collect::<Result<Vec<_>>>()?;

        Ok(rules::refusal(
            &tallies_of(keys, rows),
            &Rules::from(limits),
            now,
        ))
    }

    async fn begin(
        &self,
        keys: &AttemptKeys,
        limits: &SigninLimits,
        now: TimestampMicros,
    ) -> Result<Begun> {
        self.purge_expired(now).await?;

        let rules = Rules::from(limits);
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let before = tallies_of(keys, lock_rows(&mut tx, &keys.rows(), now).await?);
        let (after, begun) = begin(before, keys.source.is_some(), &rules, now);

        write_rows(&mut tx, &written(keys, &before, &after, &rules, now)).await?;
        tx.commit().await.map_err(sqlx_err)?;

        Ok(begun)
    }

    async fn settle(
        &self,
        keys: &AttemptKeys,
        limits: &SigninLimits,
        settlement: Settlement,
        now: TimestampMicros,
    ) -> Result<()> {
        let rules = Rules::from(limits);
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let before = tallies_of(keys, lock_rows(&mut tx, &keys.rows(), now).await?);
        let after = settled(before, keys.source.is_some(), settlement, &rules, now);

        write_rows(&mut tx, &written(keys, &before, &after, &rules, now)).await?;
        tx.commit().await.map_err(sqlx_err)?;

        Ok(())
    }

    async fn forgive(&self, keys: &AttemptKeys, now: TimestampMicros) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query(
            "UPDATE signin_attempts SET failures = 0, locked_until = NULL
             WHERE account_hash = $1",
        )
        .bind(&keys.account)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        if let Some(key) = keys.pair_row() {
            let row = Written {
                key,
                tally: succeeded_pair(now),
                expires_at: TimestampMicros(now.0 + KNOWN_SOURCE_MICROS),
            };

            write_rows(&mut tx, &[row]).await?;
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn take_quota(
        &self,
        keys: &AttemptKeys,
        quota: &Quota,
        now: TimestampMicros,
    ) -> Result<bool> {
        self.purge_expired(now).await?;

        let mut rows = vec![keys.account_row()];
        rows.extend(keys.source_row());
        rows.sort();

        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        let before = tallies_of(keys, lock_rows(&mut tx, &rows, now).await?);
        let expires_at = TimestampMicros(now.0 + quota.window_micros);

        let account = quota_taken(before.account, quota.per_account, quota.window_micros, now);
        let source = quota_taken(before.source, quota.per_source, quota.window_micros, now);
        let source_used_up = keys.source.is_some() && source.is_none();

        let Some(account) = account.filter(|_| !source_used_up) else {
            // 占位的行留着，到期清理；这次不占限额。
            tx.commit().await.map_err(sqlx_err)?;
            return Ok(false);
        };

        let mut rows = vec![Written {
            key: keys.account_row(),
            tally: account,
            expires_at,
        }];

        if let Some((key, tally)) = keys.source_row().zip(source) {
            rows.push(Written {
                key,
                tally,
                expires_at,
            });
        }

        write_rows(&mut tx, &rows).await?;
        tx.commit().await.map_err(sqlx_err)?;

        Ok(true)
    }
}
