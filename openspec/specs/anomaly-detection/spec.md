# Anomaly Detection Capability

## Purpose

基于历史同时段桶（weekday/hour/minute）的 Median Absolute Deviation 异常检测，作为 `AlertRule.kind = "anomaly"` 的求值器；EWMA / iforest 留作未来扩展。

## Requirements

### Requirement: Anomaly AlertRule Kind

`AlertRule.kind` SHALL accept the value `Anomaly`, alongside `Scheduled` and `RealTime`. Anomaly rules carry an additional `anomaly_params: { detector: "mad" | "ewma" | "iforest", lookback_days: u32, k_sigma: f32, min_samples: u32 }` block. Only `detector = "mad"` is implemented in this change; the others SHALL return `Error::Invalid("anomaly detector not yet supported: <name>")`.

#### Scenario: Anomaly rule created with MAD
- **WHEN** an Editor POSTs an `AlertRule` with `kind: "anomaly", anomaly_params: { detector: "mad", lookback_days: 7, k_sigma: 3.0, min_samples: 168 }`
- **THEN** the response is `201 Created`

#### Scenario: Unsupported detector rejected
- **WHEN** the payload requests `detector: "iforest"`
- **THEN** the response is `400 Bad Request` with `{ "error": "anomaly detector not yet supported: iforest" }`

### Requirement: MAD-Based Detector

The `MadDetector` SHALL, on each evaluation tick, fetch historical values from the same `(weekday, hour, minute)` bucket over `lookback_days`, require at least `min_samples` data points, compute the median and Median Absolute Deviation (MAD), derive a robust sigma `1.4826 * MAD`, and flag the current value as anomalous when `|current - median| > k_sigma * robust_sigma`. Sample shortage SHALL fail-open (no incident, metric `anomaly_insufficient_samples_total{rule_id}` increments).

#### Scenario: Anomaly fired on outlier
- **WHEN** the current value is 6σ above the historical median for the matching bucket and `min_samples` is satisfied
- **THEN** an `Incident { rule_id, fingerprint, status: Open }` is created, mirroring the same incident-lifecycle semantics as scheduled rules

#### Scenario: Insufficient history skips evaluation
- **WHEN** fewer than `min_samples` historical points exist
- **THEN** no incident is created; `anomaly_insufficient_samples_total{rule_id} += 1`

> Follow-up: end-to-end integration test `it_anomaly_mad.rs` (task 21.8) requires docker + Postgres testcontainer and is deferred until the e2e staging environment is available.
