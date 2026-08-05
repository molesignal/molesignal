## 1. Configuration And Wiring

- [x] 1.1 Add validated process-memory, adaptive-rotation, and Prometheus cardinality settings with safe defaults.
- [x] 1.2 Document all new settings in the example and Kubernetes configurations.
- [x] 1.3 Construct and expose one shared Prometheus series admission registry through storage bootstrap and `AppState`.

## 2. Active-Series Admission

- [x] 2.1 Implement canonical hashed series identities without retaining raw metric or label values.
- [x] 2.2 Implement atomic per-request process/org/metric active caps, new-series rate admission, and idle expiry.
- [x] 2.3 Run complete-request series admission after structural preflight and before Prometheus chunk persistence.
- [x] 2.4 Add focused fingerprint, cap, existing-series, rate, expiry, and protocol-boundary tests.

## 3. Process Memory Admission

- [x] 3.1 Split `BufferPool` responsibilities and add atomic RAII memory reservations with bounded metrics.
- [x] 3.2 Reserve serialized batch bytes before WAL append and attach accounting to active generations.
- [x] 3.3 Preserve accounting across detach/failure retry, release it after successful flush, and force-reserve replay.
- [x] 3.4 Add regression tests for pre-WAL rejection, detached charging, failure retry, and release.

## 4. Adaptive Rotation And Flush Cleanup

- [x] 4.1 Implement per-stream EWMA encoded/raw feedback with configured min/target/max thresholds.
- [x] 4.2 Use adaptive thresholds for size decisions and record feedback only after successful Parquet metadata commit.
- [x] 4.3 Delete uploaded Parquet/Tantivy outputs on metadata insert failure while preserving the original error and retry generation.
- [x] 4.4 Add adaptive-threshold, clamp, disabled-mode, and orphan-cleanup tests.

## 5. Observability And Verification

- [x] 5.1 Add low-cardinality series, memory, compression, and adaptive-target metrics.
- [x] 5.2 Strictly validate OpenSpec and complete one consolidated formatting, license, clippy, and unit-test round.
