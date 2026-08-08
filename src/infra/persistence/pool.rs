// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    borrow::Cow,
    str::FromStr,
    sync::{OnceLock, RwLock},
    time::Duration,
};

use prometheus::{
    IntGaugeVec, Opts,
    core::{Collector, Desc},
    proto::MetricFamily,
};
use sqlx::{
    ConnectOptions, PgPool,
    migrate::{Migration, MigrationType, Migrator},
    postgres::{PgConnectOptions, PgPoolOptions},
};

use crate::{
    config::MetaStoreSettings,
    shared::{Error, Result, metrics::global_registry},
};

static META_POOL: OnceLock<RwLock<Option<PgPool>>> = OnceLock::new();
static META_POOL_COLLECTOR_REGISTERED: OnceLock<()> = OnceLock::new();

/// 元数据库连接池 + 启动期迁移。
#[derive(Clone)]
pub struct MetaStore {
    pub pool: PgPool,
}

impl MetaStore {
    /// 建池并跑迁移；失败 fail-fast。
    pub async fn connect(cfg: &MetaStoreSettings) -> Result<Self> {
        if cfg.backend != "postgres" {
            return Err(Error::invalid(format!(
                "meta_store.backend must be 'postgres', got '{}'",
                cfg.backend
            )));
        }
        let mut opts = PgConnectOptions::from_str(&cfg.dsn)
            .map_err(|e| Error::invalid(format!("meta_store.dsn parse: {e}")))?;
        // SQLx's built-in debug event includes the full statement text. Keep it
        // disabled: the local sqlx facade emits sanitized query spans instead.
        opts = opts.log_statements(tracing::log::LevelFilter::Off);

        let pool = pool_options(cfg)
            .connect_with(opts)
            .await
            .map_err(|e| Error::internal(format!("meta_store connect: {e}")))?;

        embedded_migrator()
            .run(&pool)
            .await
            .map_err(|e| Error::internal(format!("meta_store migrate: {e}")))?;

        observe_meta_pool(&pool);
        Ok(Self { pool })
    }

    /// 已知 schema 就绪时旁路用（测试 fixture 等）。
    pub async fn connect_no_migrate(cfg: &MetaStoreSettings) -> Result<Self> {
        let opts = PgConnectOptions::from_str(&cfg.dsn)
            .map_err(|e| Error::invalid(format!("meta_store.dsn parse: {e}")))?;
        let opts = opts.log_statements(tracing::log::LevelFilter::Off);
        let pool = pool_options(cfg)
            .connect_with(opts)
            .await
            .map_err(|e| Error::internal(format!("meta_store connect: {e}")))?;
        observe_meta_pool(&pool);
        Ok(Self { pool })
    }
}

fn pool_options(cfg: &MetaStoreSettings) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(cfg.max_connections)
        .min_connections(cfg.min_connections.min(cfg.max_connections))
        .acquire_timeout(Duration::from_secs(10))
        // The dedicated tracing layer consumes these events into a histogram. Normal acquires stay
        // at TRACE, so ordinary log levels still emit only SQLx's slow-acquire warnings.
        .acquire_time_level(tracing::log::LevelFilter::Trace)
}

fn observed_meta_pool() -> &'static RwLock<Option<PgPool>> {
    META_POOL.get_or_init(|| RwLock::new(None))
}

fn observe_meta_pool(pool: &PgPool) {
    *observed_meta_pool()
        .write()
        .expect("meta pool metrics lock poisoned") = Some(pool.clone());
    META_POOL_COLLECTOR_REGISTERED.get_or_init(|| {
        let collector = MetaPoolCollector::new();
        match global_registry().register(Box::new(collector)) {
            Ok(()) | Err(prometheus::Error::AlreadyReg) => {}
            Err(error) => panic!("register meta pool metrics: {error}"),
        }
    });
}

struct MetaPoolCollector {
    connections: IntGaugeVec,
}

impl MetaPoolCollector {
    fn new() -> Self {
        Self {
            connections: IntGaugeVec::new(
                Opts::new(
                    "db_pool_connections",
                    "Current database pool connections by state and configured bound",
                ),
                &["pool", "state"],
            )
            .expect("create database pool connections gauge"),
        }
    }
}

impl Collector for MetaPoolCollector {
    fn desc(&self) -> Vec<&Desc> {
        self.connections.desc()
    }

    fn collect(&self) -> Vec<MetricFamily> {
        let pool = observed_meta_pool()
            .read()
            .expect("meta pool metrics lock poisoned")
            .clone();
        if let Some(pool) = pool {
            let total = i64::from(pool.size());
            let idle = pool.num_idle() as i64;
            let checked_out = total.saturating_sub(idle);
            let values = [
                ("total", total),
                ("idle", idle),
                ("checked_out", checked_out),
                ("min", i64::from(pool.options().get_min_connections())),
                ("max", i64::from(pool.options().get_max_connections())),
            ];
            for (state, value) in values {
                self.connections
                    .with_label_values(&["meta", state])
                    .set(value);
            }
        }
        self.connections.collect()
    }
}

/// Build the embedded migrator without `sqlx::migrate!`.
///
/// The macro pulls `sqlx-macros-core` into `Cargo.lock`, and that package lists
/// optional MySQL support which currently brings in the vulnerable `rsa` crate.
/// We still embed the SQL via `include_str!`, preserving the release image
/// behavior where migrations do not need to exist on disk at runtime.
fn embedded_migrator() -> Migrator {
    Migrator {
        migrations: Cow::Owned(vec![
            migration(
                20260101000001,
                "initial",
                include_str!("../migrations/20260101000001_initial.sql"),
            ),
            migration(
                20260101000002,
                "builtin dashboards",
                include_str!("../migrations/20260101000002_builtin_dashboards.sql"),
            ),
            migration(
                20260101000003,
                "iam route catalog",
                include_str!("../migrations/20260101000003_iam_route_catalog.sql"),
            ),
        ]),
        ..Migrator::DEFAULT
    }
}

fn migration(version: i64, description: &'static str, sql: &'static str) -> Migration {
    Migration::new(
        version,
        Cow::Borrowed(description),
        MigrationType::Simple,
        Cow::Borrowed(sql),
        sql.starts_with("-- no-transaction"),
    )
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::Path};

    use super::*;

    const INITIAL_SQL: &str = include_str!("../migrations/20260101000001_initial.sql");
    const BUILTIN_DASHBOARDS_SQL: &str =
        include_str!("../migrations/20260101000002_builtin_dashboards.sql");
    const IAM_ROUTE_CATALOG_SQL: &str =
        include_str!("../migrations/20260101000003_iam_route_catalog.sql");

    #[test]
    fn pool_options_prewarms_the_configured_minimum() {
        let cfg = MetaStoreSettings {
            backend: "postgres".into(),
            dsn: "postgres://localhost/molesignal".into(),
            min_connections: 3,
            max_connections: 8,
        };

        let options = pool_options(&cfg);
        assert_eq!(options.get_min_connections(), 3);
        assert_eq!(options.get_max_connections(), 8);
        assert_eq!(options.get_acquire_timeout(), Duration::from_secs(10));
    }

    #[test]
    fn pool_options_clamps_minimum_to_maximum() {
        let cfg = MetaStoreSettings {
            backend: "postgres".into(),
            dsn: "postgres://localhost/molesignal".into(),
            min_connections: 9,
            max_connections: 4,
        };

        assert_eq!(pool_options(&cfg).get_min_connections(), 4);
    }

    #[test]
    fn embedded_migrations_keep_large_seed_catalogs_separate() {
        let migrator = embedded_migrator();
        assert_eq!(migrator.migrations.len(), 3);
        assert_eq!(migrator.migrations[0].version, 20260101000001);
        assert_eq!(migrator.migrations[0].description, "initial");
        assert_eq!(migrator.migrations[1].version, 20260101000002);
        assert_eq!(migrator.migrations[1].description, "builtin dashboards");
        assert_eq!(migrator.migrations[2].version, 20260101000003);
        assert_eq!(migrator.migrations[2].description, "iam route catalog");
    }

    #[test]
    fn initial_schema_owns_field_masking_and_dashboard_seed_omits_self_logs() {
        assert!(INITIAL_SQL.contains("CREATE TABLE IF NOT EXISTS field_masking_rules"));
        assert!(INITIAL_SQL.contains("chk_field_masking_stream_type"));
        assert!(INITIAL_SQL.contains("uq_field_masking_rule_org_priority"));
        assert!(
            !BUILTIN_DASHBOARDS_SQL.contains("overview-warning-error-logs"),
            "self logs are no longer part of the built-in system dashboard"
        );
    }

    #[test]
    fn seeded_resource_ids_do_not_encode_builtin_semantics() {
        let semantic_id =
            regex::Regex::new(r"(?m)^\s*\('([^']*builtin[^']*)'\s*,").expect("valid seed id regex");

        for sql in [INITIAL_SQL, BUILTIN_DASHBOARDS_SQL, IAM_ROUTE_CATALOG_SQL] {
            let ids: Vec<&str> = semantic_id
                .captures_iter(sql)
                .map(|capture| capture.get(1).expect("id capture").as_str())
                .collect();
            assert!(ids.is_empty(), "seed resource IDs must be opaque: {ids:?}");
        }

        assert!(
            !INITIAL_SQL.contains("'builtin_' ||"),
            "generated IAM role IDs must be opaque"
        );
    }

    #[test]
    fn iam_route_catalog_owns_root_and_navigation_invariants() {
        assert!(IAM_ROUTE_CATALOG_SQL.contains("uq_iam_platform_administrators_single_active"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("trg_protect_configured_root_assignment"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("trg_protect_configured_root_user"));
        assert!(
            IAM_ROUTE_CATALOG_SQL
                .contains("DROP TRIGGER IF EXISTS trg_protect_last_platform_administrator")
        );
        assert!(
            IAM_ROUTE_CATALOG_SQL
                .contains("DROP TRIGGER IF EXISTS trg_protect_platform_administrator_user")
        );
        assert!(
            IAM_ROUTE_CATALOG_SQL
                .contains("DROP FUNCTION IF EXISTS protect_last_platform_administrator()")
        );
        assert!(
            IAM_ROUTE_CATALOG_SQL
                .contains("DROP FUNCTION IF EXISTS protect_platform_administrator_user()")
        );
        assert!(
            !IAM_ROUTE_CATALOG_SQL
                .contains("CREATE OR REPLACE FUNCTION protect_last_platform_administrator()")
        );
        assert!(
            !IAM_ROUTE_CATALOG_SQL
                .contains("CREATE OR REPLACE FUNCTION protect_platform_administrator_user()")
        );
        assert!(IAM_ROUTE_CATALOG_SQL.contains("iam_route_permissions"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("trg_iam_route_permissions_catalog_version"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("navigation_group"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("navigation_position"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("chk_iam_routes_key_format"));

        let route_seed =
            regex::Regex::new(r"(?m)^\s*\('([^']+)'\s*,\s*'/").expect("valid route seed regex");
        let invalid_route_keys: Vec<&str> = route_seed
            .captures_iter(IAM_ROUTE_CATALOG_SQL)
            .map(|capture| capture.get(1).expect("route key capture").as_str())
            .filter(|route_key| route_key.contains('-'))
            .collect();
        assert!(
            invalid_route_keys.is_empty(),
            "route keys must use dot-separated segments: {invalid_route_keys:?}"
        );
    }

    /// 守护：磁盘 migration 与 embedded migrator 必须严格一一对应。
    #[test]
    fn embedded_migrations_match_files_on_disk() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/infra/migrations");
        let mut on_disk: Vec<i64> = std::fs::read_dir(&dir)
            .expect("read migrations dir")
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let version = name.strip_suffix(".sql")?.split('_').next()?.to_owned();
                version.parse::<i64>().ok()
            })
            .collect();
        on_disk.sort_unstable();
        assert_eq!(
            on_disk,
            vec![20260101000001, 20260101000002, 20260101000003,],
            "schema and seed catalogs must use the registered embedded migrations"
        );

        let mut registered: Vec<i64> = embedded_migrator()
            .migrations
            .iter()
            .map(|m| m.version)
            .collect();
        registered.sort_unstable();

        assert_eq!(
            on_disk, registered,
            "migrations/*.sql on disk must exactly match embedded_migrator()"
        );
    }

    #[test]
    fn builtin_dashboards_cover_every_registered_production_metric() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut rust_source = String::new();
        collect_rust_source(&manifest_dir.join("src"), &mut rust_source);

        let registration = regex::Regex::new(
            r#"(?:register_(?:int_)?(?:counter|gauge|histogram)(?:_vec)?|(?:Opts|HistogramOpts)::new|(?:IntGaugeVec|GaugeVec|HistogramVec|IntCounterVec)::new)\s*!?\s*\(\s*"([a-zA-Z_:][a-zA-Z0-9_:.-]*)""#,
        )
        .expect("valid metric registration regex");
        let registered: BTreeSet<&str> = registration
            .captures_iter(&rust_source)
            .map(|capture| capture.get(1).expect("metric capture").as_str())
            .filter(|name| !name.starts_with("test_"))
            .collect();

        let dashboard_metric = regex::Regex::new(
            r#"\('molesignal-[^']+',\s*\d+,\s*'([a-zA-Z_:][a-zA-Z0-9_:.-]*)',\s*'(?:counter|gauge|histogram|timestamp|ratio|status)'"#,
        )
        .expect("valid dashboard metric regex");
        let dashboard: BTreeSet<&str> = dashboard_metric
            .captures_iter(BUILTIN_DASHBOARDS_SQL)
            .map(|capture| capture.get(1).expect("metric capture").as_str())
            .collect();

        assert!(
            registered.len() >= 87,
            "metric scanner unexpectedly found only {} production metrics",
            registered.len()
        );
        assert_eq!(
            registered, dashboard,
            "the built-in metric dashboards must cover the production metric catalog exactly"
        );
    }

    fn collect_rust_source(directory: &Path, output: &mut String) {
        let entries = std::fs::read_dir(directory).expect("read Rust source directory");
        for entry in entries {
            let entry = entry.expect("read Rust source entry");
            let path = entry.path();
            if path.is_dir() {
                collect_rust_source(&path, output);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                output.push_str(&std::fs::read_to_string(path).expect("read Rust source file"));
                output.push('\n');
            }
        }
    }
}
