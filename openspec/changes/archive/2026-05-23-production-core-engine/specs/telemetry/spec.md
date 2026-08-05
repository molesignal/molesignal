## MODIFIED Requirements

### Requirement: OTLP Trace Export

When `telemetry.otlp_endpoint` is non-empty, `molesignal_shared::telemetry::init_full` SHALL configure an `opentelemetry-otlp` exporter (gRPC transport over `tonic`) that ships spans from the `tracing` subscriber via `tracing-opentelemetry::OpenTelemetryLayer`, with resource attributes `service.name = "molesignal"`, `service.role = <node.role>`, and `service.instance.id = <node_id>`. The exporter SHALL use `Resource::from_detectors` and run on a dedicated background tokio runtime to avoid blocking application spans.

#### Scenario: Endpoint configured
- **WHEN** `otlp_endpoint = "http://collector:4317"`
- **THEN** trace spans for every incoming HTTP request are exported to that endpoint with the documented resource attributes; the exporter's queue is non-blocking (spans dropped under back-pressure increment `otlp_exporter_dropped_spans_total`)

#### Scenario: Endpoint empty
- **WHEN** `otlp_endpoint = ""`
- **THEN** no OTLP exporter is installed and no network calls are made for tracing; only the local subscriber (JSON or text) runs

#### Scenario: Invalid endpoint fails fast
- **WHEN** `otlp_endpoint` is malformed (e.g., `"not-a-url"`)
- **THEN** `init_full` returns an error and `main()` exits before any role starts

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
