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
    ConnectOptions, PgPool, SqlSafeStr,
    migrate::{Migration, MigrationType, Migrator},
    postgres::{PgConnectOptions, PgPoolOptions},
};

use crate::{
    config::MetaDatabaseSettings,
    shared::{Error, Result, metrics::global_registry},
};

static META_POOL: OnceLock<RwLock<Option<PgPool>>> = OnceLock::new();
static META_POOL_COLLECTOR_REGISTERED: OnceLock<()> = OnceLock::new();

const MIGRATION_TABLE_NAME: &str = "_molesignal_schema_meta";

/// 元数据库连接池 + 启动期迁移。
#[derive(Clone)]
pub struct MetaStore {
    pub pool: PgPool,
}

impl MetaStore {
    /// 建池并跑迁移；失败 fail-fast。
    pub async fn connect(cfg: &MetaDatabaseSettings) -> Result<Self> {
        let mut opts = PgConnectOptions::from_str(&cfg.dsn)
            .map_err(|e| Error::invalid(format!("meta_database.dsn parse: {e}")))?;
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
    pub async fn connect_no_migrate(cfg: &MetaDatabaseSettings) -> Result<Self> {
        let opts = PgConnectOptions::from_str(&cfg.dsn)
            .map_err(|e| Error::invalid(format!("meta_database.dsn parse: {e}")))?;
        let opts = opts.log_statements(tracing::log::LevelFilter::Off);

        let pool = pool_options(cfg)
            .connect_with(opts)
            .await
            .map_err(|e| Error::internal(format!("meta_store connect: {e}")))?;

        observe_meta_pool(&pool);
        Ok(Self { pool })
    }
}

fn pool_options(cfg: &MetaDatabaseSettings) -> PgPoolOptions {
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
    let mut migrator = Migrator {
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
    };

    migrator.dangerous_set_table_name(MIGRATION_TABLE_NAME);
    migrator
}

fn migration(version: i64, description: &'static str, sql: &'static str) -> Migration {
    Migration::new(
        version,
        Cow::Borrowed(description),
        MigrationType::Simple,
        sql.into_sql_str(),
        sql.starts_with("-- no-transaction"),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        path::Path,
    };

    use super::*;

    const INITIAL_SQL: &str = include_str!("../migrations/20260101000001_initial.sql");
    const BUILTIN_DASHBOARDS_SQL: &str =
        include_str!("../migrations/20260101000002_builtin_dashboards.sql");
    const IAM_ROUTE_CATALOG_SQL: &str =
        include_str!("../migrations/20260101000003_iam_route_catalog.sql");

    #[test]
    fn pool_options_prewarms_the_configured_minimum() {
        let cfg = MetaDatabaseSettings {
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
        let cfg = MetaDatabaseSettings {
            dsn: "postgres://localhost/molesignal".into(),
            min_connections: 9,
            max_connections: 4,
        };

        assert_eq!(pool_options(&cfg).get_min_connections(), 4);
    }

    #[test]
    fn embedded_migrations_keep_the_three_development_baselines() {
        let migrator = embedded_migrator();
        assert_eq!(&*migrator.table_name, MIGRATION_TABLE_NAME);
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
    fn initial_schema_and_route_catalog_own_status_pages() {
        for table in [
            "status_pages",
            "status_page_components",
            "status_page_incidents",
            "status_page_incident_updates",
        ] {
            assert!(
                INITIAL_SQL.contains(&format!("CREATE TABLE {table}")),
                "missing status-page table {table}"
            );
        }
        assert!(!INITIAL_SQL.contains("CREATE TABLE status_page_incident_components"));
        assert!(INITIAL_SQL.contains("trg_status_page_incidents_components"));
        assert!(INITIAL_SQL.contains("trg_status_page_components_named"));
        assert!(!INITIAL_SQL.contains("CREATE TABLE status_page_automation_settings"));
        assert!(INITIAL_SQL.contains("ADD COLUMN automation_paused BOOLEAN"));
        assert!(INITIAL_SQL.contains("fk_status_page_incidents_source"));
        assert!(!INITIAL_SQL.contains("'alert.status.pages'"));
        assert!(INITIAL_SQL.contains("ADD COLUMN languages TEXT[]"));
        assert!(INITIAL_SQL.contains("chk_status_pages_languages"));
        assert!(INITIAL_SQL.contains("ADD COLUMN history_days INTEGER"));
        assert!(INITIAL_SQL.contains("chk_status_pages_history_days"));
        assert!(INITIAL_SQL.contains("CREATE TABLE status_page_component_status_events"));
        assert!(INITIAL_SQL.contains("uq_status_page_component_status_events_open"));
        assert!(INITIAL_SQL.contains("CREATE TABLE status_page_subscribers"));
        assert!(INITIAL_SQL.contains("target_ciphertext"));
        assert!(INITIAL_SQL.contains("target_hash"));
        assert!(INITIAL_SQL.contains("CREATE TABLE status_page_notification_deliveries"));
        assert!(INITIAL_SQL.contains("trg_status_page_incident_notification"));
        assert!(INITIAL_SQL.contains("page.visibility = 'public'"));
        assert!(INITIAL_SQL.contains("status_pages.read"));
        assert!(INITIAL_SQL.contains("status_page_domain_configs"));
        assert!(INITIAL_SQL.contains("status_page_access_sessions"));
        assert!(INITIAL_SQL.contains("delivery_retention_days"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("'status.pages', '/status-pages'"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("'reliability', 30"));
    }

    #[test]
    fn status_page_notifications_skip_hidden_components_and_carry_event_times() {
        let component = effective_function("enqueue_status_page_component_notification");
        assert!(component.contains("AND component.lifecycle = 'active'"));
        assert!(component.contains("AND component.visibility = 'enabled'"));

        let incident = effective_function("enqueue_status_page_incident_notification");
        assert!(incident.contains("'started_at', incident.started_at_micros"));
        assert!(incident.contains("'ended_at', incident.ended_at_micros"));

        // Every path that names components to subscribers goes through one function.
        let named_through = "'component_names', status_page_event_component_names(";
        assert!(incident.contains(named_through));
        assert!(!incident.contains("status_page_components"));

        let reminder = include_str!("repositories/status_pages/planned_maintenance.rs");
        assert!(reminder.contains(named_through));
        assert!(!reminder.contains("status_page_components"));

        let (_, names) = INITIAL_SQL
            .split_once("CREATE FUNCTION status_page_event_component_names(")
            .expect("component name function");

        let names = &names[..names
            .find("$$ LANGUAGE sql STABLE;")
            .expect("end of the function")];
        assert!(names.contains("AND component.visibility = 'enabled'"));
        assert!(names.contains("AND component.lifecycle = 'active'"));
    }

    #[test]
    fn planned_maintenance_options_default_to_off_in_the_schema() {
        assert!(INITIAL_SQL.contains("ADD COLUMN auto_start BOOLEAN NOT NULL DEFAULT FALSE"));
        assert!(INITIAL_SQL.contains("ADD COLUMN reminder_minutes INTEGER NOT NULL DEFAULT 0"));
        assert!(INITIAL_SQL.contains("ADD COLUMN reminded_start_micros BIGINT"));
        assert!(INITIAL_SQL.contains("chk_status_page_incident_planned_kind"));
        assert!(INITIAL_SQL.contains("idx_status_page_incidents_planned"));
    }

    #[test]
    fn backend_language_entries_are_seeded_once_in_every_language() {
        assert!(INITIAL_SQL.contains("CREATE TABLE i18n_language"));
        assert!(INITIAL_SQL.contains("PRIMARY KEY (language_key, language_code)"));

        let (_, seed) = INITIAL_SQL
            .split_once("INSERT INTO i18n_language")
            .expect("i18n_language seed");

        let seed = &seed[..seed.find(") AS seed(").expect("end of the seed")];

        let entry = regex::Regex::new(r"(?m)^\s*\('([a-z0-9_.]+)', '([a-z0-9-]+)', E?'")
            .expect("valid language entry regex");

        let mut codes_by_key: BTreeMap<&str, Vec<&str>> = BTreeMap::new();

        for seeded in entry.captures_iter(seed) {
            let key = seeded.get(1).expect("language key").as_str();
            let code = seeded.get(2).expect("language code").as_str();
            codes_by_key.entry(key).or_default().push(code);
        }

        assert!(
            !codes_by_key.is_empty(),
            "no seeded language entry was read"
        );

        for (key, codes) in codes_by_key {
            assert_eq!(
                codes,
                ["en-us", "zh-cn"],
                "{key} must be seeded once in English and once in Chinese"
            );
        }
    }

    #[test]
    fn every_path_to_subscribers_hears_only_chosen_components_and_skips_silent_changes() {
        let hears = "status_page_subscriber_hears(";

        let component = effective_function("enqueue_status_page_component_notification");
        assert!(component.contains(hears));
        assert!(component.contains("IF NOT NEW.notify_subscribers THEN"));

        let incident = effective_function("enqueue_status_page_incident_notification");
        assert!(incident.contains(hears));
        assert!(incident.contains("IF NOT NEW.notify_subscribers THEN"));
        assert!(incident.contains("'next_update_at', NEW.next_update_at_micros"));

        for source in [
            include_str!("repositories/status_pages/planned_maintenance.rs"),
            include_str!("repositories/status_pages/postmortems.rs"),
        ] {
            assert!(source.contains(hears));
            assert!(source.contains("status_page_event_component_ids("));
            assert!(!source.contains("status_page_components"));
        }

        let (_, ids) = INITIAL_SQL
            .split_once("CREATE FUNCTION status_page_event_component_ids(")
            .expect("event component function");

        let ids = &ids[..ids
            .find("$$ LANGUAGE sql STABLE;")
            .expect("end of the function")];
        assert!(ids.contains("AND component.visibility = 'enabled'"));
        assert!(ids.contains("AND component.lifecycle = 'active'"));
    }

    #[test]
    fn initial_schema_and_route_catalog_own_business_services() {
        assert!(INITIAL_SQL.contains("CREATE TABLE business_services"));
        assert!(INITIAL_SQL.contains("health_history"));
        assert!(INITIAL_SQL.contains("'business_services.read'"));
        assert!(INITIAL_SQL.contains("fk_status_page_components_business_service"));
        assert!(INITIAL_SQL.contains("'synthetic_monitor', 'business_service'"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("'business.services', '/business-services'"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("'reliability', 25"));
    }

    #[test]
    fn iam_lists_and_counters_live_with_their_owners() {
        let (_, consolidation) = INITIAL_SQL
            .rsplit_once("-- IAM catalog consolidation")
            .expect("IAM consolidation section");

        for merged in [
            "iam_role_permissions",
            "iam_builtin_role_permissions",
            "iam_builtin_role_purposes",
            "iam_permission_bundle_items",
            "iam_policy_versions",
            "iam_permission_catalog_versions",
        ] {
            assert!(
                consolidation.contains(&format!("DROP TABLE {merged};")),
                "{merged} must be folded into its owner"
            );
        }

        assert!(consolidation.contains("CREATE TABLE iam_catalog_versions"));
        assert!(consolidation.contains("ADD COLUMN iam_policy_version BIGINT"));
        assert!(consolidation.contains("FUNCTION check_iam_permission_keys()"));
        assert!(consolidation.contains("FUNCTION release_iam_permission()"));
        assert!(!IAM_ROUTE_CATALOG_SQL.contains("iam_route_catalog_versions"));
    }

    /// The last definition of a trigger function in the baseline is the one that runs.
    fn effective_function(name: &str) -> &'static str {
        let header = format!("CREATE OR REPLACE FUNCTION {name}()");

        let start = INITIAL_SQL
            .rfind(&header)
            .expect("the baseline redefines the function");

        let definition = &INITIAL_SQL[start..];

        let end = definition
            .find("$$ LANGUAGE plpgsql;")
            .expect("function terminator");

        &definition[..end]
    }

    #[test]
    fn initial_schema_and_route_catalog_own_synthetics() {
        for table in [
            "synthetic_probe_locations",
            "synthetic_probe_agents",
            "synthetic_probe_agent_tokens",
            "synthetic_secrets",
            "synthetic_monitor_revisions",
            "synthetic_probe_tasks",
            "synthetic_results",
            "synthetic_state_transitions",
        ] {
            assert!(
                INITIAL_SQL.contains(&format!("CREATE TABLE {table}")),
                "missing synthetics table {table}"
            );
        }
        assert!(INITIAL_SQL.contains("'synthetics.read'"));
        assert!(INITIAL_SQL.contains("'status_pages.publish'"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("'synthetics', '/synthetics/*'"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("'reliability', 20"));
        assert!(INITIAL_SQL.contains("fk_synthetic_results_monitor"));
        assert!(INITIAL_SQL.contains("CREATE TABLE synthetic_probe_agent_configurations"));
        assert!(INITIAL_SQL.contains("fk_synthetic_probe_agent_tokens_location"));
        assert!(INITIAL_SQL.contains("chk_synthetic_probe_agent_tokens_hash"));
        assert!(INITIAL_SQL.contains("chk_synthetic_probe_agent_tokens_prefix"));
        assert!(INITIAL_SQL.contains("PRIMARY KEY (organization_id, agent_id)"));
        assert!(INITIAL_SQL.contains("organization_id"));
        assert!(INITIAL_SQL.contains("finished_at_micros DESC, id DESC"));
        assert!(!INITIAL_SQL.contains("CREATE TABLE synthetic_monitor_revision_locations"));
        assert!(INITIAL_SQL.contains("trg_synthetic_probe_locations_listed"));
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
    fn seeded_language_keys_fit_the_key_constraint() {
        // `chk_i18n_language_key`: a migration that seeds another key fails on a fresh database.
        let row = regex::Regex::new(r"\('([^']*\.[^']*)',\s*'[a-z]{2,3}(?:-[a-z0-9]{2,8})*',")
            .expect("valid language row regex");
        let key = regex::Regex::new(r"^[a-z0-9_]+([.][a-z0-9_]+)+$").expect("valid key regex");

        assert!(INITIAL_SQL.contains("language_key ~ '^[a-z0-9_]+([.][a-z0-9_]+)+$'"));

        for sql in [INITIAL_SQL, BUILTIN_DASHBOARDS_SQL, IAM_ROUTE_CATALOG_SQL] {
            let bad: Vec<&str> = row
                .captures_iter(sql)
                .map(|capture| capture.get(1).expect("key capture").as_str())
                .filter(|seeded| !key.is_match(seeded))
                .collect();
            assert!(bad.is_empty(), "language keys outside [a-z0-9_.]: {bad:?}");
        }
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
        assert!(!IAM_ROUTE_CATALOG_SQL.contains("iam_route_permissions"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("trg_iam_routes_permissions_known"));
        assert!(IAM_ROUTE_CATALOG_SQL.contains("trg_iam_routes_catalog_version"));
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
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/migrations");
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
            vec![20260101000001, 20260101000002, 20260101000003],
            "development uses only the schema, dashboard, and route baselines"
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
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let mut rust_source = String::new();
        collect_rust_source(&workspace_root.join("bin/molesignal/src"), &mut rust_source);
        collect_rust_source(&workspace_root.join("crates"), &mut rust_source);

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
            registered.len() >= 80,
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
