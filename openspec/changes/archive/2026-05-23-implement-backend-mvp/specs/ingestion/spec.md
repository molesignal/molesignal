## ADDED Requirements

### Requirement: HTTP Ingest Endpoints

The system SHALL expose `POST /api/v1/ingest/logs/:stream`, `POST /api/v1/ingest/metrics/:stream`, `POST /api/v1/ingest/traces/:stream` that accept newline-delimited JSON or a JSON array, route the events to `IngestService::ingest`, and return per-event acceptance counts.

#### Scenario: Logs batch is accepted into existing stream
- **WHEN** a client POSTs 100 JSON log lines to `/api/v1/ingest/logs/app` with a valid `X-Org-Id` header for an organization that owns stream `app`
- **THEN** the response is `200 OK` with body `{"accepted": 100, "rejected": 0}` and all 100 events are appended to the WAL for `(org, app, logs)` before the handler returns

#### Scenario: Unknown stream auto-creates with inferred schema
- **WHEN** the target stream does not yet exist for the requesting org
- **THEN** the system creates a new `StreamDefinition` with schema inferred from the first batch and accepts the events

#### Scenario: Malformed event in batch
- **WHEN** one event in a batch fails JSON parsing or schema validation
- **THEN** the response is `200 OK` with `{"accepted": N-1, "rejected": 1, "errors": [{"index": <i>, "reason": "..."}]}` and the remaining events are written

#### Scenario: Caller lacks StreamWrite permission
- **WHEN** the caller's role does not allow `Permission::StreamWrite`
- **THEN** the response is `403 Forbidden` and nothing is written

### Requirement: gRPC Internal Ingest

The system SHALL implement a `proto.ingest.IngestService` tonic server in the ingester role so router/edge nodes can forward batches without going through HTTP.

#### Scenario: Router forwards a batch via gRPC
- **WHEN** the router publishes an `IngestBatch` via the generated gRPC client to an ingester
- **THEN** the ingester invokes the same `IngestService::ingest` use case and returns an `IngestResult` proto message

### Requirement: Write-Ahead Log Durability

The system SHALL append every accepted `IngestBatch` to a segment-based WAL on local disk under `wal.dir`, grouped by `(org, stream, stream_type)`, with fsync throttled to `wal.sync_interval_ms`, before acknowledging the write to the caller.

#### Scenario: Crash recovery replays unflushed segments
- **WHEN** the ingester restarts and finds WAL segments whose corresponding parquet files were not persisted (no matching `ParquetFileMeta` row)
- **THEN** the ingester replays those segments into the in-memory buffer at startup before opening any ingest port

#### Scenario: Segment rolls over at configured size
- **WHEN** the current segment reaches `wal.segment_size_mb` MiB
- **THEN** the writer closes it, fsyncs, and opens a new segment

### Requirement: In-Memory Buffer and Periodic Flush

The ingester SHALL keep accepted events in a per-stream in-memory Arrow buffer, flushing to a parquet file in the object store when either `ingester.buffer_max_mb` is exceeded or `ingester.flush_interval_secs` elapses, whichever happens first.

#### Scenario: Buffer flush produces parquet + ParquetFileMeta
- **WHEN** a buffer flush triggers
- **THEN** the system writes a parquet file to object key `{org}/{stream}/{YYYY-MM-DD}/{ksuid}.parquet`, inserts a `ParquetFileMeta` row (time range, min/max values, row count, size), and only then truncates the WAL segments that contributed to the flushed batch

#### Scenario: Flush failure retains data
- **WHEN** the object store `put` call fails
- **THEN** the buffer and WAL are left intact, the failure is logged with the stream name, and the next tick retries

### Requirement: Schema Evolution

The system SHALL accept a wider set of fields than the registered schema and persist newly seen scalar fields via `StreamRepository::update_schema`, marking them `nullable = true`.

#### Scenario: New field appears in batch
- **WHEN** an event includes a field not in the current `StreamDefinition.schema`
- **THEN** the system adds the field to the schema (nullable, `indexed = false`) and accepts the event

#### Scenario: Type conflict on existing field
- **WHEN** an event provides a value whose type does not match the registered `FieldType`
- **THEN** the event is rejected with reason `type mismatch on field <name>: expected <T>, got <U>`
