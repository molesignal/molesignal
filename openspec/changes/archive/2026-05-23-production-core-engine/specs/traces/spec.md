## ADDED Requirements

### Requirement: Service Graph Aggregation

When traces are ingested, the system SHALL maintain an in-memory rolling aggregator that, for each adjacent span pair `(client_service, server_service)` derived from `span.parent_id` linkage, counts `request_count`, `error_count` (span status != OK), and tracks `duration_ms` p50/p95/p99 over a 1-minute window. Each minute boundary the aggregator SHALL flush to the `service_graph_edges { org_id, client_service, server_service, time_bucket_min, request_count, error_count, duration_p50_ms, duration_p95_ms, duration_p99_ms }` table via upsert.

#### Scenario: Two-service trace produces one edge
- **WHEN** a trace with `svcA → svcB` span hierarchy is ingested
- **THEN** within the next minute boundary an edge row `(svcA, svcB)` exists with `request_count >= 1`

#### Scenario: Span error counted
- **WHEN** the server-side span has `status = ERROR`
- **THEN** the corresponding edge row's `error_count >= 1`

### Requirement: Service Graph Query Endpoint

`GET /api/v1/traces/service_graph?from=&to=&service=` SHALL return nodes (services) and edges (call relations) aggregated over the requested window, capped at `service_graph.max_nodes` (default 200) and `max_edges` (default 1000); over-capacity responses include `truncated: true`.

#### Scenario: Window query returns graph
- **WHEN** a Viewer GETs `?from=now-1h&to=now`
- **THEN** the response is `200 OK` with `{ nodes: [{ name, request_count, error_rate }, ...], edges: [{ client, server, request_count, error_rate, latency_p95_ms }, ...], truncated: false }`

#### Scenario: Service filter narrows
- **WHEN** `?service=svcA` is passed
- **THEN** only nodes within 1 hop of `svcA` and their connecting edges are returned
