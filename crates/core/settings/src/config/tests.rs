// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::path::PathBuf;

use super::*;

#[test]
fn example_config_file_parses_all_sections() {
    // 把仓库里维护的 conf/config.toml 当作 source-of-truth：
    // 它必须能整段 parse 而不漏掉任何已声明的配置字段。
    let text = include_str!("../../../../../conf/config.toml");
    let s: Settings = toml::from_str(text).expect("example config must parse cleanly");
    // 抽样核对一些"后期补进来" section 是否真被读出来
    assert!(!s.http.tls.enabled);
    assert_eq!(s.http.tls.plain_port, 80);
    assert_eq!(s.http.tls.port, 443);
    assert_eq!(s.http.tls.key_storage_dir, "/var/lib/molesignal/acme");
    assert_eq!(s.object_store.retry.max_attempts, 4);
    assert_eq!(s.search_jobs.workers, 2);
    assert_eq!(s.querier.auto_async_threshold_rows, 50_000_000);
    assert_eq!(s.querier.estimate_throughput_per_sec, 1_000);
    assert_eq!(
        s.scheduled_reports.renderer.base_url,
        "http://127.0.0.1:5173"
    );
    assert_eq!(s.log_inspection.min_interval_secs, 3600);
    assert_eq!(s.log_inspection.max_per_org, 20);
    assert_eq!(s.log_inspection.run_timeout_secs, 300);
    assert_eq!(s.agent.daily_token_budget, 0);
    assert_eq!(s.agent.budget_warning_percent, 80);
    assert!(s.agent.skill_paths.is_empty());
    assert!(s.agent.mcp_stdio_commands.is_empty());
    assert_eq!(s.agent.max_concurrent_runs, 8);
    assert_eq!(s.agent.max_context_tokens, 128_000);
    assert_eq!(
        s.mmdb.db_path,
        "/usr/share/molesignal/mmdb/dbip-city-lite.mmdb"
    );
    assert!(!s.flight_sql.enabled);
    assert_eq!(s.flight_sql.port, 5083);
    assert!(s.telemetry.self_collect.enabled);
    assert!(!s.telemetry.self_collect.profiles_enabled);
    assert!(!s.profiling.enabled);
    assert_eq!(s.profiling.bind, "127.0.0.1");
    assert_eq!(s.profiling.port, 5086);
    assert_eq!(s.intake.prometheus.max_labels_per_series, 64);
    assert_eq!(s.intake.prometheus.max_samples_per_batch, 16_384);
    assert_eq!(s.intake.max_buffer_memory_size, 1024);
    assert!(s.intake.rotation.adaptive_enabled);
    assert_eq!(s.intake.rotation.target_file_size, 128);
    assert_eq!(
        s.intake
            .prometheus
            .cardinality
            .max_active_series_per_process,
        1_000_000
    );
    assert_eq!(s.apm.queue_capacity, 65_536);
    assert_eq!(s.apm.version_comparison.min_requests_per_version, 1_000);
    s.validate().expect("example config validation");
}

#[test]
fn probe_control_plane_has_no_runtime_switch() {
    let settings: Settings = toml::from_str("").expect("defaults parse");
    assert_eq!(settings.probe.register_port, 5084);
    assert_eq!(settings.probe.control_port, 5085);
    settings.probe.validate().expect("Probe defaults validate");

    let error = toml::from_str::<Settings>(
        r#"
[probe]
enabled = false
"#,
    )
    .expect_err("removed Probe switch must not be accepted");
    assert!(error.to_string().contains("enabled"));
}

#[test]
fn apm_defaults_are_bounded_and_validated() {
    let defaults: Settings = toml::from_str("").expect("defaults parse");
    assert_eq!(defaults.apm.flush_interval_ms, 5_000);
    assert_eq!(defaults.apm.hot_retention_hours, 24);
    assert_eq!(defaults.apm.rollup_retention_days, 30);
    assert_eq!(defaults.apm.histogram.schema_version, 2);
    assert_eq!(defaults.apm.histogram.buckets_per_octave, 8);
    assert_eq!(
        defaults.apm.version_comparison.min_requests_per_version,
        1_000
    );
    defaults.validate().expect("APM defaults validate");

    let invalid: Settings = toml::from_str(
        r#"
[apm]
queue_capacity = 1
"#,
    )
    .expect("invalid bounds still deserialize");
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("queue_capacity")
    );

    let invalid_histogram: Settings = toml::from_str(
        r#"
[apm.histogram]
buckets_per_octave = 32
"#,
    )
    .expect("invalid histogram still deserializes");
    assert!(
        invalid_histogram
            .validate()
            .unwrap_err()
            .to_string()
            .contains("buckets_per_octave")
    );

    let removed_bounds = toml::from_str::<Settings>(
        r#"
[apm.histogram]
boundaries_ms = [1, 2, 4]
"#,
    )
    .expect_err("the old fixed bounds must not be accepted");
    assert!(removed_bounds.to_string().contains("boundaries_ms"));

    for removed_switch in ["enabled", "force_disabled"] {
        let error = toml::from_str::<Settings>(&format!(
            r#"
[apm]
{removed_switch} = false
"#
        ))
        .expect_err("removed APM switches must not be accepted");
        assert!(
            error.to_string().contains(removed_switch),
            "unexpected error for removed APM switch {removed_switch}: {error}"
        );
    }

    // Cardinality limits are set on the platform settings page.
    let removed_limits = toml::from_str::<Settings>(
        r#"
[apm.cardinality]
services_per_org_hour = 100
"#,
    )
    .expect_err("cardinality limits are no longer read from the file");
    assert!(removed_limits.to_string().contains("cardinality"));
}

#[test]
fn prometheus_intake_limits_are_bounded_and_validated() {
    let defaults: Settings = toml::from_str("").expect("defaults parse");
    assert_eq!(defaults.intake.prometheus.max_labels_per_series, 64);
    assert_eq!(defaults.intake.prometheus.max_label_name_length, 128);
    assert_eq!(defaults.intake.prometheus.max_label_value_length, 2048);
    assert_eq!(defaults.intake.prometheus.max_samples_per_batch, 16_384);
    defaults.validate().expect("defaults validate");

    let mut invalid = defaults;
    invalid.intake.prometheus.max_samples_per_batch = 0;
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("max_samples_per_batch")
    );

    let mut invalid = Settings::default();
    invalid.intake.flush_parallelism = 0;
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("flush_parallelism")
    );
}

#[test]
fn intake_resource_control_defaults_are_bounded_and_validated() {
    let defaults: Settings = toml::from_str("").expect("defaults parse");
    assert_eq!(defaults.intake.max_buffer_memory_size, 1024);
    assert_eq!(defaults.intake.rotation.min_buffer_size, 16);
    assert_eq!(defaults.intake.rotation.ewma_alpha, 0.2);
    assert!(defaults.intake.prometheus.cardinality.enabled);
    defaults.validate().expect("defaults validate");

    let invalid_rotation: Settings = toml::from_str(
        r#"
[intake.rotation]
ewma_alpha = 0.0
"#,
    )
    .unwrap();
    assert!(
        invalid_rotation
            .validate()
            .unwrap_err()
            .to_string()
            .contains("ewma_alpha")
    );

    let invalid_cardinality: Settings = toml::from_str(
        r#"
[intake.prometheus.cardinality]
max_active_series_per_process = 0
"#,
    )
    .unwrap();
    assert!(
        invalid_cardinality
            .validate()
            .unwrap_err()
            .to_string()
            .contains("max_active_series_per_process")
    );

    let mut invalid_ttl = Settings::default();
    invalid_ttl.intake.prometheus.cardinality.idle_ttl_secs = 366 * 24 * 60 * 60;
    assert!(
        invalid_ttl
            .validate()
            .unwrap_err()
            .to_string()
            .contains("idle_ttl_secs")
    );
}

#[test]
fn self_telemetry_defaults_are_opt_in_and_bounded() {
    let settings: Settings = toml::from_str("").expect("defaults parse");
    let self_collect = &settings.telemetry.self_collect;
    assert!(!self_collect.enabled);
    assert!(self_collect.metrics_enabled);
    assert!(!self_collect.profiles_enabled);
    assert_eq!(self_collect.queue_capacity, 8192);
    assert_eq!(settings.profiling.bind, "127.0.0.1");
    settings.validate().unwrap();

    let enabled: Settings =
        toml::from_str("[telemetry.self_collect]\nenabled = true\n").expect("self config parses");
    assert!(enabled.telemetry.self_collect.enabled);
}

#[test]
fn self_telemetry_rejects_removed_org_slug_and_unsafe_values() {
    let removed_field = toml::from_str::<Settings>(
        r#"
[telemetry.self_collect]
enabled = true
org_slug = "_sys"
"#,
    );
    assert!(removed_field.unwrap_err().to_string().contains("org_slug"));

    let mut settings: Settings = toml::from_str("").unwrap();
    settings.telemetry.self_collect.queue_capacity = 0;
    assert!(
        settings
            .validate()
            .unwrap_err()
            .to_string()
            .contains("queue_capacity")
    );

    let mut profile_settings: Settings = toml::from_str("").unwrap();
    profile_settings.telemetry.self_collect.enabled = true;
    profile_settings
        .telemetry
        .self_collect
        .profile_interval_secs = 0;
    profile_settings
        .validate()
        .expect("disabled self profiles ignore profile-only tuning");
    profile_settings.telemetry.self_collect.profiles_enabled = true;
    assert!(
        profile_settings
            .validate()
            .unwrap_err()
            .to_string()
            .contains("profile_interval_secs")
    );
}

#[test]
fn profiling_listener_is_loopback_by_default() {
    let settings = ProfilingSettings {
        enabled: true,
        ..ProfilingSettings::default()
    };
    assert!(settings.validate().is_ok());

    let exposed = ProfilingSettings {
        enabled: true,
        bind: "0.0.0.0".into(),
        allow_remote: false,
        ..ProfilingSettings::default()
    };
    assert!(exposed.validate().is_err());

    let authorized_remote = ProfilingSettings {
        allow_remote: true,
        ..exposed
    };
    assert!(authorized_remote.validate().is_ok());
}

#[test]
fn enabled_profiling_listener_has_a_default_port_of_its_own() {
    let mut settings: Settings = toml::from_str("").expect("defaults parse");
    settings.profiling.enabled = true;
    settings.flight_sql.enabled = true;
    settings.http.tls.enabled = true;
    assert_eq!(settings.profiling.port, 5086);

    settings
        .validate()
        .expect("no other listener defaults to the pprof port");
}

#[test]
fn profiling_port_shared_with_another_listener_is_rejected() {
    for (port, extra, other) in [
        (5080, "", "http.port"),
        (6000, "[http]\nport = 6000\n", "http.port"),
        (5082, "", "grpc.port"),
        (4317, "", "otlp_grpc.port"),
        (5084, "", "probe.register_port"),
        (5085, "", "probe.control_port"),
        (5083, "[flight_sql]\nenabled = true\n", "flight_sql.port"),
        (80, "[http.tls]\nenabled = true\n", "http.tls.plain_port"),
        (443, "[http.tls]\nenabled = true\n", "http.tls.port"),
    ] {
        let text = format!("[profiling]\nenabled = true\nport = {port}\n{extra}");
        let settings: Settings = toml::from_str(&text).expect("config parses");

        let error = settings
            .validate()
            .expect_err("a shared port must be rejected");

        assert_eq!(
            error.to_string(),
            format!(
                "profiling.port and {other} are both {port}; choose a different profiling.port"
            )
        );
    }
}

#[test]
fn profiling_port_ignores_listeners_that_do_not_start() {
    // 关闭的 pprof listener 不占端口，与 Probe 注册端口相同也能加载。
    let disabled: Settings = toml::from_str("[profiling]\nport = 5084\n").expect("config parses");

    disabled
        .validate()
        .expect("a disabled pprof listener binds nothing");

    // Flight SQL 与 TLS 默认关闭，它们的端口不算。
    for port in [5083, 80, 443] {
        let text = format!("[profiling]\nenabled = true\nport = {port}\n");
        let settings: Settings = toml::from_str(&text).expect("config parses");

        settings
            .validate()
            .expect("a listener that is switched off binds nothing");
    }
}

#[test]
fn flight_sql_defaults_when_missing_from_toml() {
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert!(!s.flight_sql.enabled);
    assert_eq!(s.flight_sql.bind, "0.0.0.0");
    assert_eq!(s.flight_sql.port, 5083);
    assert_eq!(s.flight_sql.default_lookback_hours, 24);
    assert_eq!(s.flight_sql.max_message_size, 32);
    // 显式开启 round-trip
    let s2: Settings = toml::from_str("[flight_sql]\nenabled = true\nport = 6000\n").unwrap();
    assert!(s2.flight_sql.enabled);
    assert_eq!(s2.flight_sql.port, 6000);
    assert_eq!(s2.flight_sql.default_lookback_hours, 24);
}

#[test]
fn otlp_grpc_defaults_when_missing_from_toml() {
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert_eq!(s.otlp_grpc.bind, "0.0.0.0");
    assert_eq!(s.otlp_grpc.port, 4317);
    assert_eq!(s.otlp_grpc.max_message_size, 32);
    let s2: Settings = toml::from_str("[otlp_grpc]\nport = 14317\n").unwrap();
    assert_eq!(s2.otlp_grpc.port, 14317);
}

#[test]
fn removed_service_switches_are_rejected() {
    for removed in [
        "[notify]\nenabled = false\n",
        "[scheduled_reports.renderer]\nenabled = false\n",
        "[otlp_grpc]\nenabled = false\n",
        "[telemetry.self_intake]\nenabled = true\n",
        "[telemetry.self]\nenabled = true\n",
        "[telemetry.trace]\nself_intake_enabled = true\n",
        "[telemetry.self_collect]\nlogs_enabled = false\n",
        "[telemetry.self_collect]\nlogs_retention_days = 7\n",
        "[telemetry.self_collect]\nretention_days = 7\n",
        "[telemetry.self_collect]\nmetrics_retention_days = 7\n",
        "[telemetry.self_collect]\ntraces_enabled = false\n",
    ] {
        assert!(
            toml::from_str::<Settings>(removed).is_err(),
            "removed switch must be rejected: {removed}"
        );
    }
}

#[test]
fn meta_database_is_a_top_level_section() {
    let s: Settings = toml::from_str(
        "[meta_database]\ndsn = \"postgres://u:p@h:5432/db\"\nmax_connections = 8\n",
    )
    .expect("[meta_database] parses");
    assert_eq!(s.meta_database.dsn, "postgres://u:p@h:5432/db");
    assert_eq!(s.meta_database.min_connections, 2);
    assert_eq!(s.meta_database.max_connections, 8);
}

#[test]
fn unknown_keys_in_database_and_object_store_sections_are_rejected() {
    // 写错的键必须报错。
    for invalid in [
        "[meta_database]\nunknown_key = 1\n",
        "[object_store]\nunknown_key = 1\n",
        "[object_store.retry]\nunknown_key = 1\n",
    ] {
        assert!(
            toml::from_str::<Settings>(invalid).is_err(),
            "unknown key must be rejected: {invalid}"
        );
    }
}

#[test]
fn object_store_transfer_settings_are_bounded_and_validated() {
    let defaults = Settings::default();
    defaults
        .object_store
        .validate()
        .expect("defaults are valid");

    for invalid in [
        "[object_store]\nmultipart_threshold_size = 0\n",
        "[object_store]\nmultipart_part_size = 4\n",
        "[object_store]\nmultipart_part_size = 1025\n",
        "[object_store]\nrange_threshold_size = 0\n",
        "[object_store]\nrange_chunk_size = 0\n",
        "[object_store]\nrange_chunk_size = 1025\n",
    ] {
        let settings: Settings = toml::from_str(invalid).expect("valid TOML shape");
        assert!(
            settings.object_store.validate().is_err(),
            "must be rejected: {invalid}"
        );
    }
}

#[test]
fn defaults_roundtrip_through_toml() {
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert_eq!(s.http.port, 5080);
    assert_eq!(
        s.meta_database.dsn,
        "postgres://molesignal:molesignal@localhost:5432/molesignal"
    );
    assert_eq!(s.meta_database.min_connections, 2);
    assert_eq!(s.meta_database.max_connections, 16);
    assert_eq!(s.object_store.backend, "local");
    assert_eq!(s.alert.dispatch_interval_secs, 10);
    assert_eq!(s.alert.eval_timeout_secs, 10);
    assert_eq!(s.cluster.heartbeat_interval_secs, 5);
    assert_eq!(s.cluster.advertise_addr, "127.0.0.1:5082");
    assert_eq!(s.compactor.target_size, 512);
    assert_eq!(s.compactor.max_concurrent_groups, 4);
    assert_eq!(s.router.rate_limit.intake_qps, 1000);
    assert_eq!(s.cache.index_handle.capacity, 10_000);
    assert_eq!(s.cache.index_handle.ttl_secs, 600);
    assert_eq!(s.cache.query_result.capacity, 1_000);
    assert_eq!(s.cache.query_result.ttl_secs, 60);
    assert!(matches!(s.notify.smtp.tls, SmtpTls::Starttls));
    // re-emit + re-parse must be lossless on defaults
    let text = toml::to_string(&s).unwrap();
    let s2: Settings = toml::from_str(&text).unwrap();
    assert_eq!(s2.alert.dispatch_interval_secs, 10);
    assert_eq!(s2.compactor.target_size, 512);
    assert_eq!(s2.cluster.advertise_addr, "127.0.0.1:5082");
    assert_eq!(s2.cache.query_result.capacity, 1_000);
}

#[test]
fn compactor_target_size_must_be_positive_and_bounded() {
    for (size, valid) in [
        (0, false),
        (1, true),
        (512, true),
        (64 * 1024, true),
        (64 * 1024 + 1, false),
    ] {
        let settings = CompactorSettings {
            target_size: size,
            ..CompactorSettings::default()
        };

        assert_eq!(settings.validate().is_ok(), valid, "target_size {size}");
    }
}

#[test]
fn compactor_target_size_is_in_mib() {
    let toml_text = r#"
[compactor]
target_size = 256
"#;
    let s: Settings = toml::from_str(toml_text).expect("compactor target size must parse");
    assert_eq!(s.compactor.target_size, 256);
    assert_eq!(s.compactor.target_bytes(), 256 * 1024 * 1024);
}

#[test]
fn auth_settings_do_not_serialize_jwt_issuer() {
    let value = toml::Value::try_from(AuthSettings::default()).unwrap();
    assert!(!value.as_table().unwrap().contains_key("issuer"));
}

#[test]
fn object_cache_defaults_when_missing_from_toml() {
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert_eq!(s.cache.object.root, PathBuf::from("./data/cache/objects"));
    assert_eq!(s.cache.object.max_size, 10 * 1024);
    assert_eq!(s.cache.object.block_size, 4);
    assert_eq!(s.cache.object.block_bytes(), 4 * 1024 * 1024);
    assert!(s.cache.object.is_effectively_enabled());

    let only_cache: Settings = toml::from_str("[cache]\n").expect("empty [cache] must parse");
    assert_eq!(only_cache.cache.object.block_size, 4);
}

#[test]
fn object_cache_explicit_disable_round_trips() {
    let toml_text = r#"
[cache.object]
enabled = false
root = "/var/cache/molesignal/objects"
max_size = 1234
"#;
    let s: Settings = toml::from_str(toml_text).expect("object cache settings must parse");
    assert_eq!(
        s.cache.object.root,
        PathBuf::from("/var/cache/molesignal/objects")
    );
    assert_eq!(s.cache.object.max_size, 1234);
    assert!(!s.cache.object.is_effectively_enabled());
}

#[test]
fn tantivy_caches_default_when_missing_from_toml() {
    // 缺省 TOML 不写 [cache.tantivy_result] / [cache.tantivy_footer] 时全部走默认。
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert_eq!(s.cache.tantivy_result.capacity, 1_000_000);
    assert_eq!(s.cache.tantivy_result.ttl_secs, 600);
    // change `tantivy-puffin-migration`：footer 缩到 ~几 KB 后默认 capacity 上调。
    assert_eq!(s.cache.tantivy_footer.capacity, 100_000);
    assert_eq!(s.cache.tantivy_footer.ttl_secs, 3600);
}

#[test]
fn tantivy_caches_capacity_zero_round_trips() {
    let toml_text = r#"
[cache.tantivy_result]
capacity = 0
ttl_secs = 1

[cache.tantivy_footer]
capacity = 0
ttl_secs = 1
"#;
    let s: Settings = toml::from_str(toml_text).expect("zero capacity must parse");
    assert_eq!(s.cache.tantivy_result.capacity, 0);
    assert_eq!(s.cache.tantivy_footer.capacity, 0);
}

#[test]
fn streaming_agg_cache_defaults_when_missing_from_toml() {
    // 缺省 TOML（[search.stream_agg_cache] 整段都没写）必须落到默认值：默认开启，256 MiB。
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert_eq!(s.search.stream_agg_cache.max_size, 256);
    assert_eq!(s.search.stream_agg_cache.ttl_secs, 300);
    assert_eq!(s.search.stream_agg_cache.safe_lookback_secs, 300);
    assert_eq!(s.search.stream_agg_cache.max_series_per_query, 10_000);
}

#[test]
fn streaming_agg_cache_explicit_enable_round_trips() {
    let toml_text = r#"
[search.stream_agg_cache]
max_size = 64
ttl_secs = 120
safe_lookback_secs = 600
max_series_per_query = 0
"#;
    let s: Settings = toml::from_str(toml_text).expect("streaming_agg_cache must parse");
    // max_size > 0 即启用（无独立 enabled）。
    assert_eq!(s.search.stream_agg_cache.max_size, 64);
    assert_eq!(s.search.stream_agg_cache.ttl_secs, 120);
    assert_eq!(s.search.stream_agg_cache.safe_lookback_secs, 600);
    assert_eq!(s.search.stream_agg_cache.max_series_per_query, 0);
    // 其余 search 子段仍是默认。
    assert_eq!(s.search.max_result_rows, 0);
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    assert_eq!(s.search.admission.default_max_concurrent, 2 * cpus);
    assert_eq!(s.search.admission.max_wait_secs, 60);
}

#[test]
fn wal_defaults_hold_a_fifteen_second_lease_and_eight_mib_groups() {
    let s: Settings = toml::from_str("[wal]\n").expect("empty [wal] section must parse");
    assert_eq!(s.wal.lease(), std::time::Duration::from_secs(15));
    assert_eq!(s.wal.max_group_bytes(), 8 * 1024 * 1024);
    assert_eq!(s.wal.put_attempts, 3);
    assert_eq!(s.wal.takeover_interval(), std::time::Duration::from_secs(5));
    assert!(!s.wal.encrypt);

    let s: Settings = toml::from_str("[wal]\nlease_secs = 1\nmax_takeover_size = 0\n").unwrap();
    assert_eq!(s.wal.lease(), std::time::Duration::from_secs(3));
    assert_eq!(s.wal.max_takeover_bytes(), 1024 * 1024);
}

#[test]
fn parquet_metadata_and_block_memory_caches_default_on_and_turn_off_at_zero() {
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert_eq!(s.cache.parquet_metadata.max_size, 256);
    assert_eq!(s.cache.parquet_metadata.max_bytes(), 256 * 1024 * 1024);
    assert_eq!(s.cache.object.memory_size, 256);

    let toml_text = r#"
[cache.parquet_metadata]
max_size = 0

[cache.object]
memory_size = 0
"#;
    let s: Settings = toml::from_str(toml_text).expect("zero sizes must parse");
    assert_eq!(s.cache.parquet_metadata.max_size, 0);
    assert_eq!(s.cache.object.memory_size, 0);
}
