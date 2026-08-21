// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::OnceLock;

use object_store::{Error as OsError, path::Path as ObjPath};
use prometheus::{HistogramVec, IntCounterVec};
use tracing::field;

use crate::shared::metrics::{register_histogram_vec, register_int_counter_vec};

pub(super) fn operation_span(
    backend: &'static str,
    operation: &'static str,
    location: Option<&ObjPath>,
) -> tracing::Span {
    let category = location.map(object_category).unwrap_or("collection");
    let fingerprint = location.and_then(|path| {
        crate::shared::trace_normalization::optional_hmac_fingerprint(path.as_ref())
    });
    tracing::info_span!(
        "object_store.operation",
        otel.kind = "client",
        molesignal.trace.category = "object_store",
        object_store.system = backend,
        object_store.operation = operation,
        molesignal.object.category = category,
        molesignal.object.key_fingerprint = fingerprint.as_deref().unwrap_or(""),
        molesignal.object.bytes = field::Empty,
        molesignal.object.retry_count = field::Empty,
        error.type = field::Empty,
    )
}

pub(super) fn object_category(path: &ObjPath) -> &'static str {
    let value = path.as_ref().to_ascii_lowercase();
    if value.contains("/manifests/") {
        "metadata"
    } else if value.ends_with(".parquet") {
        "parquet"
    } else if value.ends_with(".puffin") || value.contains("tantivy") {
        "search_index"
    } else if value.contains("profile") || value.ends_with(".pprof") {
        "profile"
    } else if value.contains("replay") {
        "rum_replay"
    } else if value.contains("report") {
        "report"
    } else if value.contains("sourcemap") || value.ends_with(".map") {
        "source_map"
    } else {
        "other"
    }
}

pub(super) fn timeout_error(store: &'static str, operation: &str) -> OsError {
    OsError::Generic {
        store,
        source: std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("{operation} deadline exceeded"),
        )
        .into(),
    }
}

pub(super) fn error_reason(error: &OsError) -> &'static str {
    match error {
        OsError::NotFound { .. } => "not_found",
        OsError::InvalidPath { .. } => "invalid_path",
        OsError::NotSupported { .. } | OsError::NotImplemented { .. } => "not_supported",
        OsError::AlreadyExists { .. } => "already_exists",
        OsError::Precondition { .. } => "precondition",
        OsError::NotModified { .. } => "not_modified",
        OsError::PermissionDenied { .. } => "permission_denied",
        OsError::Unauthenticated { .. } => "unauthenticated",
        OsError::UnknownConfigurationKey { .. } => "configuration",
        OsError::Generic { source, .. }
            if source
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::TimedOut) =>
        {
            "timeout"
        }
        OsError::Generic { .. } => "backend",
        OsError::JoinError { .. } => "join",
        _ => "other",
    }
}

static OPS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static BYTES_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static ERRORS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static OP_DUR: OnceLock<HistogramVec> = OnceLock::new();
static HEALTH_DUR: OnceLock<HistogramVec> = OnceLock::new();

pub(super) fn ops_total() -> &'static IntCounterVec {
    OPS_TOTAL.get_or_init(|| {
        register_int_counter_vec(
            "object_store_operations_total",
            "object store operations",
            &["backend", "op"],
        )
    })
}

pub(super) fn bytes_total() -> &'static IntCounterVec {
    BYTES_TOTAL.get_or_init(|| {
        register_int_counter_vec(
            "object_store_bytes_total",
            "object store bytes by op",
            &["backend", "op"],
        )
    })
}

pub(super) fn errors_total() -> &'static IntCounterVec {
    ERRORS_TOTAL.get_or_init(|| {
        register_int_counter_vec(
            "object_store_errors_total",
            "object store errors",
            &["backend", "op", "reason"],
        )
    })
}

pub(super) fn op_dur() -> &'static HistogramVec {
    OP_DUR.get_or_init(|| {
        register_histogram_vec(
            "object_store_op_duration_seconds",
            "object store op duration",
            &["backend", "op"],
            vec![0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0],
        )
    })
}

pub fn health_dur() -> &'static HistogramVec {
    HEALTH_DUR.get_or_init(|| {
        register_histogram_vec(
            "object_store_health_check_duration_seconds",
            "health probe round-trip",
            &["backend"],
            vec![0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
        )
    })
}
