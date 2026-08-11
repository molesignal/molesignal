//! Postgres-only SQLx facade.
//!
//! The upstream `sqlx` facade records optional MySQL and SQLite packages in
//! `Cargo.lock`; MySQL currently pulls the vulnerable `rsa` crate even when the
//! feature is disabled. This crate preserves the subset of the `sqlx` facade
//! used by this workspace while depending only on `sqlx-core` and
//! `sqlx-postgres`.

use std::{
    fmt::Write,
    ops::{Deref, DerefMut},
    sync::OnceLock,
    time::Instant,
};

use hmac::{Hmac, Mac};
use sha2::Sha256;
#[cfg(feature = "migrate")]
pub use sqlx_core::migrate;
pub use sqlx_core::{
    Either,
    acquire::Acquire,
    arguments::{Arguments, IntoArguments},
    column::{Column, ColumnIndex},
    connection::{ConnectOptions, Connection},
    database::{self, Database},
    describe::Describe,
    error::{self, Error, Result},
    executor::{Execute, Executor},
    from_row::FromRow,
    pool::{self, Pool},
    query::query_with,
    query_as::query_as_with,
    query_builder::{self, QueryBuilder},
    query_scalar::query_scalar_with,
    raw_sql::{RawSql, raw_sql},
    row::Row,
    statement::Statement,
    transaction::{Transaction, TransactionManager},
    type_info::TypeInfo,
    types::Type,
    value::{Value, ValueRef},
};
pub use sqlx_postgres::{
    self as postgres, PgArguments, PgConnection, PgExecutor, PgPool, PgQueryResult, PgRow,
    PgTransaction, Postgres,
};
use tracing::{Instrument, Span, field};

/// Postgres-only traced query returned by [`query`].
///
/// Keeping this boundary in the local facade guarantees that every ordinary
/// production query is covered without ever exposing SQL text or bind values to
/// Trace attributes.
#[must_use = "query must be executed to affect database"]
pub struct TracedQuery<'q> {
    inner: sqlx_core::query::Query<'q, Postgres, PgArguments>,
    metadata: SqlTraceMetadata,
}

#[must_use = "query must be executed to affect database"]
pub struct TracedQueryAs<'q, O> {
    inner: sqlx_core::query_as::QueryAs<'q, Postgres, O, PgArguments>,
    metadata: SqlTraceMetadata,
}

#[must_use = "query must be executed to affect database"]
pub struct TracedQueryScalar<'q, O> {
    inner: sqlx_core::query_scalar::QueryScalar<'q, Postgres, O, PgArguments>,
    metadata: SqlTraceMetadata,
}

/// PostgreSQL transaction whose pool wait, statements, commit/rollback, and total lifetime share
/// one low-cardinality transaction Span.
#[must_use = "a transaction rolls back when dropped without commit"]
pub struct TracedTransaction<'c> {
    inner: Option<Transaction<'c, Postgres>>,
    span: Span,
    started: Instant,
    finished: bool,
}

/// Acquire a connection and begin an instrumented PostgreSQL transaction.
pub async fn begin(pool: &PgPool) -> Result<TracedTransaction<'static>, Error> {
    let span = tracing::info_span!(
        "db.transaction",
        otel.kind = "client",
        db.system.name = "postgresql",
        db.operation.name = "TRANSACTION",
        molesignal.db.pool_wait_included = true,
        molesignal.db.pool_wait_ms = field::Empty,
        db.transaction.outcome = field::Empty,
        db.transaction.duration_ms = field::Empty,
        error.type = field::Empty,
    );
    let started = Instant::now();
    let inner = match pool.begin().instrument(span.clone()).await {
        Ok(transaction) => transaction,
        Err(error) => {
            span.record(
                "molesignal.db.pool_wait_ms",
                started.elapsed().as_secs_f64() * 1_000.0,
            );
            span.record("db.transaction.outcome", "begin_error");
            span.record(
                "db.transaction.duration_ms",
                started.elapsed().as_secs_f64() * 1_000.0,
            );
            span.record("error.type", sql_error_type(&error));
            return Err(error);
        }
    };
    span.record(
        "molesignal.db.pool_wait_ms",
        started.elapsed().as_secs_f64() * 1_000.0,
    );
    Ok(TracedTransaction {
        inner: Some(inner),
        span,
        started,
        finished: false,
    })
}

impl TracedTransaction<'_> {
    pub async fn commit(mut self) -> Result<(), Error> {
        let inner = self.inner.take().expect("transaction already finished");
        let result = inner.commit().instrument(self.span.clone()).await;
        finish_transaction_span(&self.span, self.started, "committed", result.as_ref().err());
        self.finished = true;
        result
    }

    pub async fn rollback(mut self) -> Result<(), Error> {
        let inner = self.inner.take().expect("transaction already finished");
        let result = inner.rollback().instrument(self.span.clone()).await;
        finish_transaction_span(
            &self.span,
            self.started,
            "rolled_back",
            result.as_ref().err(),
        );
        self.finished = true;
        result
    }
}

impl Deref for TracedTransaction<'_> {
    type Target = PgConnection;

    fn deref(&self) -> &Self::Target {
        self.inner
            .as_ref()
            .expect("transaction already finished")
            .deref()
    }
}

impl DerefMut for TracedTransaction<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
            .as_mut()
            .expect("transaction already finished")
            .deref_mut()
    }
}

impl Drop for TracedTransaction<'_> {
    fn drop(&mut self) {
        if !self.finished && self.inner.is_some() {
            finish_transaction_span(&self.span, self.started, "dropped_rollback", None);
        }
    }
}

/// Construct an instrumented Postgres query.
pub fn query(sql: &str) -> TracedQuery<'_> {
    TracedQuery {
        inner: sqlx_core::query::query::<Postgres>(sql),
        metadata: SqlTraceMetadata::from_sql(sql),
    }
}

pub fn query_as<O>(sql: &str) -> TracedQueryAs<'_, O>
where
    O: for<'row> sqlx_core::from_row::FromRow<'row, PgRow>,
{
    TracedQueryAs {
        inner: sqlx_core::query_as::query_as::<Postgres, O>(sql),
        metadata: SqlTraceMetadata::from_sql(sql),
    }
}

pub fn query_scalar<O>(sql: &str) -> TracedQueryScalar<'_, O>
where
    (O,): for<'row> sqlx_core::from_row::FromRow<'row, PgRow>,
{
    TracedQueryScalar {
        inner: sqlx_core::query_scalar::query_scalar::<Postgres, O>(sql),
        metadata: SqlTraceMetadata::from_sql(sql),
    }
}

impl<'q> TracedQuery<'q> {
    pub fn bind<T>(mut self, value: T) -> Self
    where
        T: 'q + sqlx_core::encode::Encode<'q, Postgres> + sqlx_core::types::Type<Postgres>,
    {
        self.inner = self.inner.bind(value);
        self
    }

    pub fn persistent(mut self, value: bool) -> Self {
        self.inner = self.inner.persistent(value);
        self
    }

    pub async fn execute<'e, 'c: 'e, E>(self, executor: E) -> Result<PgQueryResult, Error>
    where
        'q: 'e,
        E: sqlx_core::executor::Executor<'c, Database = Postgres>,
    {
        let Self { inner, metadata } = self;
        let span = metadata.span();
        let result = executor.execute(inner).instrument(span.clone()).await;
        finish_query_span(
            &span,
            result.as_ref().ok().map(|result| result.rows_affected()),
            None,
            result.as_ref().err(),
        );
        result
    }

    pub async fn fetch_all<'e, 'c: 'e, E>(self, executor: E) -> Result<Vec<PgRow>, Error>
    where
        'q: 'e,
        E: sqlx_core::executor::Executor<'c, Database = Postgres>,
    {
        let Self { inner, metadata } = self;
        let span = metadata.span();
        let result = executor.fetch_all(inner).instrument(span.clone()).await;
        finish_query_span(
            &span,
            None,
            result.as_ref().ok().map(|rows| rows.len() as u64),
            result.as_ref().err(),
        );
        result
    }

    pub async fn fetch_one<'e, 'c: 'e, E>(self, executor: E) -> Result<PgRow, Error>
    where
        'q: 'e,
        E: sqlx_core::executor::Executor<'c, Database = Postgres>,
    {
        let Self { inner, metadata } = self;
        let span = metadata.span();
        let result = executor.fetch_one(inner).instrument(span.clone()).await;
        finish_query_span(
            &span,
            None,
            result.as_ref().ok().map(|_| 1),
            result.as_ref().err(),
        );
        result
    }

    pub async fn fetch_optional<'e, 'c: 'e, E>(self, executor: E) -> Result<Option<PgRow>, Error>
    where
        'q: 'e,
        E: sqlx_core::executor::Executor<'c, Database = Postgres>,
    {
        let Self { inner, metadata } = self;
        let span = metadata.span();
        let result = executor
            .fetch_optional(inner)
            .instrument(span.clone())
            .await;
        finish_query_span(
            &span,
            None,
            result.as_ref().ok().map(|row| u64::from(row.is_some())),
            result.as_ref().err(),
        );
        result
    }
}

impl<'q, O> TracedQueryAs<'q, O>
where
    O: Send + Unpin + for<'row> sqlx_core::from_row::FromRow<'row, PgRow>,
{
    pub fn bind<T>(mut self, value: T) -> Self
    where
        T: 'q + sqlx_core::encode::Encode<'q, Postgres> + sqlx_core::types::Type<Postgres>,
    {
        self.inner = self.inner.bind(value);
        self
    }

    pub async fn fetch_all<'e, 'c: 'e, E>(self, executor: E) -> Result<Vec<O>, Error>
    where
        'q: 'e,
        O: 'e,
        E: 'e + sqlx_core::executor::Executor<'c, Database = Postgres>,
    {
        let Self { inner, metadata } = self;
        let span = metadata.span();
        let result = inner.fetch_all(executor).instrument(span.clone()).await;
        finish_query_span(
            &span,
            None,
            result.as_ref().ok().map(|rows| rows.len() as u64),
            result.as_ref().err(),
        );
        result
    }

    pub async fn fetch_one<'e, 'c: 'e, E>(self, executor: E) -> Result<O, Error>
    where
        'q: 'e,
        O: 'e,
        E: 'e + sqlx_core::executor::Executor<'c, Database = Postgres>,
    {
        let Self { inner, metadata } = self;
        let span = metadata.span();
        let result = inner.fetch_one(executor).instrument(span.clone()).await;
        finish_query_span(
            &span,
            None,
            result.as_ref().ok().map(|_| 1),
            result.as_ref().err(),
        );
        result
    }

    pub async fn fetch_optional<'e, 'c: 'e, E>(self, executor: E) -> Result<Option<O>, Error>
    where
        'q: 'e,
        O: 'e,
        E: 'e + sqlx_core::executor::Executor<'c, Database = Postgres>,
    {
        let Self { inner, metadata } = self;
        let span = metadata.span();
        let result = inner
            .fetch_optional(executor)
            .instrument(span.clone())
            .await;
        finish_query_span(
            &span,
            None,
            result.as_ref().ok().map(|row| u64::from(row.is_some())),
            result.as_ref().err(),
        );
        result
    }
}

impl<'q, O> TracedQueryScalar<'q, O>
where
    O: Send + Unpin,
    (O,): Send + Unpin + for<'row> sqlx_core::from_row::FromRow<'row, PgRow>,
{
    pub fn bind<T>(mut self, value: T) -> Self
    where
        T: 'q + sqlx_core::encode::Encode<'q, Postgres> + sqlx_core::types::Type<Postgres>,
    {
        self.inner = self.inner.bind(value);
        self
    }

    pub async fn fetch_all<'e, 'c: 'e, E>(self, executor: E) -> Result<Vec<O>, Error>
    where
        'q: 'e,
        O: 'e,
        E: 'e + sqlx_core::executor::Executor<'c, Database = Postgres>,
    {
        let Self { inner, metadata } = self;
        let span = metadata.span();
        let result = inner.fetch_all(executor).instrument(span.clone()).await;
        finish_query_span(
            &span,
            None,
            result.as_ref().ok().map(|rows| rows.len() as u64),
            result.as_ref().err(),
        );
        result
    }

    pub async fn fetch_one<'e, 'c: 'e, E>(self, executor: E) -> Result<O, Error>
    where
        'q: 'e,
        O: 'e,
        E: 'e + sqlx_core::executor::Executor<'c, Database = Postgres>,
    {
        let Self { inner, metadata } = self;
        let span = metadata.span();
        let result = inner.fetch_one(executor).instrument(span.clone()).await;
        finish_query_span(
            &span,
            None,
            result.as_ref().ok().map(|_| 1),
            result.as_ref().err(),
        );
        result
    }

    pub async fn fetch_optional<'e, 'c: 'e, E>(self, executor: E) -> Result<Option<O>, Error>
    where
        'q: 'e,
        O: 'e,
        E: 'e + sqlx_core::executor::Executor<'c, Database = Postgres>,
    {
        let Self { inner, metadata } = self;
        let span = metadata.span();
        let result = inner
            .fetch_optional(executor)
            .instrument(span.clone())
            .await;
        finish_query_span(
            &span,
            None,
            result.as_ref().ok().map(|row| u64::from(row.is_some())),
            result.as_ref().err(),
        );
        result
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SqlTraceMetadata {
    operation: &'static str,
    collection: String,
    fingerprint: Option<String>,
}

impl SqlTraceMetadata {
    fn from_sql(sql: &str) -> Self {
        let normalized = normalize_sql_shape(sql);
        Self {
            operation: sql_operation(&normalized),
            collection: sql_collection(&normalized),
            fingerprint: sql_fingerprint(&normalized),
        }
    }

    fn span(&self) -> Span {
        let span = tracing::info_span!(
            "db.query",
            otel.name = %format!("{} {}", self.operation, self.collection),
            otel.kind = "client",
            db.system.name = "postgresql",
            db.operation.name = self.operation,
            db.collection.name = %self.collection,
            molesignal.db.query.fingerprint = field::Empty,
            molesignal.db.pool_wait_included = true,
            db.response.affected_rows = field::Empty,
            db.response.returned_rows = field::Empty,
            error.type = field::Empty,
        );
        if let Some(fingerprint) = &self.fingerprint {
            span.record("molesignal.db.query.fingerprint", fingerprint);
        }
        span
    }
}

fn finish_query_span(
    span: &Span,
    affected_rows: Option<u64>,
    returned_rows: Option<u64>,
    error: Option<&Error>,
) {
    if let Some(rows) = affected_rows {
        span.record("db.response.affected_rows", rows);
    }
    if let Some(rows) = returned_rows {
        span.record("db.response.returned_rows", rows);
    }
    if let Some(error) = error {
        span.record("error.type", sql_error_type(error));
    }
}

fn finish_transaction_span(
    span: &Span,
    started: Instant,
    successful_outcome: &'static str,
    error: Option<&Error>,
) {
    span.record(
        "db.transaction.outcome",
        if error.is_some() {
            "error"
        } else {
            successful_outcome
        },
    );
    span.record(
        "db.transaction.duration_ms",
        started.elapsed().as_secs_f64() * 1_000.0,
    );
    if let Some(error) = error {
        span.record("error.type", sql_error_type(error));
    }
}

fn sql_error_type(error: &Error) -> &'static str {
    match error {
        Error::Configuration(_) => "configuration",
        Error::InvalidArgument(_) => "invalid_argument",
        Error::Database(_) => "database",
        Error::Io(_) => "io",
        Error::Tls(_) => "tls",
        Error::Protocol(_) => "protocol",
        Error::RowNotFound => "row_not_found",
        Error::TypeNotFound { .. } => "type_not_found",
        Error::ColumnIndexOutOfBounds { .. } => "column_index",
        Error::ColumnNotFound(_) => "column_not_found",
        Error::ColumnDecode { .. } => "column_decode",
        Error::Encode(_) => "encode",
        Error::Decode(_) => "decode",
        Error::AnyDriverError(_) => "driver",
        Error::PoolTimedOut => "pool_timeout",
        Error::PoolClosed => "pool_closed",
        Error::WorkerCrashed => "worker_crashed",
        Error::InvalidSavePointStatement => "invalid_savepoint",
        Error::BeginFailed => "begin_failed",
        _ => "database",
    }
}

/// Redact literals/placeholders, collapse whitespace, and retain only the query
/// shape long enough to derive metadata. The normalized text is never recorded.
fn normalize_sql_shape(sql: &str) -> String {
    let mut output = String::with_capacity(sql.len().min(4096));
    let mut chars = sql.chars().peekable();
    let mut in_single_quote = false;
    let mut emitted_placeholder = false;
    let mut previous_space = true;

    while let Some(character) = chars.next() {
        if output.len() >= 4096 {
            break;
        }
        if in_single_quote {
            if character == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    in_single_quote = false;
                }
            }
            if !emitted_placeholder {
                output.push('?');
                emitted_placeholder = true;
                previous_space = false;
            }
            continue;
        }
        if character == '\'' {
            in_single_quote = true;
            emitted_placeholder = false;
            continue;
        }
        if character == '$' && chars.peek().is_some_and(char::is_ascii_digit) {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
            output.push('?');
            previous_space = false;
            continue;
        }
        if character.is_ascii_digit() {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
            output.push('?');
            previous_space = false;
            continue;
        }
        if character.is_whitespace() {
            if !previous_space {
                output.push(' ');
                previous_space = true;
            }
            continue;
        }
        output.extend(character.to_lowercase());
        previous_space = false;
    }
    output.trim().to_owned()
}

fn sql_operation(normalized: &str) -> &'static str {
    normalized
        .split(|character: char| !character.is_ascii_alphabetic())
        .find_map(|token| match token {
            "select" => Some("SELECT"),
            "insert" => Some("INSERT"),
            "update" => Some("UPDATE"),
            "delete" => Some("DELETE"),
            "merge" => Some("MERGE"),
            "create" => Some("CREATE"),
            "alter" => Some("ALTER"),
            "drop" => Some("DROP"),
            "truncate" => Some("TRUNCATE"),
            "set" => Some("SET"),
            _ => None,
        })
        .unwrap_or("OTHER")
}

fn sql_collection(normalized: &str) -> String {
    let tokens = normalized
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|character: char| matches!(character, '(' | ')' | ',' | ';' | '"'))
        })
        .collect::<Vec<_>>();
    let operation = sql_operation(normalized);
    let marker = match operation {
        "INSERT" => "into",
        "SELECT" | "DELETE" => "from",
        "UPDATE" => "update",
        "CREATE" | "ALTER" | "DROP" | "TRUNCATE" => "table",
        _ => return "unknown".into(),
    };
    let candidate = tokens
        .iter()
        .position(|token| *token == marker)
        .and_then(|index| tokens.get(index + 1))
        .copied()
        .unwrap_or("unknown")
        .trim_matches('"');
    if candidate.is_empty()
        || candidate.len() > 128
        || !candidate
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.'))
    {
        "unknown".into()
    } else {
        candidate.to_owned()
    }
}

fn sql_fingerprint(normalized: &str) -> Option<String> {
    static KEY: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    let key = KEY
        .get_or_init(|| {
            std::env::var("MS_TRACE_FINGERPRINT_KEY")
                .ok()
                .map(|value| value.into_bytes())
                .filter(|value| value.len() >= 16)
        })
        .as_deref()?;
    keyed_sql_fingerprint(normalized, key)
}

fn keyed_sql_fingerprint(normalized: &str, key: &[u8]) -> Option<String> {
    if key.len() < 16 {
        return None;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(key).ok()?;
    mac.update(normalized.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in mac.finalize().into_bytes() {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    Some(output)
}

#[cfg(test)]
mod trace_tests {
    use super::*;

    #[test]
    fn sql_metadata_contains_only_bounded_shape_information() {
        let metadata = SqlTraceMetadata::from_sql(
            "SELECT email FROM users WHERE id = 42 AND email = 'secret@example.com'",
        );
        assert_eq!(metadata.operation, "SELECT");
        assert_eq!(metadata.collection, "users");
        assert!(
            !metadata
                .fingerprint
                .as_deref()
                .unwrap_or_default()
                .contains("secret")
        );
    }

    #[test]
    fn fingerprint_ignores_bound_and_literal_values() {
        let first =
            SqlTraceMetadata::from_sql("SELECT * FROM incidents WHERE id = 42 AND name = 'alice'");
        let second =
            SqlTraceMetadata::from_sql("select * from incidents where id = 7 and name = 'bob'");
        assert_eq!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn hostile_sql_literals_never_change_bounded_shape_metadata() {
        let baseline = SqlTraceMetadata::from_sql(
            "SELECT * FROM incidents WHERE id = 0 AND owner = 'fixture'",
        );
        for index in 0..512 {
            let literal = format!(
                "alice+{index}@example.com'; DROP TABLE license_versions; -- Bearer secret-{index}"
            )
            .replace('\'', "''");
            let sql = format!("SELECT * FROM incidents WHERE id = {index} AND owner = '{literal}'");
            let metadata = SqlTraceMetadata::from_sql(&sql);
            assert_eq!(metadata.operation, baseline.operation);
            assert_eq!(metadata.collection, baseline.collection);
            assert_eq!(metadata.fingerprint, baseline.fingerprint);
            assert!(metadata.collection.len() <= 128);
            assert!(!metadata.collection.contains("alice"));
            assert!(!metadata.collection.contains("license"));
        }
    }

    #[test]
    fn fingerprint_is_keyed_and_has_no_short_key_fallback() {
        assert_eq!(
            keyed_sql_fingerprint("select * from incidents", b"short"),
            None
        );
        let first = keyed_sql_fingerprint("select * from incidents", b"0123456789abcdef-first-key")
            .expect("valid fingerprint key");
        let second =
            keyed_sql_fingerprint("select * from incidents", b"0123456789abcdef-second-key")
                .expect("valid fingerprint key");
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
    }

    #[test]
    fn hostile_collection_is_replaced() {
        let metadata = SqlTraceMetadata::from_sql("SELECT * FROM \"tenant-secret@example.com\"");
        assert_eq!(metadata.collection, "unknown");
    }
}

pub mod types {
    pub use sqlx_core::types::*;
}

pub mod encode {
    pub use sqlx_core::encode::{Encode, IsNull};
}

pub use self::encode::Encode;

pub mod decode {
    pub use sqlx_core::decode::Decode;
}

pub use self::decode::Decode;

pub mod query {
    pub use sqlx_core::{
        query::{Map, Query},
        query_as::QueryAs,
        query_scalar::QueryScalar,
    };
}

pub mod prelude {
    pub use super::{
        Acquire, ConnectOptions, Connection, Decode, Encode, Executor, FromRow, IntoArguments, Row,
        Statement, Type,
    };
}

#[doc(hidden)]
pub use sqlx_core::rt as __rt;
#[doc(hidden)]
pub use sqlx_core::rt::test_block_on;
#[doc(hidden)]
#[cfg(feature = "migrate")]
pub use sqlx_core::testing;
