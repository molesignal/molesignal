## ADDED Requirements

### Requirement: Bounded Prometheus Active Series

The ingester SHALL track canonical Prometheus series identities as non-reversible fixed-size fingerprints and SHALL enforce configured process-wide, per-organization, and per-metric active-series limits plus a per-organization new-series-per-minute limit. Existing active series MUST remain admissible when only the new-series rate is exhausted. Series not observed for the configured idle TTL SHALL stop consuming active-series capacity.

#### Scenario: Existing series passes after new-series rate is exhausted
- **WHEN** an organization has consumed its new-series allowance and sends samples for an already active series
- **THEN** the samples remain admissible and do not increment the new-series counter

#### Scenario: New series exceeds metric limit
- **WHEN** a request would increase one metric beyond `max_active_series_per_metric`
- **THEN** admission fails with reason `metric_active`
- **AND** raw metric or label values are not included in the error or metrics

#### Scenario: Idle series releases capacity
- **WHEN** a tracked series has not been observed for `idle_ttl_secs`
- **THEN** the next expiry pass removes it from process, organization, and metric counts
- **AND** a new series can consume the released capacity

#### Scenario: Process cap bounds many organizations
- **WHEN** otherwise valid new series across organizations would exceed `max_active_series_per_process`
- **THEN** the excess request is rejected with reason `process_active`

### Requirement: Process-Wide Ingester Memory Admission

The ingester SHALL atomically reserve estimated buffer bytes before WAL append and SHALL reject a live batch with `429 Too Many Requests` when the reservation would exceed `ingester.max_buffer_memory_mb`. Reserved bytes SHALL include active Arrow builders and detached generations until their complete Parquet, metadata, and WAL transaction finishes. A failed flush MUST retain the same reservation for retry without double charging.

#### Scenario: Memory rejection occurs before WAL
- **WHEN** a live batch cannot reserve its estimated bytes within the process limit
- **THEN** the batch returns `429`
- **AND** no WAL record or Arrow row from that batch is created

#### Scenario: Detached slow flush remains charged
- **WHEN** a generation is detached and its object-store upload is still in flight
- **THEN** its accounted bytes remain part of the process reservation
- **AND** concurrent writes are rejected if they would exceed the remaining budget

#### Scenario: Successful flush releases reservation
- **WHEN** Parquet upload, metadata insert, and WAL handling complete for a generation
- **THEN** that generation's accounted bytes are released exactly once

#### Scenario: Replay can exceed the live limit
- **WHEN** durable WAL recovery contains more bytes than the configured live-ingest memory budget
- **THEN** replay force-reserves those bytes and continues toward a forced flush
- **AND** new live batches remain subject to the configured limit

### Requirement: Bounded Resource-Control Observability

The system SHALL expose active-series rejection, active-series count, memory rejection, reserved-memory, compression-ratio, and adaptive-target metrics without organization, metric, stream name, or label identifiers.

#### Scenario: High-cardinality workload does not leak metric labels
- **WHEN** thousands of organizations, metrics, streams, and label sets are admitted or rejected
- **THEN** resource-control metric families contain only fixed reason labels, `stream_type`, or no labels

