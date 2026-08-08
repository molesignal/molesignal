// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::path::PathBuf;

use super::*;

#[test]
fn example_config_file_parses_all_sections() {
    // 把仓库里维护的 conf/config.toml 当作 source-of-truth：
    // 它必须能整段 parse 而不漏掉任何已声明的配置字段。
    let text = include_str!("../../conf/config.toml");
    let s: Settings = toml::from_str(text).expect("example config must parse cleanly");
    // 抽样核对一些"后期补进来" section 是否真被读出来
    assert!(!s.http.tls.enabled);
    assert_eq!(s.http.tls.plain_port, 80);
    assert_eq!(s.http.tls.port, 443);
    assert_eq!(s.http.tls.key_storage_dir, "/var/lib/molesignal/acme");
    assert_eq!(s.store.object.retry.max_attempts, 4);
    assert_eq!(s.intelligence.default_provider, "openai");
    assert_eq!(s.search_jobs.workers, 2);
    assert_eq!(s.querier.auto_async_threshold_rows, 50_000_000);
    assert_eq!(s.querier.estimate_throughput_per_sec, 1_000);
    assert_eq!(s.compactor.retention_days, 30);
    assert_eq!(
        s.scheduled_reports.renderer.base_url,
        "http://127.0.0.1:5173"
    );
    assert_eq!(
        s.mmdb.db_path,
        "/usr/share/molesignal/mmdb/GeoLite2-City.mmdb"
    );
    assert!(!s.flight_sql.enabled);
    assert_eq!(s.flight_sql.port, 5083);
    assert!(s.telemetry.self_collect.enabled);
    assert_eq!(s.telemetry.self_collect.retention_days, 7);
    assert!(!s.profiling.enabled);
    assert_eq!(s.profiling.bind, "127.0.0.1");
    assert_eq!(s.profiling.port, 5084);
    assert_eq!(s.ingester.prometheus.max_labels_per_series, 64);
    assert_eq!(s.ingester.prometheus.max_samples_per_batch, 16_384);
    assert_eq!(s.ingester.max_buffer_memory_mb, 1024);
    assert!(s.ingester.rotation.adaptive_enabled);
    assert_eq!(s.ingester.rotation.target_file_size_mb, 128);
    assert_eq!(
        s.ingester.prometheus.cardinality.max_active_series_per_org,
        200_000
    );
    assert_eq!(s.apm.queue_capacity, 65_536);
    assert_eq!(s.apm.cardinality.services_per_org_hour, 200);
    s.validate().expect("example config validation");
}

#[test]
fn apm_defaults_are_bounded_and_validated() {
    let defaults: Settings = toml::from_str("").expect("defaults parse");
    assert_eq!(defaults.apm.flush_interval_ms, 5_000);
    assert_eq!(defaults.apm.hot_retention_hours, 24);
    assert_eq!(defaults.apm.rollup_retention_days, 30);
    assert_eq!(defaults.apm.histogram.boundaries_ms.last(), Some(&60_000));
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
boundaries_ms = [1, 4, 4]
"#,
    )
    .expect("invalid histogram still deserializes");
    assert!(
        invalid_histogram
            .validate()
            .unwrap_err()
            .to_string()
            .contains("boundaries_ms")
    );

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
}

#[test]
fn prometheus_ingest_limits_are_bounded_and_validated() {
    let defaults: Settings = toml::from_str("").expect("defaults parse");
    assert_eq!(defaults.ingester.prometheus.max_labels_per_series, 64);
    assert_eq!(defaults.ingester.prometheus.max_label_name_bytes, 128);
    assert_eq!(defaults.ingester.prometheus.max_label_value_bytes, 2048);
    assert_eq!(defaults.ingester.prometheus.max_samples_per_batch, 16_384);
    defaults.validate().expect("defaults validate");

    let mut invalid = defaults;
    invalid.ingester.prometheus.max_samples_per_batch = 0;
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("max_samples_per_batch")
    );

    let mut invalid = Settings::default();
    invalid.ingester.flush_parallelism = 0;
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("flush_parallelism")
    );
}

#[test]
fn ingest_resource_control_defaults_are_bounded_and_validated() {
    let defaults: Settings = toml::from_str("").expect("defaults parse");
    assert_eq!(defaults.ingester.max_buffer_memory_mb, 1024);
    assert_eq!(defaults.ingester.rotation.min_buffer_mb, 16);
    assert_eq!(defaults.ingester.rotation.ewma_alpha, 0.2);
    assert!(defaults.ingester.prometheus.cardinality.enabled);
    defaults.validate().expect("defaults validate");

    let invalid_rotation: Settings = toml::from_str(
        r#"
[ingester.rotation]
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
[ingester.prometheus.cardinality]
max_active_series_per_process = 10
max_active_series_per_org = 20
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
    invalid_ttl.ingester.prometheus.cardinality.idle_ttl_secs = 366 * 24 * 60 * 60;
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
fn flight_sql_defaults_when_missing_from_toml() {
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert!(!s.flight_sql.enabled);
    assert_eq!(s.flight_sql.bind, "0.0.0.0");
    assert_eq!(s.flight_sql.port, 5083);
    assert_eq!(s.flight_sql.default_lookback_hours, 24);
    assert_eq!(s.flight_sql.max_message_size_mb, 32);
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
    assert_eq!(s.otlp_grpc.max_message_size_mb, 32);
    let s2: Settings = toml::from_str("[otlp_grpc]\nport = 14317\n").unwrap();
    assert_eq!(s2.otlp_grpc.port, 14317);
}

#[test]
fn removed_service_switches_are_rejected() {
    for removed in [
        "[notify]\nenabled = false\n",
        "[scheduled_reports.renderer]\nenabled = false\n",
        "[intelligence]\nenabled = true\n",
        "[otlp_grpc]\nenabled = false\n",
        "[telemetry.self_ingest]\nenabled = true\n",
        "[telemetry.self]\nenabled = true\n",
        "[telemetry.trace]\nself_ingest_enabled = true\n",
        "[telemetry.self_collect]\nlogs_enabled = false\n",
        "[telemetry.self_collect]\nlogs_retention_days = 7\n",
        "[telemetry.self_collect]\ntraces_enabled = false\n",
        "[telemetry.self_collect]\nprofiles_enabled = false\n",
    ] {
        assert!(
            toml::from_str::<Settings>(removed).is_err(),
            "removed switch must be rejected: {removed}"
        );
    }
}

#[test]
fn defaults_roundtrip_through_toml() {
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert_eq!(s.http.port, 5080);
    assert_eq!(s.store.meta.backend, "sqlite");
    assert_eq!(s.store.meta.min_connections, 2);
    assert_eq!(s.store.meta.max_connections, 16);
    assert_eq!(s.store.object.backend, "local");
    assert_eq!(s.alert_manager.dispatch_interval_secs, 10);
    assert_eq!(s.alert_manager.eval_timeout_secs, 10);
    assert_eq!(s.cluster.heartbeat_interval_secs, 5);
    assert_eq!(s.cluster.advertise_addr, "127.0.0.1:5082");
    assert_eq!(s.compactor.target_mb, 512);
    assert_eq!(s.compactor.max_concurrent_groups, 4);
    assert_eq!(s.router.rate_limit.ingest_qps, 1000);
    assert_eq!(s.cache.parquet_file_meta.capacity, 100_000);
    assert_eq!(s.cache.parquet_file_meta.ttl_secs, 60);
    assert_eq!(s.cache.parquet_meta.capacity, 10_000);
    assert_eq!(s.cache.parquet_meta.ttl_secs, 600);
    assert_eq!(s.cache.query_result.capacity, 1_000);
    assert_eq!(s.cache.query_result.ttl_secs, 60);
    assert!(matches!(s.notify.smtp.tls, SmtpTls::Starttls));
    // re-emit + re-parse must be lossless on defaults
    let text = toml::to_string(&s).unwrap();
    let s2: Settings = toml::from_str(&text).unwrap();
    assert_eq!(s2.alert_manager.dispatch_interval_secs, 10);
    assert_eq!(s2.compactor.target_mb, 512);
    assert_eq!(s2.cluster.advertise_addr, "127.0.0.1:5082");
    assert_eq!(s2.cache.query_result.capacity, 1_000);
}

#[test]
fn compactor_target_mb_accepts_legacy_alias() {
    let toml_text = r#"
[compactor]
target_file_size_mb = 256
"#;
    let s: Settings = toml::from_str(toml_text).expect("alias must parse");
    assert_eq!(s.compactor.target_mb, 256);
}

#[test]
fn auth_default_has_no_deprecated_secret() {
    let a = AuthSettings::default();
    assert!(a.deprecated_jwt_secret.is_none());
}

#[test]
fn auth_settings_do_not_serialize_jwt_issuer() {
    let value = toml::Value::try_from(AuthSettings::default()).unwrap();
    assert!(!value.as_table().unwrap().contains_key("issuer"));
}

#[test]
fn auth_legacy_jwt_secret_is_absorbed_into_deprecation_slot() {
    // auth-hardening：旧 [auth].jwt_secret = "..." 应被 alias 吸收，
    // 不再用于签名；deprecation 警告由 main 启动期发出。
    let toml_text = r#"
[auth]
jwt_secret = "legacy-value"
"#;
    let s: Settings = toml::from_str(toml_text).expect("legacy alias must parse");
    assert_eq!(
        s.auth.deprecated_jwt_secret.as_deref(),
        Some("legacy-value")
    );
}

#[test]
fn disk_cache_defaults_when_missing_from_toml() {
    // 缺省 TOML（[cache.disk_cache] 整段都没写）必须落到默认值：
    // dir=./data/cache/parquet / max_size_gb=10（>0 即启用）
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert_eq!(
        s.cache.disk_cache.dir,
        PathBuf::from("./data/cache/parquet")
    );
    assert_eq!(s.cache.disk_cache.max_size_gb, 10);
    assert!(s.cache.disk_cache.is_effectively_enabled());
    assert_eq!(s.cache.disk_cache.max_size_bytes(), 10 * 1024 * 1024 * 1024);

    // 只写 [cache] 段不写 disk_cache 子段也要走默认。
    let only_cache: Settings = toml::from_str("[cache]\n").expect("empty [cache] must parse");
    assert_eq!(only_cache.cache.disk_cache.max_size_gb, 10);
}

#[test]
fn disk_cache_max_size_gb_zero_is_effectively_disabled() {
    let toml_text = r#"
[cache.disk_cache]
max_size_gb = 0
"#;
    let s: Settings = toml::from_str(toml_text).expect("zero-size must parse");
    assert_eq!(s.cache.disk_cache.max_size_gb, 0);
    assert!(!s.cache.disk_cache.is_effectively_enabled());
}

#[test]
fn parquet_file_meta_dump_default_when_missing_from_toml() {
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert!(s.storage.parquet_file_meta_dump.enabled);
    assert_eq!(s.storage.parquet_file_meta_dump.cold_after_days, 30);
    assert_eq!(s.storage.parquet_file_meta_dump.interval_secs, 3600);
    assert_eq!(
        s.storage.parquet_file_meta_dump.max_partitions_per_tick,
        100
    );
    assert_eq!(
        s.storage.parquet_file_meta_dump.partition_level,
        PartitionLevel::Daily
    );
}

#[test]
fn parquet_file_meta_dump_explicit_disable_round_trips() {
    let toml_text = r#"
[storage.parquet_file_meta_dump]
enabled = false
cold_after_days = 90
interval_secs = 7200
max_partitions_per_tick = 50
partition_level = "hourly"
"#;
    let s: Settings = toml::from_str(toml_text).expect("parquet_file_meta_dump must parse");
    assert!(!s.storage.parquet_file_meta_dump.enabled);
    assert_eq!(s.storage.parquet_file_meta_dump.cold_after_days, 90);
    assert_eq!(s.storage.parquet_file_meta_dump.interval_secs, 7200);
    assert_eq!(s.storage.parquet_file_meta_dump.max_partitions_per_tick, 50);
    assert_eq!(
        s.storage.parquet_file_meta_dump.partition_level,
        PartitionLevel::Hourly
    );
}

#[test]
fn partition_level_parses_both_variants() {
    let daily: PartitionLevel = serde_json::from_str("\"daily\"").unwrap();
    let hourly: PartitionLevel = serde_json::from_str("\"hourly\"").unwrap();
    assert_eq!(daily, PartitionLevel::Daily);
    assert_eq!(hourly, PartitionLevel::Hourly);
    assert_eq!(daily.as_str(), "daily");
    assert_eq!(hourly.as_str(), "hourly");
}

#[test]
fn parquet_file_meta_dump_cache_defaults_when_missing_from_toml() {
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert_eq!(s.cache.parquet_file_meta_dump.capacity, 10_000);
    assert_eq!(s.cache.parquet_file_meta_dump.ttl_secs, 600);
}

#[test]
fn parquet_file_meta_dump_cache_capacity_zero_round_trips() {
    let toml_text = r#"
[cache.parquet_file_meta_dump]
capacity = 0
ttl_secs = 1
"#;
    let s: Settings = toml::from_str(toml_text).expect("zero capacity must parse");
    assert_eq!(s.cache.parquet_file_meta_dump.capacity, 0);
    assert_eq!(s.cache.parquet_file_meta_dump.ttl_secs, 1);
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
fn disk_cache_explicit_disable_round_trips() {
    // 关闭 = max_size_gb = 0（无独立 enabled 开关）。
    let toml_text = r#"
[cache.disk_cache]
dir = "/var/cache/molesignal/parquet"
max_size_gb = 0
"#;
    let s: Settings = toml::from_str(toml_text).expect("explicit disk_cache must parse");
    assert_eq!(
        s.cache.disk_cache.dir,
        PathBuf::from("/var/cache/molesignal/parquet")
    );
    assert_eq!(s.cache.disk_cache.max_size_gb, 0);
    assert!(!s.cache.disk_cache.is_effectively_enabled());
}

#[test]
fn streaming_agg_cache_defaults_when_missing_from_toml() {
    // 缺省 TOML（[search.streaming_agg_cache] 整段都没写）必须落到默认值，且默认关闭（capacity=0）。
    let s: Settings = toml::from_str("").expect("empty TOML must parse with defaults");
    assert_eq!(s.search.stream_agg_cache.capacity, 0);
    assert_eq!(s.search.stream_agg_cache.ttl_secs, 300);
    assert_eq!(s.search.stream_agg_cache.safe_lookback_secs, 300);
    assert_eq!(s.search.stream_agg_cache.max_series_per_query, 10_000);
}

#[test]
fn streaming_agg_cache_explicit_enable_round_trips() {
    let toml_text = r#"
[search.stream_agg_cache]
capacity = 64
ttl_secs = 120
safe_lookback_secs = 600
max_series_per_query = 0
"#;
    let s: Settings = toml::from_str(toml_text).expect("streaming_agg_cache must parse");
    // capacity > 0 即启用（无独立 enabled）。
    assert_eq!(s.search.stream_agg_cache.capacity, 64);
    assert_eq!(s.search.stream_agg_cache.ttl_secs, 120);
    assert_eq!(s.search.stream_agg_cache.safe_lookback_secs, 600);
    assert_eq!(s.search.stream_agg_cache.max_series_per_query, 0);
    // 其余 search 子段仍是默认。
    assert_eq!(s.search.max_result_rows, 0);
    assert_eq!(s.search.admission.default_max_concurrent, 0);
}

#[test]
fn wal_defaults_yield_batch_data_64_50() {
    let s: Settings = toml::from_str("[wal]\n").expect("empty [wal] section must parse");
    assert_eq!(s.wal.dir, "./data/wal");
    assert_eq!(s.wal.segment_size_mb, 256);
    assert!(matches!(s.wal.flush_strategy, WalFlushStrategy::Batch));
    assert!(matches!(s.wal.sync_level, WalSyncLevel::Data));
    assert_eq!(s.wal.batch_max_pending, 64);
    assert_eq!(s.wal.batch_max_delay_ms, 50);
}

#[test]
fn wal_legacy_sync_interval_ms_alias_maps_to_batch_max_delay_ms() {
    // 旧 TOML 写 `sync_interval_ms = 200` 应该被映射到 batch_max_delay_ms = 200。
    let toml_text = r#"
[wal]
sync_interval_ms = 200
"#;
    let s: Settings = toml::from_str(toml_text).expect("legacy alias must parse");
    assert_eq!(s.wal.batch_max_delay_ms, 200);
    // 其余字段仍是默认。
    assert!(matches!(s.wal.flush_strategy, WalFlushStrategy::Batch));
    assert!(matches!(s.wal.sync_level, WalSyncLevel::Data));
}

#[test]
fn wal_flush_strategy_none_round_trip() {
    // 显式指定 flush_strategy = "none" 可以回归旧的 never-fsync 行为。
    let toml_text = r#"
[wal]
flush_strategy = "none"
sync_level = "none"
"#;
    let s: Settings = toml::from_str(toml_text).expect("explicit none must parse");
    assert!(matches!(s.wal.flush_strategy, WalFlushStrategy::None));
    assert!(matches!(s.wal.sync_level, WalSyncLevel::None));
    // re-emit + re-parse must be lossless
    let text = toml::to_string(&s.wal).expect("serialize WalSettings");
    let w2: WalSettings = toml::from_str(&text).expect("re-parse");
    assert!(matches!(w2.flush_strategy, WalFlushStrategy::None));
    assert!(matches!(w2.sync_level, WalSyncLevel::None));
}
