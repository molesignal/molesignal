# Web Misc Pages Capability

## Purpose

This capability covers standalone web routes that round out feature parity outside the main investigation flow: logs inspector (`/logs/inspector`), trace detail (`/traces/:id`), trace session detail (`/traces/session/:id`), stream explore (`/streams/:id`), service graph (`/service-graph`), dashboard import (`/dashboards/import`), dashboard new panel (`/dashboards/:id/panels/new`), alerts history (`/alerts/history`), alerts insights (`/alerts/insights`), short URL resolver (`/short/:code`), and the ingestion vendor pages (`/ingest/:category/:source`).

## Requirements

### Requirement: Logs Inspector Route

The web app SHALL expose `/logs/inspector` as a search-job inspector view. The page SHALL accept a `?id=<search_job_id>` query param, call `GET /api/v1/search_jobs/{id}` via the existing search-jobs client, and render the job's status / scanned-rows / SQL statement in a KvRow block. Without an `id` param, the page renders a "Pick a search job" empty state with a link back to `/logs`.

#### Scenario: Page renders for a known job id

- **WHEN** the user opens `/logs/inspector?id=abc` and the search-jobs endpoint returns a job
- **THEN** the page displays the job's `state`, `submitted_at_micros`, `scanned_rows`, and `request_json` summary

#### Scenario: Missing id renders empty state

- **WHEN** the user opens `/logs/inspector` without an `id` query param
- **THEN** the page renders an empty state with `Pick a search job` and a link to `/logs`

### Requirement: Inline PromQL Builder

The web app SHALL expose a Builder mode inside `/metrics` on top of the existing query endpoint. The builder SHALL let the user pick a metric name, add label matchers, and pick an aggregation function before pressing `Run`; pressing `Run` issues a `POST /api/v1/query` and renders the result via the existing timeseries primitive. The app SHALL NOT expose a separate `/metrics/promql-builder` route.

#### Scenario: Run executes the composed query

- **WHEN** the user opens Builder mode on `/metrics`, picks metric `http_requests_total`, label `status="200"`, function `rate()`, range `5m`, and clicks Run
- **THEN** the page POSTs `rate(http_requests_total{status="200"}[5m])` to the metrics endpoint
- **AND** renders the returned series in the timeseries chart

### Requirement: Trace Detail Route

The web app SHALL expose `/traces/:id` as a trace detail view backed by the existing `/api/v1/web/trace` endpoint. The page SHALL render the trace's span tree (sorted by start time), KvRow summary of trace duration / service count / span count, and `Search around` link to `/logs` filtered to the trace id.

#### Scenario: Trace renders with spans

- **WHEN** the user opens `/traces/abc123` and the endpoint returns spans
- **THEN** the page renders the span tree with each span's service / operation / duration
- **AND** the header KvRow shows total duration and span count

#### Scenario: Trace missing renders not-found state

- **WHEN** the user opens `/traces/missing` and the endpoint returns 404
- **THEN** the page renders an error state with `Trace not found` and a back link

### Requirement: Trace Session Detail Route

The web app SHALL expose `/traces/session/:id` as a session detail view backed by the trace endpoint scoped by session id. The page lists all traces in the session sorted by start time.

#### Scenario: Session lists its traces

- **WHEN** the user opens `/traces/session/sess-1` and the endpoint returns 5 traces
- **THEN** the page renders a DataTable of 5 rows linking each to `/traces/:id`

### Requirement: Stream Explore Route

The web app SHALL expose `/streams/:id` as a per-stream explore view backed by the existing `/api/v1/web/streams/:id` endpoint. The page SHALL show stream metadata (schema, retention, partition keys) in a KvRow block and a `Query in Logs` button that opens `/logs` pre-filtered to the stream.

#### Scenario: Stream view renders schema and quick links

- **WHEN** the user opens `/streams/app-logs`
- **THEN** the page renders the stream's schema columns and retention policy
- **AND** the `Query in Logs` button targets `/logs?stream=app-logs`

### Requirement: Service Graph Route

The web app SHALL expose `/service-graph` as a topology view backed by the existing `/api/v1/web/topology` endpoint. The page renders nodes / edges via the existing topology primitive; click on a service node navigates to `/streams/<service>` or `/traces/?service=<service>` based on the user's choice in a small popover.

#### Scenario: Topology renders nodes and edges

- **WHEN** the user opens `/service-graph` and the endpoint returns a non-empty topology
- **THEN** the page renders all nodes + edges
- **AND** clicking a node opens a popover with `Explore traces` / `Explore stream` actions

### Requirement: Dashboard Import Route

The web app SHALL expose `/dashboards/import` that accepts a pasted / uploaded JSON or YAML dashboard payload, parses it client-side, and POSTs the resulting body to `POST /api/v1/dashboards`. On success the page navigates to the new dashboard.

#### Scenario: Valid JSON imports successfully

- **WHEN** the user pastes a valid dashboard JSON and clicks Import
- **THEN** the page POSTs the body and `nav("/dashboards/" + newId)` on success

#### Scenario: Invalid input shows inline error

- **WHEN** the user pastes malformed JSON and clicks Import
- **THEN** the page shows an inline error citing the parse failure and does NOT POST

### Requirement: Dashboard New-Panel Route

The web app SHALL expose `/dashboards/:id/panels/new` as a dedicated panel-creation route that opens the panel editor inside the dashboard layout. On save the route navigates back to `/dashboards/:id`.

#### Scenario: Save returns to dashboard

- **WHEN** the user fills the panel form and clicks Save on `/dashboards/d1/panels/new`
- **THEN** the new panel is PATCHed onto dashboard `d1` and the page navigates to `/dashboards/d1`

### Requirement: Alerts History Route

The web app SHALL expose `/alerts/history` showing the recent N alert deliveries from `GET /api/v1/alerts/deliveries` (existing endpoint). Each row shows rule name / channel / status / sent-at; the page paginates by `before_micros`.

#### Scenario: History lists recent deliveries

- **WHEN** the user opens `/alerts/history` and the endpoint returns 20 deliveries
- **THEN** the page renders a DataTable of 20 rows sorted by `sent_at_micros` DESC

### Requirement: Alerts Insights Route

The web app SHALL expose `/alerts/insights` showing aggregate insights from the existing `/api/v1/alerts/insights` endpoint (alert counts, top firing rules, MTTR).

#### Scenario: Insights renders KPI strip

- **WHEN** the user opens `/alerts/insights`
- **THEN** the page renders a KPI strip with total fires / top rule / MTTR pulled from the endpoint

### Requirement: Short URL Resolver Route

The web app SHALL expose `/short/:code` that resolves the short code via `GET /api/v1/short_url/:code` and immediately calls `navigate(longUrl, { replace: true })` on success. The page renders no layout chrome; on failure it shows a one-line error with a link back to `/`.

#### Scenario: Known code redirects

- **WHEN** the user opens `/short/abc` and the endpoint returns `{long_url: "/dashboards/d1"}`
- **THEN** the router replaces the URL with `/dashboards/d1`

#### Scenario: Unknown code shows error

- **WHEN** the user opens `/short/missing` and the endpoint returns 404
- **THEN** the page renders `Short URL not found` with a link to `/`

### Requirement: Ingestion Real Pages

The web app SHALL replace the docs-only placeholders at `/ingest/:category/:source` with real per-vendor pages. Each page SHALL render: (a) the auto-detected ingest endpoint URL for the user's org, (b) a copy-pasteable snippet for the vendor, (c) a `Test event` button that POSTs a small fixture to the existing `/api/v1/ingest/_health` endpoint and shows the result.

#### Scenario: Vendor page shows endpoint + snippet

- **WHEN** the user opens `/ingest/logs/fluentbit`
- **THEN** the page renders the org's `/api/v1/ingest/logs` URL and a Fluent Bit config snippet pointing at it

#### Scenario: Test event surfaces backend health

- **WHEN** the user clicks `Test event` on any vendor page
- **THEN** the page POSTs to `/api/v1/ingest/_health` and renders the status code + latency
