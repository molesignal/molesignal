// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Lintable inventory of tracing policies for global boundaries and long-lived workers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerTracePolicy {
    RootPerRun,
    ChildOfActiveRun,
    TransportBoundary,
    SuppressedSelfTelemetry,
    ProbeExcluded,
}

#[derive(Debug, Clone, Copy)]
pub struct WorkerCoverage {
    pub source: &'static str,
    pub policy: WorkerTracePolicy,
}

pub const WORKER_COVERAGE: &[WorkerCoverage] = &[
    WorkerCoverage {
        source: "src/bootstrap/roles/alert_manager.rs",
        policy: WorkerTracePolicy::ChildOfActiveRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/roles/compactor.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/roles/health_probe.rs",
        policy: WorkerTracePolicy::ProbeExcluded,
    },
    WorkerCoverage {
        source: "src/bootstrap/roles/heartbeat.rs",
        policy: WorkerTracePolicy::TransportBoundary,
    },
    WorkerCoverage {
        source: "src/bootstrap/roles/ingester.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/syslog.rs",
        policy: WorkerTracePolicy::ChildOfActiveRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/acme.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/admission_load_sync.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/cluster/event_sync.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/cluster/gossip.rs",
        policy: WorkerTracePolicy::TransportBoundary,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/parquet_file_meta_dumper.rs",
        policy: WorkerTracePolicy::ChildOfActiveRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/pipeline_exec.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/rca_sweeper.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/rum_replay_retention.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/scheduled_reports.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/search_jobs.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/service_graph/flush.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/service_graph/recompute.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/slow_query_analyzer.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
    WorkerCoverage {
        source: "src/bootstrap/workers/trial_sweeper.rs",
        policy: WorkerTracePolicy::RootPerRun,
    },
];

pub const HTTP_CLIENT_COVERAGE: &[&str] = &[
    "src/api/http/routes/intelligence/mcp/runtime.rs",
    "src/app/trace/export.rs",
    "src/bootstrap/roles/router.rs",
    "src/bootstrap/workers/scheduled_reports.rs",
    "src/infra/connectors/cloudwatch_logs.rs",
    "src/infra/enrichment/mmdb_downloader.rs",
    "src/infra/notify/adapters/lark.rs",
    "src/infra/notify/adapters/slack.rs",
    "src/infra/notify/adapters/webhook.rs",
    "src/infra/sso/jwks.rs",
    "src/infra/sso/oidc.rs",
    "src/intelligence/chat/providers/anthropic.rs",
    "src/intelligence/chat/providers/openai.rs",
];

pub const GRPC_CLIENT_COVERAGE: &[&str] = &[
    "src/api/http/routes/clusters.rs",
    "src/api/http/routes/query.rs",
    "src/app/self_telemetry.rs",
    "src/app/trace/candidate_router.rs",
    "src/app/trace/export.rs",
    "src/bootstrap/workers/cluster/event_sync.rs",
    "src/bootstrap/workers/cluster/gossip.rs",
    "src/infra/query/distributed.rs",
    "src/infra/query/federated.rs",
];

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        fs,
        path::{Path, PathBuf},
    };

    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn read(relative: &str) -> String {
        fs::read_to_string(root().join(relative))
            .unwrap_or_else(|error| panic!("read coverage source {relative}: {error}"))
    }

    fn rust_files_with_spawn(path: &Path, out: &mut BTreeSet<String>) {
        for entry in fs::read_dir(path).expect("read bootstrap source directory") {
            let path = entry.expect("read bootstrap directory entry").path();
            if path.is_dir() {
                rust_files_with_spawn(&path, out);
            } else if path.extension().and_then(|value| value.to_str()) == Some("rs")
                && fs::read_to_string(&path)
                    .expect("read bootstrap Rust source")
                    .contains("tokio::spawn(")
            {
                out.insert(
                    path.strip_prefix(root())
                        .expect("source beneath manifest")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }

    fn rust_files_with_direct_transaction_begin(path: &Path, out: &mut BTreeSet<String>) {
        // Build the searched token from parts so this coverage test does not match its own source.
        let direct_begin = [".", "begin()"].concat();
        for entry in fs::read_dir(path).expect("read Rust source directory") {
            let path = entry.expect("read Rust source directory entry").path();
            if path.is_dir() {
                rust_files_with_direct_transaction_begin(&path, out);
            } else if path.extension().and_then(|value| value.to_str()) == Some("rs")
                && path != root().join("src/sqlx-shim/src/lib.rs")
                && fs::read_to_string(&path)
                    .expect("read Rust source")
                    .contains(&direct_begin)
            {
                out.insert(
                    path.strip_prefix(root())
                        .expect("source beneath manifest")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }

    #[test]
    fn all_registered_http_and_grpc_servers_use_global_trace_layers() {
        let http = read("src/api/http/mod.rs");
        assert!(http.contains("middleware::trace_context_layer"));
        assert!(http.contains("x-request-id"));
        assert!(http.contains("x-trace-id"));

        let grpc = read("src/api/grpc/mod.rs");
        assert_eq!(
            grpc.matches("Server::builder()").count(),
            grpc.matches(".layer(trace::layer::GrpcTraceLayer)").count(),
            "every Tonic server builder must install GrpcTraceLayer"
        );
    }

    #[test]
    fn every_spawned_bootstrap_worker_has_an_explicit_policy() {
        let mut discovered = BTreeSet::new();
        rust_files_with_spawn(&root().join("src/bootstrap/workers"), &mut discovered);
        rust_files_with_spawn(&root().join("src/bootstrap/roles"), &mut discovered);
        if read("src/bootstrap/syslog.rs").contains("tokio::spawn(") {
            discovered.insert("src/bootstrap/syslog.rs".into());
        }
        let registered = WORKER_COVERAGE
            .iter()
            .map(|entry| entry.source.to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(discovered, registered);
    }

    #[test]
    fn sql_object_store_and_outbound_clients_cannot_bypass_shared_boundaries() {
        let sql = read("src/sqlx-shim/src/lib.rs");
        for wrapper in [
            "pub fn query(",
            "pub fn query_as<",
            "pub fn query_scalar<",
            "pub async fn begin(",
            "db.query",
            "db.transaction",
            "MS_TRACE_FINGERPRINT_KEY",
            "keyed_sql_fingerprint",
        ] {
            assert!(sql.contains(wrapper), "missing SQL trace wrapper {wrapper}");
        }
        assert!(
            !sql.contains("fnv1a"),
            "SQL fingerprints must never fall back to an unkeyed hash"
        );
        let mut direct_transaction_bypasses = BTreeSet::new();
        rust_files_with_direct_transaction_begin(
            &root().join("src"),
            &mut direct_transaction_bypasses,
        );
        assert!(
            direct_transaction_bypasses.is_empty(),
            "production SQL transactions bypass the traced facade: {direct_transaction_bypasses:?}"
        );
        assert_eq!(
            read("src/infra/persistence/pool.rs")
                .matches("log_statements(tracing::log::LevelFilter::Off)")
                .count(),
            2,
            "both PostgreSQL pool constructors must suppress full SQL logging"
        );

        let object = read("src/infra/storage/object/production.rs");
        assert!(object.contains("impl ObjectStore for ProductionObjectStore"));
        assert!(!object.contains("pub fn inner("));
        assert!(read("src/bootstrap/core.rs").contains("ProductionObjectStore::wrap"));

        for source in HTTP_CLIENT_COVERAGE {
            assert!(
                read(source).contains("http_trace::send"),
                "{source} bypasses shared HTTP tracing"
            );
        }
        for source in GRPC_CLIENT_COVERAGE {
            assert!(
                read(source).contains("grpc_trace::call"),
                "{source} bypasses shared gRPC tracing"
            );
        }
    }
}
