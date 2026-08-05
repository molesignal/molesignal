// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! /metrics 端点单元测试（spec 简化版，无 docker）：
//!
//! 在全局 registry 上注册一个 counter + inc 后调 `gather_text`，验文本含 metric line。
//! 真正的端到端（ingest → query → /metrics HTTP）由 it_ingest_query 类的 fixture 覆盖。

use molesignal::shared::metrics::{gather_text, register_int_counter};

#[test]
fn metrics_endpoint_returns_text_format() {
    let c = register_int_counter("it_metrics_probe_total", "test probe");
    c.inc();
    c.inc();
    let text = gather_text().unwrap();
    assert!(
        text.contains("it_metrics_probe_total"),
        "metric line missing in: {text}"
    );
    // 至少看到一个 HELP/TYPE 头
    assert!(
        text.contains("# HELP") && text.contains("# TYPE"),
        "expected prometheus text format markers"
    );
}
