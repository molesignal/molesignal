// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Metrics of the object-storage WAL.
//!
//! - `wal_group_commits_total{outcome}`: outcome ∈ {ok, store_error, lease_lost}.
//! - `wal_group_commit_duration_seconds`: from taking a group to its object being written.
//! - `wal_group_size_bytes`: the size of each WAL object written.
//! - `wal_takeover_rows_total`: rows a takeover published for another writer.
//! - `wal_writer_fenced_total`: times this process found its writer fenced and took a new epoch.

use std::sync::OnceLock;

use prometheus::{Histogram, IntCounter, IntCounterVec};
use signals::metrics::{register_histogram, register_int_counter, register_int_counter_vec};

pub(crate) const OK: &str = "ok";
pub(crate) const STORE_ERROR: &str = "store_error";
pub(crate) const LEASE_LOST: &str = "lease_lost";

fn group_commits() -> &'static IntCounterVec {
    static C: OnceLock<IntCounterVec> = OnceLock::new();

    C.get_or_init(|| {
        register_int_counter_vec(
            "wal_group_commits_total",
            "WAL group commits by outcome",
            &["outcome"],
        )
    })
}

fn group_duration() -> &'static Histogram {
    static H: OnceLock<Histogram> = OnceLock::new();

    H.get_or_init(|| {
        register_histogram(
            "wal_group_commit_duration_seconds",
            "Time from taking a WAL group to its object being written",
            vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 10.0,
            ],
        )
    })
}

fn group_size() -> &'static Histogram {
    static H: OnceLock<Histogram> = OnceLock::new();

    H.get_or_init(|| {
        register_histogram(
            "wal_group_size_bytes",
            "Size of each WAL object written",
            prometheus::exponential_buckets(4096.0, 4.0, 8).expect("static buckets"),
        )
    })
}

fn takeover_rows() -> &'static IntCounter {
    static C: OnceLock<IntCounter> = OnceLock::new();

    C.get_or_init(|| {
        register_int_counter(
            "wal_takeover_rows_total",
            "Rows a WAL takeover published for another writer",
        )
    })
}

fn writer_fenced() -> &'static IntCounter {
    static C: OnceLock<IntCounter> = OnceLock::new();

    C.get_or_init(|| {
        register_int_counter(
            "wal_writer_fenced_total",
            "Times this process found its WAL writer fenced and took a new epoch",
        )
    })
}

pub(crate) fn observe_group(outcome: &str, seconds: f64, bytes: usize) {
    group_commits().with_label_values(&[outcome]).inc();
    group_duration().observe(seconds);

    if outcome == OK {
        group_size().observe(bytes as f64);
    }
}

pub(crate) fn inc_takeover_rows(rows: usize) {
    takeover_rows().inc_by(rows as u64);
}

pub(crate) fn inc_writer_fenced() {
    writer_fenced().inc();
}
