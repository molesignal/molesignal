## MODIFIED Requirements

### Requirement: Structured Logging

`molesignal_shared::telemetry::init` SHALL initialize `tracing_subscriber` with either JSON or text formatting based on `telemetry.log_format`. Logging SHALL respect `telemetry.log_level` as the log filter and accept per-target overrides through `RUST_LOG`. Trace Span filtering SHALL use an independent `trace.filter`; changing either filter SHALL NOT implicitly change the other. Every log event emitted inside a recorded Span SHALL carry `trace_id` and `span_id`, but ordinary log events SHALL NOT automatically be copied into Span Events.

#### Scenario: JSON format selected
- **WHEN** `telemetry.log_format = "json"`
- **THEN** every log line is a single JSON object containing at minimum `timestamp`, `level`, `target`, `message`, and any active span fields
- **AND** an event inside a recorded Span contains `trace_id` and `span_id`

#### Scenario: RUST_LOG overrides log level only
- **WHEN** `RUST_LOG=molesignal_infra=trace` is set, `telemetry.log_level = "info"`, and `trace.filter = "warn"`
- **THEN** logging uses `info` by default with `molesignal_infra::*` at `trace`
- **AND** Trace Span recording still follows the independent `warn` filter

#### Scenario: Ordinary log is not duplicated into Trace
- **WHEN** an info log is emitted inside a Span without being marked as an explicit Span Event
- **THEN** the log sink receives the correlated log
- **AND** the stored Span does not contain a duplicate Event

### Requirement: OTLP Trace Export

When an external Trace exporter is configured, the unified Trace pipeline SHALL export retained CanonicalSpans through `opentelemetry-otlp` using an explicitly selected `grpc` or `http/protobuf` protocol, defaulting to gRPC. It SHALL support bounded batching, timeout, compression, custom authentication metadata/headers, TLS custom CA, and optional mTLS. Credentials SHALL be resolved only from environment variables or secret references and SHALL never appear in logs, traces, config diffs, or API responses.

The external sink SHALL receive the same sampling decision as self-ingest but SHALL use an independent non-blocking queue and retry state. Runtime exporter failures SHALL be fail-open; syntactically invalid endpoint, protocol, TLS, authentication, sampling, or security configuration SHALL fail startup before roles start.

Resource identity SHALL include `service.namespace = "molesignal"`, role-aware `service.name`, `service.version`, stable `service.instance.id`, `deployment.environment.name`, `node.id`, `cluster.id`, and available cloud region/zone fields.

#### Scenario: gRPC endpoint configured
- **WHEN** external export is enabled with protocol `grpc` and endpoint `https://collector:4317`
- **THEN** only retained spans are exported with the required Resource attributes
- **AND** its queue, retries, drops, and health are independent from self-ingest

#### Scenario: HTTP/protobuf endpoint configured
- **WHEN** external export is enabled with protocol `http/protobuf` and endpoint `https://collector:4318/v1/traces`
- **THEN** retained spans are exported using OTLP HTTP/protobuf with the configured TLS and authentication settings

#### Scenario: External endpoint absent
- **WHEN** no external exporter endpoint is configured
- **THEN** no external Trace network calls are made
- **AND** local logging and enabled self-ingest tracing continue operating

#### Scenario: Invalid static configuration fails fast
- **WHEN** the protocol, URL, certificate, key, CA, or secret reference is invalid
- **THEN** initialization returns an actionable error and `main()` exits before any role starts

#### Scenario: Runtime collector failure is fail-open
- **WHEN** a valid configured collector becomes unavailable after startup
- **THEN** the exporter performs bounded exponential-backoff retries
- **AND** queue overflow or retry expiry drops spans with reason metrics
- **AND** application requests and the self-ingest sink continue

#### Scenario: Self-export loop is rejected
- **WHEN** self-ingest is enabled and the external endpoint resolves to the same MoleSignal cluster
- **THEN** the external sink is rejected unless `allow_self_export = true`
- **AND** an explicit override still uses recursion suppression and span deduplication

### Requirement: Request ID Propagation

Every HTTP request SHALL have a validated `X-Request-Id` generated when absent or invalid. The ID SHALL be added to the request Span and correlated logs, echoed in the response, and propagated as trusted internal `request.id` Baggage only to allowlisted internal targets. Every traced HTTP response SHALL also expose `X-Trace-Id`; gRPC errors SHALL include the Trace ID in response metadata. Response headers SHALL be exposed through configured CORS policy where applicable.

#### Scenario: Caller-provided id is preserved
- **WHEN** a request arrives with a valid `X-Request-Id: abc-123`
- **THEN** all log lines for that request include `request_id = "abc-123"`
- **AND** the response carries the same `X-Request-Id` and the active `X-Trace-Id`

#### Scenario: Invalid request ID is replaced
- **WHEN** a caller provides an overlong or malformed request ID
- **THEN** the server generates a safe replacement
- **AND** it does not echo or propagate the untrusted value

#### Scenario: gRPC error is correlatable
- **WHEN** a traced gRPC request returns an error status
- **THEN** response metadata contains the corresponding Trace ID
- **AND** logs for the error contain the same trace and span IDs

## ADDED Requirements

### Requirement: Dynamic Trace Runtime Policy

The system SHALL persist dynamic Trace policy in `_sys` and expose it through `/api/v1/system/telemetry` to callers with `SystemTelemetryManage`. Dynamic policy SHALL cover runtime enablement, normal sampling ratio, ordered sampling rules, slow thresholds, decision window, queue/cache soft limits, and per-Span/per-Trace limits. Static exporter protocol, endpoint, TLS, authentication, and Resource identity SHALL remain deployment configuration and SHALL require restart.

Enablement precedence SHALL be deployment force-disable, then persisted `_sys` policy, then code default enabled. A policy update SHALL atomically affect new Traces; an in-flight Trace SHALL remain bound to the policy version under which it started.

#### Scenario: Deployment force-disable wins
- **WHEN** deployment force-disable is true but persisted `_sys` policy is enabled
- **THEN** no new application Trace candidates are recorded or exported
- **AND** the API reports that deployment policy is the effective disable source

#### Scenario: Sampling policy changes online
- **WHEN** an authorized platform administrator changes the normal ratio from 10% to 20%
- **THEN** new Traces use the new version without process restart
- **AND** unresolved Traces continue using their original policy version

### Requirement: Trace Pipeline Health and Alerts

The system SHALL expose bounded-cardinality metrics for Span generation, acceptance, sampling decisions and reasons, duplicate/conflict/late/partial records, sink queue depth/capacity, tail-cache occupancy, decision latency, retries, exported/dropped records, and exporter latency. Trace runtime failure SHALL not make liveness/readiness fail, but detailed health SHALL report `degraded`.

Default alerts SHALL cover sustained exporter failure, queue or tail-cache utilization above 80%, sustained drop rate above 1%, and failure to load `_sys`, License, or dynamic Trace policy. Metrics labels SHALL NOT include trace ID, organization ID, raw route, raw object key, or other unbounded identifiers.

#### Scenario: Exporter degradation is visible but non-fatal
- **WHEN** one Trace sink fails continuously
- **THEN** `/healthz` and `/readyz` continue returning success for an otherwise healthy data plane
- **AND** detailed health identifies the failed sink as degraded
- **AND** the relevant failure and queue alerts become active

### Requirement: Bounded Trace Shutdown

On graceful shutdown, the system SHALL stop accepting new Trace candidates, finalize decidable traces, and flush self-ingest and external sink queues before the existing ingestion drain. The flush SHALL have a configurable timeout defaulting to ten seconds; timeout SHALL record remaining counts and SHALL NOT prevent process exit.

#### Scenario: Shutdown flush succeeds
- **WHEN** shutdown begins with retained spans queued and both sinks healthy
- **THEN** queued spans are exported before ingestion drain and process exit

#### Scenario: Shutdown flush times out
- **WHEN** a sink cannot flush within ten seconds
- **THEN** the system records the unexported count and timeout reason
- **AND** shutdown proceeds without waiting indefinitely

