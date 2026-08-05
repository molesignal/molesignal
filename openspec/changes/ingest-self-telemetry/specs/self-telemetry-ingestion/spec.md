## ADDED Requirements

### Requirement: Self-Telemetry Runtime Configuration

The system SHALL provide an opt-in self-telemetry runtime configured under `telemetry.self_collect`. The runtime SHALL be disabled by default. When enabled, it SHALL resolve the immutable `_sys` organization from the system-organization constant after identity bootstrap, use the exact stream name `_molesignal`, always enable logs and profiles, and enable metrics unless `metrics_enabled = false`. Trace self-ingest SHALL additionally require the effective `telemetry.trace.enabled` policy. The configuration schema SHALL NOT expose or accept the legacy `telemetry.self_ingest` section, `telemetry.trace.self_ingest_enabled`, `telemetry.self_collect.org_slug`, `logs_enabled`, `traces_enabled`, or `profiles_enabled`.

#### Scenario: Enabled runtime resolves the system organization

- **WHEN** `telemetry.self_collect.enabled = true` and `_sys` has been prepared
- **THEN** the runtime starts after the ingestion service is ready
- **AND** every self-telemetry batch carries the `_sys` organization ID

#### Scenario: Removed organization override is rejected

- **WHEN** configuration contains `telemetry.self_collect.org_slug`
- **THEN** configuration parsing fails with an unknown-field error
- **AND** no tenant organization can be selected as a fallback

#### Scenario: Removed per-signal switches are rejected

- **WHEN** configuration contains the legacy `telemetry.self_ingest` section, `telemetry.trace.self_ingest_enabled`, or `telemetry.self_collect.logs_enabled`, `traces_enabled`, or `profiles_enabled`
- **THEN** configuration parsing fails with an unknown-field error

#### Scenario: Disabled runtime has no storage side effects

- **WHEN** `telemetry.self_collect.enabled = false`
- **THEN** no logs, metrics, traces, or profiles `_molesignal` stream is created
- **AND** no log worker, metrics snapshot loop, or scheduled profile capture is started
- **AND** Trace capture and an independently configured external OTLP exporter retain their configured behavior

### Requirement: Four Typed System Streams

For every enabled signal, the system SHALL create or reuse a stream named `_molesignal` in `_sys` with the matching `StreamType`. Logs, metrics, traces, and profiles SHALL therefore occupy four independently schematized and queryable streams identified as `(_sys_org_id, "_molesignal", stream_type)`. System-created streams SHALL use the configured self-telemetry retention, defaulting to seven days.

#### Scenario: All signals create distinct typed streams

- **WHEN** all four self-ingest signals are enabled
- **THEN** system stream listing for `_sys` contains `logs/_molesignal`, `metrics/_molesignal`, `traces/_molesignal`, and `profiles/_molesignal`
- **AND** each stream has its own schema and retention

#### Scenario: Metrics can be disabled independently

- **WHEN** self-ingest is enabled with `metrics_enabled = false`
- **THEN** logs and profiles system streams are available
- **AND** the runtime does not create `metrics/_molesignal`
- **AND** the traces system stream is available only when the effective `telemetry.trace` policy also enables capture

### Requirement: Stable Resource Identity

Every self-telemetry record SHALL include resource identity sufficient to distinguish nodes and releases: `service.name = "molesignal"`, `service.version`, `service.instance.id`, `service.role`, and `node.id`. `service.instance.id` SHALL remain stable for the process lifetime and SHALL differ after a process restart unless a persisted node instance ID is configured.

#### Scenario: Two nodes remain distinguishable

- **WHEN** an ingester node and a querier node write self telemetry to `_sys`
- **THEN** their records have different `service.instance.id` or `node.id` values
- **AND** `service.role` identifies the role set that emitted each record

### Requirement: Self Metrics Collection

At the configured interval, the runtime SHALL snapshot the in-process Prometheus registry without making an HTTP request to `/metrics` and append normalized metric samples to `metrics/_molesignal`. Every row SHALL include `metric_name`, `metric_kind`, `value`, the original bounded labels, the common resource identity, and the snapshot timestamp. Counters and gauges SHALL produce one row per label set; histogram and summary families SHALL preserve bucket or quantile values plus count and sum series.

#### Scenario: Counter and histogram are queryable

- **WHEN** the registry contains a counter and a histogram at snapshot time
- **THEN** `metrics/_molesignal` receives rows for the counter value
- **AND** it receives the histogram bucket, count, and sum values without losing their labels

#### Scenario: Metrics collection does not scrape the HTTP endpoint

- **WHEN** `/metrics` is unavailable on a node but the registry is initialized
- **THEN** self metrics continue to be captured from the registry
- **AND** no loopback network request is attempted

### Requirement: Self Log and Trace Collection

The tracing subscriber SHALL mirror accepted structured events to `logs/_molesignal` and completed spans to `traces/_molesignal` without replacing console/file logging or an explicitly configured external OTLP trace exporter. Log records SHALL retain timestamp, level, target, message, structured fields, source location when available, and active trace/span IDs. Trace records SHALL retain trace ID, span ID, parent span ID, name, kind, start/end time, duration, status, attributes, events, and resource identity.

#### Scenario: Correlated log and trace are stored

- **WHEN** a request span emits a structured error event
- **THEN** the completed span appears in `traces/_molesignal`
- **AND** the event appears in `logs/_molesignal` with the same trace and span IDs

#### Scenario: Existing sinks remain active

- **WHEN** console logging and an external OTLP endpoint are configured together with self-ingest
- **THEN** the log is still written to the console
- **AND** the span is still exported to the external endpoint
- **AND** both are also queued for internal storage

### Requirement: Self Profile Collection

When `telemetry.self_collect.enabled = true`, the runtime SHALL periodically capture configured profile kinds and persist canonical pprof data through the existing continuous-profiling archive path with metadata in `profiles/_molesignal`. CPU captures SHALL be time-bounded and non-overlapping. Heap capture SHALL be enabled only on supported allocator/platform combinations; unsupported kinds SHALL be reported through availability metrics without stopping other signals.

#### Scenario: Scheduled CPU profile is archived

- **WHEN** self-ingest is enabled with CPU in `profile_kinds` and a valid interval and duration
- **THEN** one capture produces a canonical pprof object in object storage
- **AND** its metadata row is appended to `profiles/_molesignal` with `service = "molesignal"` and `profile_type = "cpu"`

#### Scenario: Unsupported heap profiling degrades safely

- **WHEN** self-ingest is enabled with heap in `profile_kinds` on a build without a supported heap profiler
- **THEN** the runtime records the profile kind as unavailable and skips that capture
- **AND** logs, metrics, traces, and supported profile kinds continue operating

### Requirement: Recursion Prevention and Bounded Backpressure

Self-telemetry callbacks SHALL never synchronously wait for storage. Each signal SHALL use a bounded non-blocking queue, and the ingestion worker SHALL execute inside an explicit suppression scope so events and spans produced by schema lookup, masking, WAL append, remote routing, profile archival, retries, and exporter diagnostics are not re-enqueued. Queue overflow, conversion failures, ingest failures, retry attempts, and dropped records SHALL be exposed through bounded-cardinality Prometheus metrics by signal and reason.

#### Scenario: Internal ingestion does not feed itself

- **WHEN** writing one self log generates tracing events inside the ingestion and storage stack
- **THEN** those internal events are excluded from the self-telemetry queues
- **AND** the original log causes at most one stored log record

#### Scenario: A log storm cannot block request handling

- **WHEN** producers emit logs faster than the bounded log queue can drain
- **THEN** producer threads continue without awaiting storage
- **AND** excess records are dropped
- **AND** `self_telemetry_dropped_total{signal="logs",reason="queue_full"}` increases

### Requirement: Role-Aware Routing and Lifecycle

An ingester or standalone node SHALL write self telemetry through its local internal ingestion path. A node without the ingester role SHALL route batches through the authenticated cluster ingest path to a live ingester while retaining the originating node's resource identity. Producers SHALL stop and queued batches SHALL be given a bounded flush opportunity before node drain begins; lack of an ingester or timeout SHALL never prevent process shutdown.

#### Scenario: Querier telemetry reaches an ingester

- **WHEN** a querier-only node emits self telemetry and a healthy ingester is registered
- **THEN** the querier routes the batch to that ingester
- **AND** the stored record still identifies the querier as the emitting node

#### Scenario: Shutdown is bounded

- **WHEN** the process receives a shutdown signal with queued self telemetry
- **THEN** producers stop and the worker attempts to flush within the configured timeout before ingestion drain starts
- **AND** the process proceeds with shutdown after the timeout even if the queue is not empty
