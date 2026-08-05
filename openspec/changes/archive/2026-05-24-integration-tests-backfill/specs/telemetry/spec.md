## ADDED Requirements

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
