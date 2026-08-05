// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 元数据库实现：sqlx + Postgres。
//!
//! - [`MetaStore`] 持有 `sqlx::PgPool`，启动期通过嵌入式迁移器跑迁移。
//! - 各 repository 在 [`repositories`] 中按 domain trait 实现。
//!
//! 表结构定义在仓库 `src/infra/migrations/20260101000001_initial.sql`，编译期 embed。
//! ID 列统一 `varchar(64)`（KSUID/UUID 字符串），时间戳列统一 `BIGINT`（微秒，
//! 直对 `shared::time::TimestampMicros(i64)`），复杂结构走 `JSONB`。

pub mod pool;
pub mod repositories;

pub use pool::MetaStore;

/// sqlx 错误 → `crate::shared::Error` 的统一映射。
pub(crate) fn sqlx_err(e: sqlx::Error) -> crate::shared::Error {
    use sqlx::Error as SE;

    use crate::shared::Error;
    match &e {
        SE::RowNotFound => Error::not_found("row not found"),
        SE::Database(db_err) => {
            // Postgres SQLSTATE 23505 = unique_violation
            if db_err.code().as_deref() == Some("23505") {
                Error::conflict(db_err.message().to_string())
            } else {
                Error::internal(e.to_string())
            }
        }
        _ => Error::internal(e.to_string()),
    }
}
