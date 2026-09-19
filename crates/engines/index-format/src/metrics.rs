// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puffin reader / directory 的 Prometheus metric family。
//!
//! 全部走全局 default registry（`prometheus::default_registry`）以便 caller 不必
//! 注入自定义 registry。`AlreadyReg` 容错。

use std::sync::OnceLock;

use prometheus::{Histogram, HistogramOpts, IntCounter, Opts, register_int_counter};

struct Metrics {
    footer_bytes_read: IntCounter,
    blob_range_reads: IntCounter,
    directory_open_total: IntCounter,
    directory_open_seconds: Histogram,
}

fn metrics() -> &'static Metrics {
    static M: OnceLock<Metrics> = OnceLock::new();
    M.get_or_init(|| {
        let footer_bytes_read = match register_int_counter!(
            "tantivy_puffin_footer_bytes_read_total",
            "bytes read for puffin footer parse (tail + payload)"
        ) {
            Ok(c) => c,
            Err(prometheus::Error::AlreadyReg) => {
                // re-resolve via default registry
                let opts = Opts::new(
                    "tantivy_puffin_footer_bytes_read_total",
                    "bytes read for puffin footer parse (tail + payload)",
                );
                IntCounter::with_opts(opts).expect("counter")
            }
            Err(e) => panic!("register tantivy_puffin_footer_bytes_read_total: {e}"),
        };
        let blob_range_reads = match register_int_counter!(
            "tantivy_puffin_blob_range_reads_total",
            "one increment per puffin blob sub-range get_range call"
        ) {
            Ok(c) => c,
            Err(prometheus::Error::AlreadyReg) => {
                let opts = Opts::new(
                    "tantivy_puffin_blob_range_reads_total",
                    "one increment per puffin blob sub-range get_range call",
                );
                IntCounter::with_opts(opts).expect("counter")
            }
            Err(e) => panic!("register tantivy_puffin_blob_range_reads_total: {e}"),
        };
        let directory_open_total = match register_int_counter!(
            "tantivy_puffin_directory_open_total",
            "PuffinDirReader::from_object_store invocations"
        ) {
            Ok(c) => c,
            Err(prometheus::Error::AlreadyReg) => {
                let opts = Opts::new(
                    "tantivy_puffin_directory_open_total",
                    "PuffinDirReader::from_object_store invocations",
                );
                IntCounter::with_opts(opts).expect("counter")
            }
            Err(e) => panic!("register tantivy_puffin_directory_open_total: {e}"),
        };
        let directory_open_seconds = {
            let opts = HistogramOpts::new(
                "tantivy_puffin_directory_open_seconds",
                "footer fetch + parse latency for PuffinDirReader",
            )
            .buckets(vec![0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]);
            let h = Histogram::with_opts(opts).expect("histogram");
            match prometheus::default_registry().register(Box::new(h.clone())) {
                Ok(()) | Err(prometheus::Error::AlreadyReg) => h,
                Err(e) => panic!("register tantivy_puffin_directory_open_seconds: {e}"),
            }
        };
        Metrics {
            footer_bytes_read,
            blob_range_reads,
            directory_open_total,
            directory_open_seconds,
        }
    })
}

pub(crate) fn footer_bytes_read() -> &'static IntCounter {
    &metrics().footer_bytes_read
}

pub(crate) fn blob_range_reads() -> &'static IntCounter {
    &metrics().blob_range_reads
}

pub(crate) fn directory_open_total() -> &'static IntCounter {
    &metrics().directory_open_total
}

pub(crate) fn directory_open_seconds() -> &'static Histogram {
    &metrics().directory_open_seconds
}
