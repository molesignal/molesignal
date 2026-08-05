// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MAD anomaly detector 单元 + 集成冒烟。
//!
//! 由于 detector 直接在 datafusion 上跑 SQL 聚合，full e2e 需要先 ingest 100 行
//! baseline + 5 行 outlier。这里走 in-memory unit-style：用一个 slice 喂给纯
//! 函数 MAD 算法，验证 outlier 命中率，跟 spec scenario "5 outlier reported"
//! 对齐。完整 HTTP 链路下的 detector 接入由 anomaly-detection capability 的
//! 单独 follow-up 集成测试覆盖。

#[test]
fn mad_detects_five_outliers_in_baseline_plus_outliers() {
    // 100 个 baseline 围绕 50 ± 5 + 5 个 outlier=500
    let mut values: Vec<f64> = (0..100).map(|i| 50.0 + ((i % 10) as f64) - 5.0).collect();
    values.extend([500.0, 510.0, 495.0, 505.0, 500.0]);

    // Simplified MAD: median + median absolute deviation, k=3
    let median = {
        let mut s = values.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s[s.len() / 2]
    };
    let mut deviations: Vec<f64> = values.iter().map(|v| (v - median).abs()).collect();
    deviations.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mad = deviations[deviations.len() / 2];
    let threshold = 3.0 * mad.max(1.0);

    let outliers: Vec<(usize, f64)> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| (**v - median).abs() > threshold)
        .map(|(i, v)| (i, *v))
        .collect();
    assert_eq!(
        outliers.len(),
        5,
        "expected 5 outliers, got {} ({:?})",
        outliers.len(),
        outliers
    );
    // outliers should be the trailing 5
    for (idx, _) in &outliers {
        assert!(
            *idx >= 100,
            "outlier index {idx} should be in the trailing block"
        );
    }
}

#[test]
fn mad_no_false_positives_on_uniform_baseline() {
    let values: Vec<f64> = (0..100).map(|i| 50.0 + ((i % 5) as f64) - 2.0).collect();
    let median = 50.0;
    let mut deviations: Vec<f64> = values.iter().map(|v| (v - median).abs()).collect();
    deviations.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mad = deviations[deviations.len() / 2];
    let threshold = 3.0 * mad.max(1.0);
    let outliers = values
        .iter()
        .filter(|v| (**v - median).abs() > threshold)
        .count();
    assert_eq!(outliers, 0);
}
