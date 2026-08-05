## ADDED Requirements

### Requirement: Structured Logging

`molesignal_shared::telemetry::init` SHALL initialize `tracing_subscriber` with either JSON or text formatting based on `telemetry.log_format`, respecting `telemetry.log_level` as the global filter and accepting per-target overrides through `RUST_LOG`.

#### Scenario: JSON format selected
- **WHEN** `telemetry.log_format = "json"`
- **THEN** every log line is a single JSON object containing at minimum `timestamp`, `level`, `target`, `message`, and any `span` fields

#### Scenario: RUST_LOG overrides level
- **WHEN** `RUST_LOG=molesignal_infra=trace` is set and `telemetry.log_level = "info"`
- **THEN** `info` is the default but `molesignal_infra::*` emits at `trace`

### Requirement: OTLP Trace Export

When `telemetry.otlp_endpoint` is non-empty, the server SHALL configure an OpenTelemetry OTLP exporter that ships spans from the `tracing` subscriber, including request spans created by `tower_http::trace::TraceLayer`.

#### Scenario: Endpoint configured
- **WHEN** `otlp_endpoint = "http://collector:4317"`
- **THEN** trace spans for every incoming HTTP request are exported to that endpoint with service.name = `molesignal` and `service.role = <node.role>`

#### Scenario: Endpoint empty
- **WHEN** `otlp_endpoint = ""`
- **THEN** no OTLP exporter is installed and no network calls are made for tracing

### Requirement: Prometheus Metrics Endpoint

The HTTP server SHALL expose `GET /metrics` (no auth required) returning text-format Prometheus metrics including HTTP request counts/latency, ingest batch counts/bytes, query latency, alert rule eval count, and `Delivery` send count by status.

#### Scenario: Metrics endpoint reachable
- **WHEN** `metrics_enabled = true` and a GET is issued to `/metrics`
- **THEN** the response is `200 OK` with `Content-Type: text/plain; version=0.0.4` and contains at least one `molesignal_http_requests_total` line

#### Scenario: Metrics disabled
- **WHEN** `metrics_enabled = false`
- **THEN** `/metrics` returns `404 Not Found`

### Requirement: Request ID Propagation

Every HTTP request SHALL be tagged with an `X-Request-Id` (generated when absent) that is added to the tracing span and echoed back in the response headers.

#### Scenario: Caller-provided id is preserved
- **WHEN** a request arrives with `X-Request-Id: abc-123`
- **THEN** all log lines for that request include `request_id = "abc-123"` and the response carries the same header value
