# Ingestion Capability

## Purpose

日志/指标/Trace/Extend 的 HTTP + gRPC 写入、Stream HTTP CRUD、schema 校验/演化、ingester 启动 WAL 回放、按流 Arrow buffer、parquet + Tantivy 原子 flush。

`StreamType` 现在有 4 个变体（`Logs / Metrics / Traces / Extend`）。多协议接收器（OTLP / Prometheus remote_write / Loki / ES bulk / Syslog / Firehose）见 `ingest-protocols` capability。
## Requirements
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

The system SHALL append every accepted `IngestBatch` to a segment-based WAL on local disk under `wal.dir`, grouped by `(org, stream, stream_type)`, before acknowledging the write to the caller. The fsync behaviour SHALL be controlled by `wal.flush_strategy` together with `wal.sync_level`, with the following three modes:

- `flush_strategy = "none"`: each record is `BufWriter::flush()`-ed into the kernel page cache; `sync_*` is **never** called regardless of `sync_level`. Crash durability is page-cache-only.
- `flush_strategy = "every_write"`: each record is `BufWriter::flush()`-ed, then `sync_file(file, sync_level)` is invoked immediately.
- `flush_strategy = "batch"` (default): each record is `BufWriter::flush()`-ed; `sync_file(file, sync_level)` is invoked when either `wal.batch_max_pending` records have accumulated or `wal.batch_max_delay_ms` milliseconds have elapsed since the last sync — whichever comes first — and unconditionally on segment rotate or pool drain.

`sync_level` SHALL map to: `"none"` → no-op, `"data"` → `sync_data`, `"all"` → `sync_all` + `sync_dir_parent_of` on the segment's parent directory. The legacy field `wal.sync_interval_ms` SHALL be honoured as an alias of `wal.batch_max_delay_ms` for backward compatibility with TOML files written before this requirement was introduced.

#### Scenario: Crash recovery replays unflushed segments
- **WHEN** the ingester restarts and finds WAL segments whose corresponding parquet files were not persisted (no matching `ParquetFileMeta` row)
- **THEN** the ingester replays those segments into the in-memory buffer at startup before opening any ingest port

#### Scenario: Segment rolls over at configured size
- **WHEN** the current segment reaches `wal.segment_size_mb` MiB
- **THEN** the writer closes it, runs `sync_file` at the configured `sync_level` regardless of `flush_strategy`, and opens a new segment

#### Scenario: Default batch strategy fsyncs within delay budget
- **WHEN** `flush_strategy = "batch"`, `batch_max_delay_ms = 50`, `batch_max_pending = 64`, and a single record is appended while no other writes follow
- **THEN** `sync_data` is invoked on the segment file within 50 milliseconds + scheduler jitter of the append returning

#### Scenario: Batch strategy fsyncs on count threshold before delay
- **WHEN** `flush_strategy = "batch"`, `batch_max_pending = 64`, and 64 records are appended in rapid succession within `batch_max_delay_ms`
- **THEN** `sync_data` is invoked once after the 64th record; the 65th record starts a new batch

#### Scenario: Every-write strategy syncs each record
- **WHEN** `flush_strategy = "every_write"` and `sync_level = "data"`
- **THEN** every `WalPool::append` call returns only after a successful `sync_data` on the affected segment file

#### Scenario: None strategy never syncs
- **WHEN** `flush_strategy = "none"`
- **THEN** no `sync_data` / `sync_all` is ever invoked from the WAL append path; the segment-rotate path still calls `sync_file` to enforce segment durability before unlinking is permitted

#### Scenario: SyncLevel "all" also fsyncs parent directory
- **WHEN** `sync_level = "all"` and a segment is rotated
- **THEN** after `file.sync_all()`, the segment's parent directory is opened and `sync_all`-ed via `sync_dir_parent_of`

#### Scenario: Legacy sync_interval_ms alias is honoured
- **WHEN** a TOML file written before this change contains `[wal].sync_interval_ms = 200` but no `[wal].batch_max_delay_ms`
- **THEN** the runtime treats `batch_max_delay_ms = 200`; if both fields are present, `batch_max_delay_ms` takes precedence

### Requirement: Ingester Startup Replay Before Port Open

The `ingester` role SHALL, before binding any HTTP or gRPC port, scan `wal.dir` for segments whose corresponding `ParquetFileMeta` row is absent, replay every well-formed record into the in-memory Arrow buffer via the same `IngestService::ingest` code path, and only then transition its readiness probe to "ready" so the router excludes the node from rotation until replay completes.

#### Scenario: Replay completes before traffic flows
- **WHEN** the process starts with `wal-000003.seg` containing 1,200 records not yet reflected in `parquet_file_meta`
- **THEN** those 1,200 records are pushed back into the buffer (which may immediately trigger a flush), and `GET /api/v1/healthz` returns `200 OK` only after replay finishes; while replay runs the endpoint returns `503 Service Unavailable`

#### Scenario: Corrupted tail is truncated, replay continues
- **WHEN** the last record of `wal-000003.seg` fails CRC32C verification
- **THEN** the segment is truncated at the prior intact boundary, the preceding records are replayed, and a `wal_recovery_truncated_total` counter increments

### Requirement: Stream HTTP CRUD

The system SHALL expose `GET/POST /api/v1/streams`, `GET/PUT/DELETE /api/v1/streams/:id`, `PUT /api/v1/streams/:id/schema`, and `PUT /api/v1/streams/:id/retention`, backed by `StreamRepository`. A `Stream` carries `{ id, org_id, name, stream_type: "logs" | "metrics" | "traces" | "extend", schema: Vec<FieldDef>, retention: { days, hot_days? }, indexed_fields: Vec<String>, created_at, updated_at }`. List supports `?page=&page_size=&filter=&stream_type=`; default `page_size = 50`, cap `200`.

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

### Requirement: Stream Type Includes Extend

`StreamDefinition.stream_type` SHALL accept the value `extend` in addition to `logs / metrics / traces`. Extend streams have a fixed schema `(key TEXT NOT NULL, value JSONB)` and SHALL be queryable by SQL but NOT written through the standard pipeline path (a pipeline targeting an extend stream is rejected at create time with `400 Bad Request`). Derived streams such as `copilot_traces` and `rum_*` are produced by their respective capability fan-outs and are not configured through this enum value.

#### Scenario: Extend stream created
- **WHEN** a POST creates a stream with `stream_type = "extend"`
- **THEN** the response is `201 Created`; ingest via `/api/v1/ingest/extend/<name>` writes key/value rows

#### Scenario: Pipeline targeting extend rejected
- **WHEN** a POST creates a pipeline with `stream_targets[0].stream_type = "extend"`
- **THEN** the response is `400 Bad Request` with `{ "error": "pipelines cannot target extend streams" }`

### Requirement: Ingester Per-Stream Buffer Pool

The ingester SHALL maintain one Arrow `RecordBatchBuilder` per active `(org, stream_type, stream)` triple, share a single tokio mutex per buffer, and never block one stream's writes on another stream's flush.

#### Scenario: Independent flush ordering
- **WHEN** stream `A` triggers a flush while stream `B` is mid-append
- **THEN** stream `B`'s append proceeds without waiting for `A`'s parquet upload to complete

#### Scenario: Schema extension synced to live buffer
- **WHEN** `StreamRepository::update_schema` adds a new column to stream `app`
- **THEN** the in-memory buffer's builder rolls forward to include the new column with null values for all previously buffered rows so the next flush emits a Parquet file that already has the new column

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

### Requirement: Schema Evolution

The system SHALL accept a wider set of fields than the registered schema and persist newly seen scalar fields via `StreamRepository::update_schema`, marking them `nullable = true`.

#### Scenario: New field appears in batch
- **WHEN** an event includes a field not in the current `StreamDefinition.schema`
- **THEN** the system adds the field to the schema (nullable, `indexed = false`) and accepts the event

#### Scenario: Type conflict on existing field
- **WHEN** an event provides a value whose type does not match the registered `FieldType`
- **THEN** the event is rejected with reason `type mismatch on field <name>: expected <T>, got <U>`

### Requirement: WAL Fsync Policy Honored At Runtime

The runtime instantiation of `WalPool` SHALL receive a fully constructed `FsyncPolicy` derived from `WalSettings` at bootstrap wire time. `WalPool::open_or_create` and its tests SHALL NOT call `FsyncPolicy::none_default()` as a hardcoded literal in production code paths. Changing `wal.flush_strategy` / `sync_level` / `batch_*` in `conf/config.toml` followed by a process restart SHALL alter the actual `sync_*` behaviour observed at the segment file.

#### Scenario: Configured batch strategy reaches the segment file
- **WHEN** the operator sets `[wal] flush_strategy = "batch"` `batch_max_delay_ms = 100` `batch_max_pending = 32` and the ingester boots
- **THEN** a strace / fs-level observation on the WAL directory shows `fdatasync` calls at intervals bounded by either 32 records or 100 ms whichever is first, and no calls when neither threshold is hit

#### Scenario: Configured every_write strategy reaches the segment file
- **WHEN** the operator sets `[wal] flush_strategy = "every_write"` `sync_level = "data"` and appends 10 records
- **THEN** the segment file's `fdatasync` is observed exactly 10 times before `WalPool::append` returns on the 10th call

#### Scenario: Configured none strategy reaches the segment file
- **WHEN** the operator sets `[wal] flush_strategy = "none"`
- **THEN** `fdatasync` / `fsync` are not invoked from the append path on any segment file regardless of throughput, until segment rotate or pool drain

#### Scenario: Bootstrap logs the effective policy
- **WHEN** the ingester role starts
- **THEN** an INFO-level log line is emitted with the resolved fields: `flush_strategy`, `sync_level`, `batch_max_pending`, `batch_max_delay_ms`, so operators can confirm the configuration took effect

#### Scenario: Fsync errors are counted, not retried
- **WHEN** `sync_data` returns an `io::Error` (e.g., disk full, fs corruption)
- **THEN** `wal_fsync_errors_total{kind}` increments by 1 with `kind ∈ {batch_flush, every_write, segment_rotate}`; the error is propagated to `WalPool::append` and surfaces to the ingest caller as an internal error; no retry is attempted at the WAL layer

### Requirement: WAL Per-Key Append Observability

The system SHALL expose two Prometheus metrics covering the contention on the per-`(org, stream_type, stream)` `Arc<Mutex<SegmentWal>>` held by `WalPool`, sufficient to identify stream-type-level mutex bottlenecks without leaking high-cardinality identifiers.

- `wal_append_lock_wait_seconds`: Histogram, label set `{stream_type}`, buckets `[0.0001, 0.001, 0.01, 0.1, 1.0]`. Observed value SHALL be the wall-clock duration between the moment `WalPool::append` begins waiting for the per-key mutex and the moment the mutex is acquired.
- `wal_append_inflight`: IntGauge, label set `{stream_type}`. Incremented when the mutex is acquired inside `WalPool::append`, decremented on drop of the critical section guard.

Labels SHALL NOT include `org_id` or `stream_name` to keep cardinality bounded at `|StreamType|` (currently 4).

#### Scenario: Lock wait histogram captures concurrent appends to one key
- **WHEN** 8 tasks concurrently call `WalPool::append` against the same `(org, logs, app)` key
- **THEN** after all tasks complete, `wal_append_lock_wait_seconds_count{stream_type="logs"} >= 8` and the histogram's max bucket reflects the actual serialisation delay

#### Scenario: Inflight gauge returns to zero
- **WHEN** all concurrent `WalPool::append` calls have returned
- **THEN** `wal_append_inflight{stream_type="logs"} == 0`

#### Scenario: Metrics differentiate stream_type
- **WHEN** appends are interleaved against `(_, logs, _)` and `(_, traces, _)` keys
- **THEN** the histogram exposes two distinct series `{stream_type="logs"}` and `{stream_type="traces"}` with independent counts

#### Scenario: Cardinality bound respected
- **WHEN** 10,000 distinct stream names are appended to across many orgs
- **THEN** the `/metrics` scrape exposes at most `|StreamType|` series per metric (no `org_id` / `stream_name` label appears)

### Requirement: WAL Term Source Injection Seam

`WalPool` SHALL accept an `Arc<dyn TermSource>` at construction time. `TermSource` is a trait `{ fn current_term(&self) -> u64 }` declared in `crates/infra/src/segment_wal/types.rs`. The default OSS bootstrap wire SHALL inject `StaticTermSource(1)`. Future consensus integrations SHALL be able to provide a custom `TermSource` implementation without modifying `WalPool::new` or `SegmentWal::new`.

On every `WalPool::append`, after acquiring the per-key mutex and before invoking `SegmentWal::write_raw`, the runtime SHALL call `term_source.current_term()` and, if the value differs from the segment's current term, invoke `SegmentWal::set_term(new)` so that subsequent record headers carry the up-to-date term value.

#### Scenario: StaticTermSource(1) is the OSS default
- **WHEN** the ingester starts with OSS bootstrap wire
- **THEN** every WAL record header carries `term = 1`

#### Scenario: Custom TermSource value propagates into record headers
- **WHEN** a `WalPool` is constructed with `Arc::new(StaticTermSource(7))` and one record is appended
- **THEN** `scan_segment_file_readonly` on the resulting segment returns a `WalRecord` with `term == 7`

#### Scenario: Term change between two appends is reflected per-record
- **WHEN** a `WalPool` is constructed with a `TermSource` whose `current_term()` returns `7` for the first call and `9` for the second, and two records are appended
- **THEN** the first record's header carries `term = 7` and the second carries `term = 9`

#### Scenario: WalPool::new signature does not assume raft is integrated
- **WHEN** developers add a hypothetical `RaftTermSource` implementing `TermSource`
- **THEN** swapping `StaticTermSource(1)` for `RaftTermSource::new(raft_node)` at the bootstrap wire site is sufficient; `WalPool::new`, `SegmentWal::new`, and the WAL record format SHALL NOT require modification

### Requirement: Profiles Stream Type Ingestion

The ingestion pipeline SHALL accept `StreamType::Profiles` batches through the same `IngestService::ingest` path as other signals — honoring drain gating, schema-on-write stream creation, and masking — while treating profiles streams as ineligible pipeline transform targets (`allowed_as_pipeline_target` returns `false`).

#### Scenario: Profiles batch flows through the unified pipeline

- **WHEN** a profiles metadata batch is ingested for a not-yet-existing stream
- **THEN** the stream is auto-created via schema-on-write
- **AND** the metadata rows are persisted with `stream_type = Profiles`

#### Scenario: Profiles stream rejected as pipeline target

- **WHEN** a pipeline is configured to target a profiles stream
- **THEN** the configuration is rejected because profiles are not an allowed pipeline target

#### Scenario: Draining node rejects profiles writes

- **WHEN** the node is draining and a profiles batch arrives
- **THEN** ingestion returns `503` and persists nothing
