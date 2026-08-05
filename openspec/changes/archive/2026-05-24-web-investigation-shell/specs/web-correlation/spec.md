## ADDED Requirements

### Requirement: Link Provider Registry

The web app SHALL ship a static registry of `LinkProvider { from_kind, to_kind, label, derive(ctx) -> CorrelationContext }` covering at minimum these eight pairs: metric→trace, metric→log, trace→log, trace→host, log→trace, log→host, host→metric, service→trace. Right-click or selecting "View related" on a contextual field SHALL show only providers whose `from_kind` matches the field's kind.

#### Scenario: Metric tooltip shows two links
- **WHEN** the user hovers a span in a TimeSeriesPlot (metric kind) at a particular timestamp
- **THEN** the tooltip's "View related" submenu lists at least `View traces` and `View logs` (matching the metric→trace and metric→log providers); no `View host` entry appears

#### Scenario: Unknown kind exposes nothing
- **WHEN** a field with `kind = 'session'` (not registered) is clicked
- **THEN** the link submenu is empty and the field click is a no-op

### Requirement: Context Derivation

When a link is followed, the provider's `derive(ctx)` SHALL produce a `CorrelationContext { time_range, filters, prefill }` with: (a) the time range taken from the source's local cursor ± a kind-specific halo (`±30s` for span, `±5s` for log row, `±60s` for metric sample) intersected with the global time window; (b) filters built from the source's mapped fields (e.g., `trace_id`, `service.name`, `host`); (c) optional `prefill.statement` for SQL/PromQL frames.

#### Scenario: Trace to log halo
- **WHEN** the user clicks a span starting at 09:42:31 and lasting 380ms in a trace frame
- **THEN** the new log frame's `time_range_override` is `[09:42:01, 09:43:01.380]` and filters include `trace_id = <span.trace_id>` and `service.name = <span.service>`

#### Scenario: Metric to trace prefill SQL
- **WHEN** the user clicks "View traces" on a spike in the `http_requests_total` metric for `service=api`
- **THEN** the new trace frame contains a prefilled SQL `SELECT trace_id, duration_ms FROM traces WHERE service.name = 'api' AND _timestamp BETWEEN ... ORDER BY duration_ms DESC LIMIT 100`

### Requirement: Server-side Correlation Endpoint

The web app SHALL be able to delegate correlation derivation to `GET /api/v1/web/correlation/:from_kind/:to_kind?ctx=<base64-json>`; the server returns `{ time_range, filters, prefill }` matching the client-side contract. The client SHALL prefer server-side derivation when the source field includes server-known identifiers (e.g., `trace_id`) so that backend-discovered links (e.g., trace→incident) can supplement static client providers.

#### Scenario: Server returns extra filter
- **WHEN** the client calls `GET /api/v1/web/correlation/trace/log?ctx=<base64({trace_id:'t1'})>`
- **THEN** the response includes server-derived `filters: [{field: 'trace_id', op: '=', value: 't1'}, {field: 'service.name', op: 'IN', value: ['api','db']}]` reflecting services touched by that trace

#### Scenario: Server timeout falls back to client provider
- **WHEN** the server endpoint does not respond within 400ms
- **THEN** the client uses the local provider's `derive(ctx)` result; a metric `correlation_server_timeout_total` is incremented in browser-side telemetry

### Requirement: Anchor And Filter Inheritance

A pushed frame SHALL inherit the parent frame's filters by default, augmented by the link provider's added filters; the new frame's UI SHALL render an inherited-filter chip strip with each parent filter shown as a removable chip. Removing a chip SHALL re-issue the frame's underlying query.

#### Scenario: Inherited chips visible
- **WHEN** a log frame is pushed from a trace frame carrying `service.name = api, host = ip-10-0-1-2`
- **THEN** the log frame header shows two chips `service.name=api` and `host=ip-10-0-1-2`, plus the new `trace_id=t1` chip; clicking the `×` on `host` removes that filter and refetches
