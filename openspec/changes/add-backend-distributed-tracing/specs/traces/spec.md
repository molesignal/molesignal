## MODIFIED Requirements

### Requirement: Service Graph Aggregation

When traces are ingested, the system SHALL maintain a rolling aggregator that, for each adjacent span pair derived from `(trace_id, parent_span_id)` linkage, counts `request_count`, `error_count`, and tracks duration p50/p95/p99 over a 1-minute window. Service identity SHALL use the effective role-aware service name, so `router`, `ingester`, `querier`, `compactor`, and `alert_manager` appear as distinct nodes even when multiple roles share a process. Same-trace affinity SHALL allow parent/child pairing across producer nodes before graph aggregation. Each minute boundary the aggregator SHALL flush to `service_graph_edges { org_id, client_service, server_service, time_bucket_min, request_count, error_count, duration_p50_ms, duration_p95_ms, duration_p99_ms }` via idempotent upsert.

#### Scenario: Two-service trace produces one edge
- **WHEN** a trace with effective service hierarchy `molesignal-router → molesignal-querier` is ingested
- **THEN** within the next minute boundary an edge row `(molesignal-router, molesignal-querier)` exists with `request_count >= 1`

#### Scenario: Span error counted
- **WHEN** the server-side span has OpenTelemetry status ERROR
- **THEN** the corresponding edge row's `error_count >= 1`

#### Scenario: Multi-role process remains split
- **WHEN** one process executes both router and querier spans
- **THEN** the graph derives two role-specific nodes from per-Span execution role
- **AND** it does not create a combined `router+querier` node

#### Scenario: Span IDs from different traces do not collide
- **WHEN** two traces contain the same parent span ID value
- **THEN** graph pairing remains isolated by trace ID
- **AND** no edge is created across traces

## ADDED Requirements

### Requirement: Complete Canonical Trace Contract

All Trace sources SHALL normalize into one CanonicalSpan contract containing trace/span/parent identifiers, trace flags/state, name, kind, start/end/duration, status, Resource, Instrumentation Scope, attributes, Events, Links, dropped attributes/events/links counts, semantic-convention/schema version, sampling reason, and partial/truncation metadata. Public OTLP ingest, internal self tracing, storage, Trace query responses, and external OTLP export SHALL use this common contract.

#### Scenario: Links and Events survive round trip
- **WHEN** an OTLP Span with two Links, one explicit Event, a named instrumentation scope, and nonzero dropped counts is ingested
- **THEN** querying the Trace returns equivalent Links, Event, scope, and dropped counts
- **AND** re-exporting that Span preserves the same semantic data

#### Scenario: Self and public OTLP fields match
- **WHEN** semantically equivalent spans arrive from the internal tracing layer and public OTLP receiver
- **THEN** both produce the same canonical field names, status representation, and nested Event/Link shapes

### Requirement: Trace Span Deduplication

Trace ingestion SHALL deduplicate spans by `(org_id, trace_id, span_id)` within a configurable window. An identical retry SHALL not create a second stored Span. The first complete canonical record SHALL win; later conflicting content SHALL be rejected or quarantined and SHALL increment conflict metrics.

#### Scenario: OTLP retry is idempotent
- **WHEN** an exporter submits the same completed Span twice
- **THEN** only one canonical Span is stored
- **AND** the duplicate counter increments

#### Scenario: Conflicting duplicate is visible
- **WHEN** a second Span has the same identity but different parent, timing, or service identity
- **THEN** the stored complete Span is not silently overwritten
- **AND** a conflict metric and diagnostic record identify the conflict without sensitive payload

### Requirement: Trace Completeness Metadata

The Trace query contract SHALL identify partial traces and reasons including span limit, attribute/event/link truncation, sampler overflow, late span, owner failure, and sink drop. Query clients SHALL be able to distinguish a complete Trace from a best-effort partial Trace.

#### Scenario: Span limit marks partial
- **WHEN** a Trace exceeds its configured Span limit
- **THEN** retained spans and aggregate summary remain queryable
- **AND** the Trace response reports `partial = true` with reason `span_limit`

### Requirement: Development-Stage Trace Schema Replacement

The new CanonicalSpan storage schema SHALL replace the current development-stage Trace row contract. The implementation SHALL NOT provide historical row conversion, compatibility aliases, or old-schema query fallback.

#### Scenario: Fresh system stream uses only the new schema
- **WHEN** `_sys/traces/_molesignal` is created after this change
- **THEN** it is initialized/evolved according to the CanonicalSpan contract
- **AND** no legacy Trace columns or translation path are required

