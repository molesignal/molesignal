// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 通用聚合工具：service topology rps / err_rate / p95 计算（spec web-shell）。
//!
//! 与 api crate handler 内联版同算法；提到这里仅为单测从 axum 解耦。
//! 后续 把 handler 迁过来时直接调用。

#[derive(Debug, Clone, Copy)]
pub struct EdgeSample {
    pub request_count: u64,
    pub error_count: u64,
    pub p95_us: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EdgeMetrics {
    pub rps: f64,
    pub err_rate: f64,
    pub p95_ms: f64,
}

/// 多个 sample 合一：rps 按 window_secs 平均，err_rate = sum(err)/sum(req)，p95 取 max。
pub fn fold_edge(samples: &[EdgeSample], window_secs: f64) -> EdgeMetrics {
    if samples.is_empty() || window_secs <= 0.0 {
        return EdgeMetrics::default();
    }
    let mut total_req = 0u64;
    let mut total_err = 0u64;
    let mut p95_max = 0i64;
    for s in samples {
        total_req += s.request_count;
        total_err += s.error_count;
        if let Some(p) = s.p95_us {
            p95_max = p95_max.max(p);
        }
    }
    EdgeMetrics {
        rps: (total_req as f64) / window_secs,
        err_rate: if total_req == 0 {
            0.0
        } else {
            (total_err as f64) / (total_req as f64)
        },
        p95_ms: (p95_max as f64) / 1_000.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_yields_default() {
        let m = fold_edge(&[], 60.0);
        assert_eq!(m.rps, 0.0);
        assert_eq!(m.err_rate, 0.0);
    }

    #[test]
    fn folds_request_error_p95() {
        let s = vec![
            EdgeSample {
                request_count: 100,
                error_count: 10,
                p95_us: Some(5_000),
            },
            EdgeSample {
                request_count: 50,
                error_count: 2,
                p95_us: Some(8_000),
            },
        ];
        let m = fold_edge(&s, 60.0);
        assert!((m.rps - 150.0 / 60.0).abs() < 1e-9);
        assert!((m.err_rate - 12.0 / 150.0).abs() < 1e-9);
        assert!((m.p95_ms - 8.0).abs() < 1e-9);
    }
}
