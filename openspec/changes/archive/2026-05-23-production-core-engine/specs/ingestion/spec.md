## ADDED Requirements

### Requirement: Ingester Startup Replay Before Port Open

The `ingester` role SHALL, before binding any HTTP or gRPC port, scan `wal.dir` for segments whose corresponding `ParquetFileMeta` row is absent, replay every well-formed record into the in-memory Arrow buffer via the same `IngestService::ingest` code path, and only then transition its readiness probe to "ready" so the router excludes the node from rotation until replay completes.

#### Scenario: Replay completes before traffic flows
- **WHEN** the process starts with `wal-000003.seg` containing 1,200 records not yet reflected in `parquet_file_meta`
- **THEN** those 1,200 records are pushed back into the buffer (which may immediately trigger a flush), and `GET /api/v1/healthz` returns `200 OK` only after replay finishes; while replay runs the endpoint returns `503 Service Unavailable`

#### Scenario: Corrupted tail is truncated, replay continues
- **WHEN** the last record of `wal-000003.seg` fails CRC32C verification
- **THEN** the segment is truncated at the prior intact boundary, the preceding records are replayed, and a `wal_recovery_truncated_total` counter increments

### Requirement: Stream HTTP CRUD

The system SHALL expose `GET/POST /api/v1/streams`, `GET/PUT/DELETE /api/v1/streams/:id`, `PUT /api/v1/streams/:id/schema`, and `PUT /api/v1/streams/:id/retention`, backed by `StreamRepository`. A `Stream` carries `{ id, org_id, name, stream_type: "logs" | "metrics" | "traces", schema: Vec<FieldDef>, retention: { days, hot_days? }, indexed_fields: Vec<String>, created_at, updated_at }`. List supports `?page=&page_size=&filter=&stream_type=`; default `page_size = 50`, cap `200`.

#### Scenario: Create stream with explicit schema
- **WHEN** an Editor POSTs `{ "name": "audit", "stream_type": "logs", "schema": [{name:"actor",type:"Utf8",indexed:true,nullable:false}, ...], "retention": { "days": 30 } }`
- **THEN** the response is `201 Created` with the persisted row and subsequent ingests to `/api/v1/ingest/logs/audit` validate against the explicit schema

#### Scenario: Schema PUT widens but never narrows
- **WHEN** a PUT to `/streams/:id/schema` removes a previously-defined column
- **THEN** the response is `400 Bad Request` with `{ "error": "column removal not allowed; only additions or nullability widening" }`

#### Scenario: Retention PUT triggers compactor sweep
- **WHEN** a PUT to `/streams/:id/retention` shortens `days` from 30 to 7
- **THEN** the row updates, and the next compactor retention tick observes the new value and starts marking files older than 7 days as `deleted`

#### Scenario: Cross-org stream lookup returns 404
- **WHEN** a member of `orgA` requests `/streams/:id` for a stream owned by `orgB`
- **THEN** the response is `404 Not Found`

### Requirement: Stream Type Includes Enrichment

`StreamDefinition.stream_type` SHALL accept the value `enrichment` in addition to `logs / metrics / traces / llm_traces / rum_*`. Enrichment streams have a fixed schema `(key TEXT NOT NULL, value JSONB)` and SHALL be queryable by SQL but NOT written through the standard pipeline path (a pipeline targeting an enrichment stream is rejected at create time with `400 Bad Request`).

#### Scenario: Enrichment stream created
- **WHEN** a POST creates a stream with `stream_type = "enrichment"`
- **THEN** the response is `201 Created`; ingest via `/api/v1/ingest/enrichment/<name>` writes key/value rows

#### Scenario: Pipeline targeting enrichment rejected
- **WHEN** a POST creates a pipeline with `stream_targets[0].stream_type = "enrichment"`
- **THEN** the response is `400 Bad Request` with `{ "error": "pipelines cannot target enrichment streams" }`

### Requirement: Ingester Per-Stream Buffer Pool

The ingester SHALL maintain one Arrow `RecordBatchBuilder` per active `(org, stream_type, stream)` triple, share a single tokio mutex per buffer, and never block one stream's writes on another stream's flush.

#### Scenario: Independent flush ordering
- **WHEN** stream `A` triggers a flush while stream `B` is mid-append
- **THEN** stream `B`'s append proceeds without waiting for `A`'s parquet upload to complete

#### Scenario: Schema extension synced to live buffer
- **WHEN** `StreamRepository::update_schema` adds a new column to stream `app`
- **THEN** the in-memory buffer's builder rolls forward to include the new column with null values for all previously buffered rows so the next flush emits a Parquet file that already has the new column

## MODIFIED Requirements

### Requirement: In-Memory Buffer and Periodic Flush

The ingester SHALL keep accepted events in a per-stream in-memory Arrow buffer, flushing to a parquet file in the object store when either `ingester.buffer_max_mb` is exceeded or `ingester.flush_interval_secs` elapses, whichever happens first. Flushes SHALL be atomic: parquet upload → Tantivy archive upload (when any field is `indexed=true`) → `ParquetFileMetaRepository::insert` → WAL truncation up to the buffer's high-watermark sequence index, in that order; any failure aborts subsequent steps and the buffer is left intact for retry on the next tick.

#### Scenario: Buffer flush produces parquet + ParquetFileMeta + Tantivy archive
- **WHEN** a buffer flush triggers and at least one field has `indexed = true`
- **THEN** the system uploads the parquet to `{org}/{stream}/{YYYY-MM-DD}/{ksuid}.parquet`, uploads the Tantivy archive to `{object_key}.tantivy.tar.zst`, inserts the `ParquetFileMeta` row (time range, min/max values, row count, size, deleted=false), and only then truncates the WAL segments contributing to the flushed batch

#### Scenario: Flush failure retains data
- **WHEN** the object store `put` call fails on either parquet or Tantivy archive
- **THEN** the buffer and WAL are left intact, the failure is logged with the stream name and which step failed, an `ingester_flush_errors_total{step="…"}` counter increments, and the next tick retries the entire flush from the start

#### Scenario: ParquetFileMeta insert failure deletes orphan objects
- **WHEN** parquet and Tantivy archive both upload successfully but `ParquetFileMetaRepository::insert` fails
- **THEN** both objects are deleted from the object store, the buffer is left intact, and the original DB error is bubbled to the next tick
