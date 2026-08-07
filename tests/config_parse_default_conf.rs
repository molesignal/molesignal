// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 校验仓库内置配置样例能被 Settings 解析。
use molesignal::config::Settings;

#[test]
fn repo_default_conf_parses() {
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("conf/config.toml"),
    )
    .expect("read conf");
    let s: Settings = toml::from_str(&raw).expect("parse conf");
    assert_eq!(s.store.meta.backend, "postgres");
    assert_eq!(s.store.meta.min_connections, 2);
    assert_eq!(s.store.meta.max_connections, 16);
    assert_eq!(s.store.object.backend, "local");
    assert_eq!(s.alert_manager.dispatch_interval_secs, 10);
    assert_eq!(s.alert_manager.eval_timeout_secs, 10);
    assert_eq!(s.router.rate_limit.ingest_qps, 1000);
    assert_eq!(s.cluster.heartbeat_interval_secs, 5);
    assert_eq!(s.cluster.advertise_addr, "127.0.0.1:5082");
    assert_eq!(s.compactor.target_mb, 512);
    assert_eq!(s.compactor.max_concurrent_groups, 4);
    assert_eq!(s.cache.parquet_file_meta.capacity, 100_000);
    assert_eq!(s.cache.parquet_meta.ttl_secs, 600);
    assert_eq!(s.cache.query_result.capacity, 1_000);
    assert!(s.telemetry.self_collect.enabled);
    assert!(!s.profiling.enabled);
}

#[test]
fn k8s_configmap_conf_parses() {
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("deploy/k8s/10-configmap.yaml"),
    )
    .expect("read k8s configmap");
    let embedded = extract_configmap_toml(&raw);
    let s: Settings = toml::from_str(&embedded).expect("parse k8s configmap conf");

    assert_eq!(s.store.meta.backend, "postgres");
    assert_eq!(s.store.meta.min_connections, 2);
    assert_eq!(s.store.meta.max_connections, 16);
    assert_eq!(s.store.object.backend, "s3");
    assert_eq!(s.wal.batch_max_delay_ms, 50);
    assert_eq!(s.querier.auto_async_threshold_rows, 50_000_000);
    assert_eq!(s.compactor.retention_days, 30);
    assert_eq!(s.cache.parquet_file_meta.capacity, 10_000);
    assert_eq!(s.cache.parquet_meta.capacity, 2_000);
}

fn extract_configmap_toml(raw: &str) -> String {
    let mut in_block = false;
    let mut out = String::new();

    for line in raw.lines() {
        if in_block {
            if let Some(stripped) = line.strip_prefix("    ") {
                out.push_str(stripped);
                out.push('\n');
            } else if line.trim().is_empty() {
                out.push('\n');
            } else {
                break;
            }
        } else if line.trim() == "config.toml: |" {
            in_block = true;
        }
    }

    assert!(!out.is_empty(), "config.toml block must exist");
    out
}
