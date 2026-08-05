# Telemetry Capability

## Purpose

结构化日志、统一 Trace pipeline 的自回灌与 OTLP 外发、`/metrics` Prometheus 端点（含 caching / object_store 指标族）、请求 ID 注入。审计事件落库见 `audit` capability。

## Requirements

### Requirement: Structured Logging

`molesignal_shared::telemetry::init` SHALL initialize `tracing_subscriber` with either JSON or text formatting based on `telemetry.log_format`, respecting `telemetry.log_level` as the global filter and accepting per-target overrides through `RUST_LOG`.

#### Scenario: JSON format selected
- **WHEN** `telemetry.log_format = "json"`
- **THEN** every log line is a single JSON object containing at minimum `timestamp`, `level`, `target`, `message`, and any `span` fields

#### Scenario: RUST_LOG overrides level
- **WHEN** `RUST_LOG=molesignal_infra=trace` is set and `telemetry.log_level = "info"`
- **THEN** `info` is the default but `molesignal_infra::*` emits at `trace`

### Requirement: Unified OTLP Trace Export

Retained traces SHALL be exported only after the unified tail-sampling pipeline. External export SHALL be configured exclusively through `telemetry.trace.external`; the logging subscriber SHALL NOT install a second, unsampled OTLP exporter.

#### Scenario: External endpoint configured
- **WHEN** `telemetry.trace.external.endpoint = "http://collector:4317"` and `protocol = "grpc"`
- **THEN** traces retained by the tail sampler are exported through the bounded external sink using the configured timeout, queue, batch, compression, headers, and TLS settings

#### Scenario: External endpoint empty
- **WHEN** `telemetry.trace.external.endpoint = ""`
- **THEN** no external Trace sink is installed and the Trace pipeline continues serving configured self-ingest only

#### Scenario: Invalid external endpoint fails fast
- **WHEN** `telemetry.trace.external.endpoint` is malformed or its protocol is not `grpc` or `http/protobuf`
- **THEN** configuration validation fails before any role starts

### Requirement: Prometheus Metrics Endpoint

The HTTP server SHALL expose `GET /metrics` (no auth required, no rate limit) returning Prometheus exposition (text format, version 0.0.4) backed by a single global `prometheus::Registry` populated at wire time. Built-in metric families MUST include HTTP request count/latency (histogram), ingest batch count/bytes, query latency, alert rule eval count, `Delivery` send count by status, all `caching::*` counters/gauges, and node-level CPU/memory if available from the runtime.

#### Scenario: Metrics endpoint reachable
- **WHEN** `metrics_enabled = true` and a GET is issued to `/metrics`
- **THEN** the response is `200 OK` with `Content-Type: text/plain; version=0.0.4` and contains at least one `molesignal_http_requests_total` line and at least one `cache_parquet_file_meta_hits_total` line

#### Scenario: Metrics disabled
- **WHEN** `metrics_enabled = false`
- **THEN** `/metrics` returns `404 Not Found` and the route is not registered in the axum router

#### Scenario: Metrics endpoint accessible without auth
- **WHEN** a request hits `/metrics` with no `Authorization` header
- **THEN** the response is still `200 OK`; the auth middleware whitelists `/metrics` alongside `/api/v1/auth/login` and `/api/v1/healthz`

### Requirement: Request ID Propagation

Every HTTP request SHALL be tagged with an `X-Request-Id` (generated when absent) that is added to the tracing span and echoed back in the response headers.

#### Scenario: Caller-provided id is preserved
- **WHEN** a request arrives with `X-Request-Id: abc-123`
- **THEN** all log lines for that request include `request_id = "abc-123"` and the response carries the same header value

### Requirement: Integration Test Coverage For Production-Core-Engine

The bootstrap test suite SHALL contain `MS_RUN_IT=1` gated end-to-end tests for the 4 capabilities listed in production-core-engine follow-up:

- `it_service_graph.rs`: trace ingest → graph aggregation → HTTP query
- `it_anomaly_mad.rs`: baseline ingest → MAD detector → outlier list
- `it_copilot_fanout.rs`: copilot routes cfg + license gate
- `it_rum_ingest.rs`: RUM session/action/error/replay ingest end-to-end

Each test SHALL exercise at least one happy path (writes succeed + queries return expected) and one sad path (e.g. missing org / bad input / license disabled).

#### Scenario: Service graph aggregation visible via HTTP

- **WHEN** ingest 100 trace spans with client_service=web, server_service=api over 2 minutes
- **AND** dispatcher_tick flushes service_graph_aggregator
- **THEN** GET `/api/v1/traces/service_graph?from=...&to=...` returns an edge `{client: "web", server: "api", request_count: 100, …}`

#### Scenario: MAD detector identifies outlier

- **WHEN** seed 100 baseline values around 50 + 5 outliers at 500 into the source stream
- **AND** detector run with k=3
- **THEN** the 5 outliers are reported with `is_outlier=true`, baseline values not flagged

### Requirement: Integration Test Coverage For Feature-Parity Capabilities

The bootstrap test suite SHALL contain `MS_RUN_IT=1` gated end-to-end tests for the 5 capabilities listed in feature-parity follow-ups:

- `it_short_url.rs`: create → redirect → click_count → expiry → 410
- `it_annotations.rs`: CRUD + tag filter + cross-org isolation
- `it_sourcemaps.rs`: upload multipart → object_store → translate_frame
- `it_log_patterns.rs`: CRUD + compile_check + first_match
- `it_search_jobs.rs`: Prefer: respond-async → 202 → worker → done → results

Plus extensions for `it_scheduled_pipelines.rs`, `it_connectors.rs`, `it_search_around.rs`, `it_cipher_keys.rs`, `it_license_gates.rs` covering deeper scenarios spec'd in the parent change.

#### Scenario: Short URL expiry returns 410

- **WHEN** POST `/api/v1/short` with `expires_at = now - 1s`
- **AND** GET `/s/<code>` is called
- **THEN** response is 410 Gone

#### Scenario: Async search job completes and serves results

- **WHEN** POST `/api/v1/query` with `Prefer: respond-async`
- **AND** the search_jobs worker picks up the row
- **THEN** within 5s, GET `/api/v1/query/jobs/{id}` returns `state: "done"`
- **AND** GET `/api/v1/query/jobs/{id}/results` returns NDJSON rows

### Requirement: Integration Test For Scheduled Reports Delivery

`it_scheduled_reports.rs` SHALL spin up a wiremock HTTP server, create a scheduled report with format=json + recipient `{kind: "webhook", target: wiremock_url}`, force a tick, and assert:
- wiremock received the POST with `Content-Type: application/json`
- `report_deliveries` table has one row with `status: sent`

#### Scenario: Webhook delivery records sent status

- **WHEN** scheduled report fires + wiremock responds 200
- **THEN** `report_deliveries` has exactly one row with `status=sent`, `recipient_kind=webhook`, `recipient_target=<wiremock_url>`

#### Scenario: Webhook 500 records failed status

- **WHEN** wiremock returns 500
- **THEN** `report_deliveries` row has `status=failed`, `error` non-NULL
