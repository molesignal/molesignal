## 1. Configuration and Ingestion Contracts

- [x] 1.1 Add validated `telemetry.self_collect` and `[profiling]` settings, defaults, environment compatibility overrides, example TOML, and config parsing tests.
- [x] 1.2 Introduce one shared `_molesignal` reserved-name predicate and enforce it across public HTTP, OTLP, Prometheus, compatibility, connector, profile, gRPC, stream mutation, and pipeline target paths.
- [x] 1.3 Add a non-user-serializable internal ingestion entry point for self telemetry; preserve schema evolution, masking, WAL, and drain behavior while skipping user pipelines.
- [x] 1.4 Add unit tests proving external callers cannot assert internal origin, internal batches can reach `_molesignal`, and drain rejects new internal batches.
- [x] 1.5 Resolve the immutable `_sys` organization after identity bootstrap and idempotently pre-create enabled typed `_molesignal` streams with configured retention.

## 2. Telemetry Capture Primitives

- [x] 2.1 Refactor telemetry initialization to return a lifecycle guard with late-bound, bounded log/trace hooks while preserving console/file output and external OTLP export.
- [x] 2.2 Implement a structured `tracing::Event` visitor that emits log records with message, metadata, source location, structured fields, resource identity, and active trace/span IDs.
- [x] 2.3 Implement a finished-span exporter and extract shared OTLP trace-to-`RawEvent` normalization so internal and public OTLP traces use the same field contract.
- [x] 2.4 Add a structured Prometheus registry snapshot API and normalize counters, gauges, histogram buckets/count/sum, and summary quantiles/count/sum into metrics events.
- [x] 2.5 Register bounded-cardinality self-exporter queue, accepted, dropped, failure, retry, last-success, and profile-availability metrics.
- [x] 2.6 Add process resource identity construction with stable process-lifetime instance ID, node ID, role set, version, and `service.name = "molesignal"`.

## 3. Self-Telemetry Runtime

- [x] 3.1 Implement per-signal bounded queues, batching by max events/max delay, non-blocking producer `try_send`, and explicit drop-reason accounting.
- [x] 3.2 Implement task/thread suppression scopes and exporter target filtering; add a regression test proving one internal log write cannot recursively enqueue another.
- [x] 3.3 Implement the interval metrics snapshot producer and write normalized samples to `metrics/_molesignal` without loopback HTTP.
- [x] 3.4 Implement local internal batch delivery for standalone/ingester roles and preserve producer resource identity.
- [x] 3.5 Implement authenticated cluster delivery for non-ingester roles with ingester selection, bounded retry/backoff, queue age limits, and failure metrics.
- [x] 3.6 Activate the runtime only after org resolution, stream creation, and ingestion wiring; keep disabled mode free of workers and stream side effects.
- [x] 3.7 Integrate bounded `stop_and_flush` before `DrainController::begin_drain()` and verify shutdown proceeds after timeout.

## 4. pprof Capture and Self Profiles

- [x] 4.1 Add a feature-gated CPU sampler capable of emitting canonical pprof protobuf and verify supported release targets build with the required unwind/frame-pointer settings.
- [x] 4.2 Introduce a shared CPU capture service with 1–120 second validation, process-wide non-overlap control, `409 + Retry-After`, and deterministic test seams.
- [x] 4.3 Implement the supported jemalloc heap-to-`NormalizedProfile`/canonical-pprof adapter with fixtures; return explicit `501` availability errors on unsupported builds.
- [x] 4.4 Add the node-local profiling listener on every role with `/debug/pprof/profile` and `/debug/pprof/heap`, loopback defaults, remote-address enforcement, and remote Administrator authorization.
- [x] 4.5 Refactor `/api/v1/debug/profile/{cpu,heap}` into compatibility aliases backed by the same capture service and response contract.
- [x] 4.6 Refactor continuous-profile storage to accept a trusted metadata stream selector while keeping every public profile ingress fixed to `profiles/default`.
- [x] 4.7 Implement scheduled self profile capture with configurable kinds/interval/duration, shared CPU concurrency control, and writes to `profiles/_molesignal`.
- [x] 4.8 Persist successful on-demand captures asynchronously when self profiles are enabled, without changing or truncating the completed pprof response on persistence failure.

## 5. Integration and Safety Verification

- [x] 5.1 Add a standalone integration test that enables all four signals and verifies typed `_molesignal` streams, resource identity, queryability, profile blob archival, and configured retention.
- [x] 5.2 Add conversion tests for correlated log/trace IDs and for every Prometheus family shape, including histogram and summary labels.
- [x] 5.3 Add stress tests for queue overflow, non-blocking producers, bounded cardinality, batching, retry limits, and suppression under ingestion-generated telemetry.
- [x] 5.4 Add split-role integration coverage showing querier/compactor telemetry routes to an ingester while retaining the producer node identity.
- [x] 5.5 Add public API tests proving `_molesignal` writes and mutations are forbidden across protocols while authorized management-org queries remain allowed.
- [x] 5.6 Add pprof endpoint tests for valid CPU protobuf, supported/unsupported heap behavior, aliases, disabled 404, localhost enforcement, remote admin authorization, and concurrent capture rejection.
- [x] 5.7 Add startup/shutdown tests for missing target-org failure, disabled no-op behavior, bounded startup buffering, pre-drain flush, and flush timeout.

## 6. Documentation and Final Validation

- [x] 6.1 Document self-ingest architecture, storage/retention cost, recursion/drop semantics, management-org access, multi-role routing, and example queries for all four streams.
- [x] 6.2 Document the profiling listener, pprof commands, security defaults, CPU/heap platform support, compatibility paths, and profile overhead.
- [x] 6.3 Update API/config references and upgrade notes, including the breaking reservation and migration guidance for an existing user stream named `_molesignal`.
- [x] 6.4 Run formatting, focused unit/integration tests, full `cargo test`, and `cargo clippy --all-targets --all-features`; record any platform-gated profiling exclusions.
  - Formatting, focused/full tests, and exact all-features Clippy passed. Platform-gated profiling exclusions are recorded in `validation.md`.
