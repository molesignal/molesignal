// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 元数据库实现：sqlx + Postgres。
//!
//! - [`MetaStore`] 持有 `sqlx::PgPool`，启动期通过嵌入式迁移器跑迁移。
//! - 各 repository 在 [`repositories`] 中按 domain trait 实现。
//!
//! 表结构定义在 `crates/engines/postgres/src/migrations/`，编译期 embed。
//! ID 列统一 `varchar(64)`（KSUID/UUID 字符串），时间戳列统一 `BIGINT`（微秒，
//! 直对 `shared::time::TimestampMicros(i64)`），复杂结构走 `JSONB`。

pub mod pool;
pub mod repositories;

pub use pool::MetaStore;

/// The `kernel::Error` of an sqlx error.
///
/// A database error maps by its SQLSTATE: a row the change runs into is a conflict (409), a value
/// the table refuses is an invalid argument (400), and what trying again can fix is unavailable
/// (503). The rest is internal (500).
pub fn sqlx_err(e: sqlx::Error) -> kernel::Error {
    use kernel::Error;
    use sqlx::Error as SE;

    match &e {
        SE::RowNotFound => Error::not_found("row not found"),
        SE::Database(db_err) => {
            let message = db_err.message().to_string();

            match db_err.code().as_deref().unwrap_or_default() {
                // unique_violation: the row is there already. foreign_key_violation: another row
                // still refers to the one being deleted, or the row names one that is not there.
                // raise_exception: a trigger refused the change and says why.
                "23505" | "23503" | "P0001" => Error::conflict(message),
                // check_violation, not_null_violation, and the data exceptions (class 22): a
                // value that is too long, out of range or not of the column's type.
                code if code == "23514" || code == "23502" || code.starts_with("22") => {
                    Error::invalid(message)
                }
                // serialization_failure, deadlock_detected, lock_not_available, query_canceled
                // (a statement timeout), and the connection and resource classes: the same
                // request can succeed a moment later.
                code if matches!(code, "40001" | "40P01" | "55P03" | "57014")
                    || code.starts_with("08")
                    || code.starts_with("53")
                    || code.starts_with("57P") =>
                {
                    Error::unavailable(format!("metadata database: {message}"))
                }
                _ => Error::internal(e.to_string()),
            }
        }
        // No connection within the acquire timeout, or the connection broke: the database is busy
        // or out of reach for a moment. Trying again later works; the request itself is fine.
        SE::PoolTimedOut | SE::PoolClosed | SE::WorkerCrashed | SE::Io(_) | SE::Tls(_) => {
            Error::unavailable(format!("no metadata database connection: {e}"))
        }
        _ => Error::internal(e.to_string()),
    }
}

/// Whether the database refused a row because a unique key holds it already (SQLSTATE 23505).
pub fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.code().as_deref() == Some("23505"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Violation(&'static str);

    impl std::fmt::Display for Violation {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "violation {}", self.0)
        }
    }

    impl std::error::Error for Violation {}

    impl sqlx::error::DatabaseError for Violation {
        fn message(&self) -> &str {
            "still referenced"
        }

        fn code(&self) -> Option<std::borrow::Cow<'_, str>> {
            Some(self.0.into())
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }
    }

    #[test]
    fn database_errors_map_by_their_sqlstate() {
        for (code, status) in [
            ("23505", 409),
            ("23503", 409),
            ("P0001", 409),
            ("23514", 400),
            ("23502", 400),
            ("22001", 400),
            ("22P02", 400),
            ("40001", 503),
            ("40P01", 503),
            ("57014", 503),
            ("08006", 503),
            ("53300", 503),
            ("57P01", 503),
            ("42P01", 500),
            ("XX000", 500),
        ] {
            let error = sqlx_err(sqlx::Error::Database(Box::new(Violation(code))));

            assert_eq!(error.http_status_code(), status, "{code}");
        }
    }

    #[test]
    fn only_a_unique_key_makes_a_duplicate() {
        assert!(is_unique_violation(&sqlx::Error::Database(Box::new(
            Violation("23505")
        ))));

        for code in ["23503", "P0001"] {
            assert!(!is_unique_violation(&sqlx::Error::Database(Box::new(
                Violation(code)
            ))));
        }
    }

    #[test]
    fn a_broken_connection_is_unavailable() {
        let error = sqlx_err(sqlx::Error::Io(std::io::Error::from(
            std::io::ErrorKind::ConnectionReset,
        )));

        assert_eq!(error.http_status_code(), 503);
    }

    #[test]
    fn a_busy_pool_is_unavailable_not_an_internal_error() {
        let error = sqlx_err(sqlx::Error::PoolTimedOut);

        assert_eq!(error.http_status_code(), 503);
        assert!(error.to_string().contains("pool timed out"), "{error}");
    }
}
