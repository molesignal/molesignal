## 1. Configuration And Contracts

- [x] 1.1 Add validated `[ingester.prometheus]` structural-label and chunk-size settings with backward-compatible defaults.
- [x] 1.2 Document the new settings in the example and Kubernetes configurations.

## 2. Prometheus Admission And Chunking

- [x] 2.1 Implement complete-request structural label preflight with bounded rejection reasons and no sensitive values in errors or metrics.
- [x] 2.2 Submit valid remote-write samples in per-metric chunks no larger than `max_samples_per_batch`.
- [x] 2.3 Add unit tests for duplicate/reserved/oversized/excessive labels and bounded chunk construction.

## 3. Rotation State And Scheduling

- [x] 3.1 Track active-generation age and expose deterministic size/age/retry due decisions from `RecordBuilder`.
- [x] 3.2 Make steady-state writes notify only due buffers while preserving forced replay, drain, and explicit flush behavior.
- [x] 3.3 Apply `flush_parallelism` across distinct keys and enforce a per-key single-flight lock across the complete flush transaction.

## 4. Observability And Verification

- [x] 4.1 Add low-cardinality rotation, structural rejection, and flush in-flight metrics with focused unit tests.
- [x] 4.2 Add worker regression tests for under-threshold writes, size/age triggers, bounded cross-stream concurrency, and same-stream ordering.
- [x] 4.3 Run strict OpenSpec validation and one consolidated Rust formatting, license, lint, and test verification round.
