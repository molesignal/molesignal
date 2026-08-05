# Storage Capability

## Purpose

parquet 文件读写、对象存储多 backend（local/s3/azure/gcs）+ 生产化（multipart / range download / 重试 / 超时 / 并发上限 / 健康探测 / 跨 backend 指标）、ParquetFileMeta 索引与分区裁剪、Tantivy 倒排索引归档与缓存、Compactor 合并 + 保留 + 失败回滚、parquet footer 缓存。
## Requirements
### Requirement: Multi-Backend Object Store

The system SHALL build an `object_store::ObjectStore` from `[object_store]` settings supporting `backend = "local" | "s3" | "azure" | "gcs"`, with credentials, region, bucket, and endpoint read from the same config table.

#### Scenario: Local backend uses prefixed directory
- **WHEN** `backend = "local"` and `root = "./data/objects"`
- **THEN** the constructed store writes under that directory and creates it if missing

#### Scenario: S3 backend uses configured credentials
- **WHEN** `backend = "s3"` and `access_key`, `secret_key`, `region`, `bucket` are populated
- **THEN** the store routes requests to AWS S3 (or the `endpoint` override for MinIO-compatible stores)

#### Scenario: Unknown backend
- **WHEN** `backend` is not one of the four supported values
- **THEN** server startup fails with `Error::Invalid("unsupported object_store backend: <name>")`

### Requirement: Object Store Static Credential Sources

The system SHALL accept S3-style static credentials (`access_key` + `secret_key`) from three sources in this precedence: (1) `MS_OBJECT_STORE_ACCESS_KEY` / `SECRET_KEY` environment variables (highest), (2) `[object_store].credentials_file` pointing to a key-value file with `access_key=...` and `secret_key=...` lines, (3) inline `[object_store].access_key` / `secret_key` in TOML (lowest). Cloud-native credential chains (IAM role, managed identity, workload identity) are NOT supported in this change.

#### Scenario: Environment variable wins over file
- **WHEN** both env vars and `credentials_file` are set
- **THEN** the env var pair is used and a `object_store_credentials_source="env"` info log is emitted at startup

#### Scenario: Credentials file missing key fails fast
- **WHEN** `credentials_file` exists but lacks `secret_key`
- **THEN** `bootstrap::build_state` returns `Err("object_store credentials_file missing secret_key")` and the process exits

### Requirement: Object Store Health Check

`bootstrap::build_state` SHALL perform a startup probe (PUT → GET → DELETE a 128-byte object under `_health/<uuid>`); failure SHALL abort startup. The HTTP server SHALL additionally run a background probe every `object_store.health_probe_interval_secs` (default 30s); three consecutive failures SHALL flip `/api/v1/healthz` to `503 Service Unavailable` with body `{ "status": "degraded", "reason": "object store unreachable" }` while `/metrics` continues to serve.

#### Scenario: Startup probe failure aborts boot
- **WHEN** the configured bucket does not exist or credentials are wrong
- **THEN** `main()` returns an error before role subsystems start; the exit log includes the probe key and error

#### Scenario: Three consecutive runtime failures degrade health
- **WHEN** the background probe fails three times in a row
- **THEN** `/api/v1/healthz` returns `503` until the next successful probe; `/metrics` still returns `200`

### Requirement: Parquet Writer

The system SHALL serialize an `Arrow RecordBatch` (built from a flushed in-memory buffer) to a parquet stream with snappy compression and upload it to the object store under `{org}/{stream}/{YYYY-MM-DD}/{ksuid}.parquet`, recording a `ParquetFileMeta` row before returning success.

#### Scenario: Successful write inserts ParquetFileMeta
- **WHEN** the writer finishes uploading
- **THEN** a `ParquetFileMeta` row is inserted with `object_key`, `time_range = min..=max(_timestamp)`, `rows`, `size_bytes`, `min_values`/`max_values` for indexed fields, and `deleted = false`

#### Scenario: ParquetFileMeta insert failure deletes orphan object
- **WHEN** the parquet upload succeeds but the `ParquetFileMetaRepository::insert` call fails
- **THEN** the writer attempts to delete the just-uploaded object and bubbles the original error

### Requirement: Multipart Upload For Large Objects

The system SHALL perform multipart uploads through `object_store::ObjectStore::put_multipart` when the payload exceeds `object_store.multipart_threshold_mb` (default 32 MiB), splitting the bytes into `object_store.multipart_part_size_mb` (default 8 MiB) chunks and uploading them with up to `object_store.max_concurrency` (default 8) in-flight parts. Smaller payloads SHALL still use the simple `put` path.

#### Scenario: 200 MiB parquet uses multipart
- **WHEN** the ingester flushes a 200 MiB parquet
- **THEN** multipart upload is used (`object_store_operations_total{op="put_multipart"}` increments) and the file is reconstructible end-to-end via `get`

#### Scenario: 1 MiB Tantivy archive uses single put
- **WHEN** a 1 MiB Tantivy archive is uploaded
- **THEN** a single `put` is issued (`object_store_operations_total{op="put"}` increments, not `put_multipart`)

#### Scenario: Multipart abort on mid-stream error
- **WHEN** a part upload fails after the first 3 of 10 succeed
- **THEN** the in-progress `MultipartUpload` is aborted (or recorded for cleanup if backend doesn't support abort) and `object_store_errors_total{op="put_multipart"}` increments; the next ingester tick retries the entire flush from the start with the buffer intact

### Requirement: Parquet Reader

The system SHALL stream parquet files from the object store as Arrow `RecordBatch`es with predicate and projection pushdown, exposed via a `ParquetExec` registered with DataFusion.

#### Scenario: Reader exposes ParquetExec
- **WHEN** a query plan needs to scan a `ParquetFileMeta` set
- **THEN** the system constructs a `ParquetExec` whose `FileScanConfig` lists those objects and supports projection of the columns referenced in the query

### Requirement: Parquet Footer Metadata Cache

Reading a parquet file's footer metadata SHALL go through the `caching::parquet_meta` cache so the same `object_key` is fetched at most once per process per TTL (`caching.parquet_meta.ttl_secs`, default 600s).

#### Scenario: Two queries share a footer fetch
- **WHEN** two concurrent queries each need the metadata of `orgA/app/.../X.parquet` and neither has been seen before
- **THEN** exactly one object-store `get_range` for the footer is issued; the second query waits on the same future and both receive the resulting `Arc<ParquetMetaData>`

### Requirement: Range Download For Large Objects

The system SHALL perform parallel range-based downloads when an object's known size exceeds `object_store.range_threshold_mb` (default 16 MiB), splitting into `object_store.range_chunk_mb` (default 8 MiB) chunks and issuing concurrent `get_range` calls capped by `object_store.max_concurrency`.

#### Scenario: 100 MiB parquet downloaded in parallel
- **WHEN** the querier needs a 100 MiB parquet for scan
- **THEN** the download issues multiple `get_range` calls in parallel (each ≤ 8 MiB) instead of a single `get`, reducing wall-clock fetch latency

#### Scenario: Small footer fetch stays a single get_range
- **WHEN** the parquet metadata footer is fetched (typically < 1 MiB)
- **THEN** a single `get_range` is issued, no parallelization

### Requirement: Retry With Exponential Backoff and Jitter

Every object store read or write operation SHALL be wrapped by a retry policy parameterized by `[object_store.retry] max_attempts, base_backoff_ms, max_backoff_ms, jitter_ratio`. Retryable errors include HTTP 5xx, `Throttling`, `SlowDown`, `Timeout`, and transient connection failures. Permanent errors (404, 403, malformed request) SHALL NOT retry. The retry policy SHALL bound total time to `max_backoff_ms * max_attempts` worst-case.

#### Scenario: Transient 5xx eventually succeeds
- **WHEN** an S3 PUT fails with 503 twice and succeeds on the third attempt
- **THEN** the call returns success, two `object_store_errors_total{reason="503"}` and one `object_store_operations_total{op="put"}` are recorded, total elapsed time ≤ `max_backoff_ms * 2 + epsilon`

#### Scenario: Permanent 403 does not retry
- **WHEN** an S3 PUT fails with 403
- **THEN** the call returns the error immediately, exactly one `object_store_errors_total{reason="403"}` is recorded, no backoff is incurred

#### Scenario: Max attempts exhausted
- **WHEN** an S3 PUT fails with 503 on all `max_attempts` (default 4) tries
- **THEN** the final error is propagated unwrapped (with attempt count in the error context), and the operation fails

### Requirement: Per-Operation Timeout And Concurrency Cap

Every object store operation SHALL be wrapped in `tokio::time::timeout(object_store.op_timeout_secs)` (default 30s) and SHALL acquire a permit from a process-wide `Semaphore` of `object_store.max_concurrency` size before issuing.

#### Scenario: Timeout fires on stuck operation
- **WHEN** an S3 PUT does not return within `op_timeout_secs`
- **THEN** the call returns `Error::Timeout`, the permit is released, and a retry attempt may proceed

#### Scenario: Concurrency cap respected
- **WHEN** 100 concurrent flushes target the object store and `max_concurrency = 8`
- **THEN** at most 8 calls are in flight at any moment; the rest wait for permits

### Requirement: Cross-Backend Object Store Metrics

The system SHALL export `object_store_operations_total{backend, op}`, `object_store_bytes_total{backend, op, direction}`, `object_store_errors_total{backend, op, reason}`, `object_store_op_duration_seconds_bucket{backend, op}`, and `object_store_health_check_duration_seconds_bucket{backend}` via the global `prometheus::Registry`. `backend` label values are `"local" | "s3" | "azure" | "gcs"`.

#### Scenario: Metrics reflect mixed backend
- **WHEN** the configured backend is `s3` and the process has performed 100 PUTs and 200 GETs
- **THEN** `/metrics` exposes `object_store_operations_total{backend="s3",op="put"} 100` and `..."get"} 200`

### Requirement: ParquetFileMeta Partition Pruning

`ParquetFileMetaRepository::find` SHALL accept a `TimeRange` and return only files whose `time_range` overlaps it, ordered by `time_range.start`.

#### Scenario: Range filter
- **WHEN** a query restricts `_timestamp BETWEEN t0 AND t1`
- **THEN** only `ParquetFileMeta` rows with `time_range.end >= t0 AND time_range.start <= t1 AND deleted = false` are returned

### Requirement: Compactor

The compactor role SHALL periodically scan `ParquetFileMeta` rows under `compactor.target_mb` (default 32 MiB) for each `(org, stream, stream_type, date)` tuple, greedily merge time-adjacent files in batches that stay under the target size, atomically swap the metadata via `ParquetFileMetaRepository::replace`, and delete the merged source objects from the object store after the swap commits. The compactor SHALL run no more than `compactor.max_concurrent_groups` (default 4) merges in parallel and SHALL invalidate the `parquet_file_meta` cache prefix for every affected `(org, stream, stream_type)`.

#### Scenario: Merge happens atomically
- **WHEN** the compactor merges files `[a, b, c]` into file `d`
- **THEN** `ParquetFileMetaRepository::replace(&[a, b, c], vec![d])` is called in a single transaction; on success the objects backing `a`, `b`, `c` are removed; on failure `d`'s object is deleted and `a`, `b`, `c` remain referenceable

#### Scenario: Retention sweep
- **WHEN** a file's `time_range.end` is older than the owning `StreamDefinition.retention.days`
- **THEN** the compactor marks the `ParquetFileMeta` as `deleted = true` and removes the object on the next sweep

#### Scenario: Concurrency cap respected
- **WHEN** 20 merge groups become eligible on the same tick and `compactor.max_concurrent_groups = 4`
- **THEN** at most four merges run in parallel; the remaining 16 are queued for subsequent ticks

### Requirement: Compactor Failure Tolerance

When the compactor encounters a transient error during `ParquetFileMetaRepository::replace`, the merged-target object SHALL be deleted from the object store, the source files SHALL remain referenceable for the next compaction tick, and the failure SHALL be recorded with `compactor_failures_total{reason}`.

#### Scenario: replace() rolls back, target object cleaned up
- **WHEN** the merged parquet is uploaded successfully but the `replace` transaction fails (e.g., constraint violation, lock timeout)
- **THEN** the just-uploaded merged object is deleted from the object store, `compactor_failures_total{reason="replace_failed"}` increments by one, and the original source `ParquetFileMeta` rows remain intact and eligible for the next compaction attempt

#### Scenario: Object delete failure after successful replace
- **WHEN** `replace` commits but deleting one of the source objects fails (e.g., S3 transient 503)
- **THEN** the metadata swap is preserved (sources are already tombstoned), the orphaned source object is logged with key + reason, and a separate `retention` sweep on the next tick reattempts the deletion

### Requirement: Tantivy Inverted Index

The ingester SHALL build a Tantivy index per parquet file for any field marked `indexed = true`, serialize the index as a **Puffin v1 file** (multi-blob single-object container; one blob per tantivy segment file plus one `O2TtvFooterV1` footer-cache blob), and store it at the canonical sidecar key `files/{org}/index/{stream}_{stream_type}/{YYYY}/{MM}/{DD}/00/{ksuid}.ttv` derived from the parquet object key via `convert_parquet_file_name_to_tantivy_file`. The querier SHALL use the puffin sidecar to skip files whose index proves no matches exist for a `MATCH(field, term)` predicate. Loading a puffin sidecar SHALL go through `PuffinDirReader::from_object_store`, fetching only the footer (≥ 12 bytes + payload) up front; subsequent tantivy reads SHALL translate into per-blob sub-range `get_range` calls so the full sidecar is never downloaded as a single blob. The legacy `tar+zstd`-encoded sidecar at `{object_key}.tantivy.tar.zst` SHALL no longer be written or read; missing puffin sidecars SHALL be handled by the existing `tantivy_missing_archive_total` fall-through.

#### Scenario: Index skips file with no match
- **WHEN** a SQL query includes `MATCH(message, 'panic')` and a candidate file's tantivy index contains no posting for `panic`
- **THEN** that file is excluded from the `ParquetExec` for that query and a `tantivy_pruned_files_total` counter increments

#### Scenario: Missing puffin sidecar falls back to full scan
- **WHEN** a `MATCH` predicate targets a field but the parquet has no companion `.ttv` puffin sidecar (e.g. field was not yet `indexed=true` when the file was written, or the legacy `.tantivy.tar.zst` exists but the puffin sidecar does not)
- **THEN** that file is kept in the candidate set, the `MATCH` is evaluated row-by-row by DataFusion, and `tantivy_missing_archive_total` increments; **the legacy `.tantivy.tar.zst` SHALL NOT be downloaded or parsed under any circumstance**

#### Scenario: Sidecar key follows puffin canonical layout
- **WHEN** an ingester flushes a parquet at key `orgA/logs/log_app/2026-01-15/abc123.parquet`
- **THEN** the puffin sidecar SHALL be uploaded to `files/orgA/index/log_app_logs/2026/01/15/00/abc123.ttv`; no other sidecar location SHALL be created

#### Scenario: Querier loads sidecar via footer only
- **WHEN** the querier opens a 12 MiB puffin sidecar to check a single `(field, term)` predicate
- **THEN** the first `object_store` access is a `get_range` for the trailing 12 bytes (footer tail) followed by one `get_range` for the footer payload; the full 12 MiB is NOT downloaded; subsequent tantivy reads issue per-blob `get_range` calls only for the segment files actually touched

#### Scenario: Index sidecar size bounded
- **WHEN** the ingester writes a Tantivy puffin sidecar for a parquet that is `S` bytes
- **THEN** the sidecar size MUST be less than `S * 0.20`; if it exceeds 20% the writer logs a warning with the field names and proceeds (no hard fail)

### Requirement: Async File Downloader

The system SHALL expose `POST /api/v1/files/download` accepting `{ object_keys: [<key>], expires_in_secs }` and returning `{ download_url, expires_at }`. The URL SHALL be a pre-signed S3 URL when the backend is `s3`, or a temporary streaming endpoint `/api/v1/files/stream/<token>` for other backends. Required permission: `Permission::StreamRead` for the underlying stream.

#### Scenario: Pre-signed URL for S3

- **WHEN** a user POSTs `{ "object_keys": ["app/2026/05/file-xxx.parquet"], "expires_in_secs": 3600 }` on an S3-backed deployment
- **THEN** the response carries an HTTPS pre-signed URL that downloads the parquet directly from S3

#### Scenario: Streaming token for local backend

- **WHEN** the same request is made on a `local` backend deployment
- **THEN** the response carries `/api/v1/files/stream/<token>`; GET on that URL streams the file body for the duration of `expires_in_secs`

### Requirement: Org Schema Cache

The system SHALL maintain an in-memory `OrgSchemaCache` keyed by `(org_id, stream_name, stream_type)` → `Arc<Schema>` with TTL 60s + invalidation on `StreamRepository::update_schema`. Cache hits SHALL avoid the DB roundtrip on every ingest event.

#### Scenario: Schema update invalidates cache

- **WHEN** `PUT /api/v1/streams/<id>/schema` adds a column
- **THEN** subsequent ingest events for that stream see the new schema within 1 second (cache invalidation propagated)

#### Scenario: Cache hit avoids DB

- **WHEN** 10000 ingest events for the same `(org, stream)` arrive in a 60-second window
- **THEN** at most 1 DB roundtrip is made to fetch the schema; the remaining 9999 events use the cache

### Requirement: ParquetFileMeta Dump Spillover

When `[storage.parquet_file_meta_dump] enabled = true` (default), the system SHALL run a periodic worker that, for every `(org, stream, stream_type, partition_level, partition_key)` partition whose effective time window is older than `today() - storage.parquet_file_meta_dump.cold_after_days` (default 30 days), serializes every live `ParquetFileMeta` row of that partition into a **columnar Parquet file** with the fixed schema `{ id: Utf8, org_id: Utf8, stream: Utf8, stream_type: Utf8, date: Utf8, object_key: Utf8, deleted: Boolean, rows: Int64, size_bytes: Int64, time_start_micros: Int64, time_end_micros: Int64, min_values_json: Utf8, max_values_json: Utf8, updated_at_micros: Int64 }`, uploads it to `{org}/_parquet_file_meta_dump/{stream_type}/{stream}/{partition_key}.parquet`, inserts an index row into the `parquet_file_meta_dump` table containing `(org, stream, stream_type, partition_level, partition_key, object_key, deleted=false, rows_in_dump, min_ts_micros, max_ts_micros, size_bytes, created_at_micros)`, inserts an aggregate row into `parquet_file_meta_dump_stats` containing `(object_key, rows_total, files_total, time_start_micros, time_end_micros, storage_size_bytes, updated_at_micros)`, and deletes the corresponding rows from the main `parquet_file_meta` table. `partition_level` SHALL come from `[storage.parquet_file_meta_dump] partition_level` (`"daily"` default, `"hourly"` opt-in) and the corresponding `partition_key` SHALL be `YYYY-MM-DD` or `YYYY-MM-DD-HH`. The dump-and-delete operation SHALL satisfy: (a) object upload happens before any database mutation; (b) `parquet_file_meta_dump` insert + `parquet_file_meta_dump_stats` insert + `parquet_file_meta` delete run in a single database transaction; (c) the transaction SHALL acquire `pg_try_advisory_xact_lock(hashtext(org_id||'|'||stream||'|'||stream_type||'|'||partition_level||'|'||partition_key))` before any work; failure to acquire SHALL increment `parquet_file_meta_dump_partitions_skipped_total{reason="locked"}` and skip; (d) failure at any step after lock acquisition leaves the main table untouched and a retry on the next worker tick is safe. The worker SHALL respect `storage.parquet_file_meta_dump.max_partitions_per_tick` (default 100) and `storage.parquet_file_meta_dump.interval_secs` (default 3600). The dump parquet schema SHALL NOT include any single-column JSON fallback path; the legacy single-column `meta_json: Utf8` format is no longer produced or readable.

#### Scenario: Cold daily partition gets dumped end-to-end
- **WHEN** a partition `(orgA, log_app, logs, daily, 2026-01-15)` has effective end older than `today - 30` and 12 live ParquetFileMeta rows
- **THEN** the worker uploads a single `orgA/_parquet_file_meta_dump/logs/log_app/2026-01-15.parquet` whose schema matches the columnar contract (14 fixed columns), inserts one `parquet_file_meta_dump` row with `rows_in_dump = 12 AND deleted = false AND partition_level = "daily"`, inserts one `parquet_file_meta_dump_stats` row with `rows_total = 12 AND files_total = 12`, deletes the 12 main-table rows, and commits all of the above in the same transaction

#### Scenario: Cold hourly partition uses hour-suffixed key
- **WHEN** `[storage.parquet_file_meta_dump] partition_level = "hourly"` and a partition `(orgA, log_app, logs, hourly, 2026-01-15-13)` is eligible
- **THEN** the dump object key is `orgA/_parquet_file_meta_dump/logs/log_app/2026-01-15-13.parquet` and the `parquet_file_meta_dump` row stores `partition_level = "hourly" AND partition_key = "2026-01-15-13"`

#### Scenario: Object upload fails — main table untouched
- **WHEN** the dump parquet upload to object_store fails (network error)
- **THEN** no `parquet_file_meta_dump` row is inserted, no `parquet_file_meta_dump_stats` row is inserted, no `parquet_file_meta` row is deleted, `parquet_file_meta_dump_partitions_skipped_total{reason="error"}` increments by 1, and the partition stays eligible for the next tick

#### Scenario: Transaction failure after upload — re-dumpable next tick
- **WHEN** the upload succeeds and the advisory lock is held but the `INSERT parquet_file_meta_dump + INSERT parquet_file_meta_dump_stats + DELETE parquet_file_meta` transaction fails
- **THEN** the uploaded object remains in the bucket (becomes a transient orphan), no main table rows are deleted, and the next tick re-uploads the same content (overwriting the orphan); `parquet_file_meta_dump_partitions_skipped_total{reason="error"}` increments

#### Scenario: Concurrent compactor loses the lock
- **WHEN** two worker instances tick on the same partition within the same window
- **THEN** exactly one acquires `pg_try_advisory_xact_lock` and proceeds; the other observes `false`, increments `parquet_file_meta_dump_partitions_skipped_total{reason="locked"}`, and moves on

#### Scenario: Worker honors max_partitions_per_tick
- **WHEN** 500 partitions are eligible and `max_partitions_per_tick = 100`
- **THEN** exactly 100 partitions are dumped this tick and the remaining 400 are dumped over the following ticks

#### Scenario: Worker disabled
- **WHEN** `[storage.parquet_file_meta_dump] enabled = false`
- **THEN** the worker does not run, no dumps are produced, and the main `parquet_file_meta` table is the sole source for `ParquetFileMetaRepository::find`

### Requirement: ParquetFileMeta Dump Query Path

`ParquetFileMetaRepository::find(time_range)` SHALL transparently merge results from the main `parquet_file_meta` table and any relevant dump parquet files. When the requested `time_range.start` is older than `today() - cold_after_days`, the implementation SHALL: (1) query the main table for hot rows in range; (2) query `parquet_file_meta_dump` for index rows whose `deleted = false AND time_end_micros >= time_range.start AND time_start_micros <= time_range.end`; (3) for every dump object load it through `read_dump_filtered(store, object_key, time_range)` which SHALL (3a) consult an in-process LRU cache keyed by `(org_id, stream, stream_type, partition_level, partition_key)` returning `Arc<Vec<ParquetFileMeta>>` on hit; (3b) on miss, download via `ProductionObjectStore` (passing through `parquet_meta` and `parquet_disk_cache` layers), open the parquet with an Arrow predicate `time_end_micros >= time_range.start AND time_start_micros <= time_range.end` registered as a row filter, decode columns directly into `ParquetFileMeta` (no JSON deserialization of the row), write the unfiltered `Arc<Vec<ParquetFileMeta>>` into the cache; (4) merge with the hot results, deduplicate by `ParquetFileMeta.id`, and sort by `time_range.start`. The merged result SHALL be indistinguishable from what a pre-dump main-table query would have returned. The query path SHALL NOT attempt to read any object whose corresponding `parquet_file_meta_dump` row has `deleted = true`.

#### Scenario: Pure-hot query bypasses dump path
- **WHEN** `time_range.start >= today - cold_after_days`
- **THEN** only the main `parquet_file_meta` table is queried; no dump parquet downloads occur; `parquet_file_meta_dump_query_hits_total` is NOT incremented; the dump cache is not consulted

#### Scenario: Cross-boundary query merges sources with predicate pushdown
- **WHEN** `time_range = (today - 45 days)..(today - 5 days)` and the partition `(today - 45 ... today - 30)` has been dumped, and the dump file contains rows whose `time_end_micros < time_range.start`
- **THEN** the implementation queries both the main table (for `today - 30 ... today - 5`) and dumps (for `today - 45 ... today - 30`); rows that do not satisfy `time_end_micros >= time_range.start AND time_start_micros <= time_range.end` are filtered at the Parquet row filter layer (not after construction); the merged result is dedup'd by `id` and returned in `time_range.start` order; `parquet_file_meta_dump_query_hits_total` is incremented

#### Scenario: Cache hit avoids object store fetch
- **WHEN** two consecutive cross-boundary queries hit the same dump partition within the cache TTL
- **THEN** the second query reads `Arc<Vec<ParquetFileMeta>>` from the in-process cache; no `ProductionObjectStore::get` is issued for that dump; `cache_parquet_file_meta_dump_hits_total` is incremented; the second query's `parquet_file_meta_dump_query_load_seconds` observation is below the eager-fetch latency

#### Scenario: Deleted dump rows are skipped
- **WHEN** a `parquet_file_meta_dump` row has `deleted = true` (left over from a `delete_by_time_range` rewrite) and a query's range overlaps its window
- **THEN** the row is not selected by the dump-index query; no GET is issued; the rewritten replacement dump is selected instead

#### Scenario: Duplicate id between hot and cold survives one row
- **WHEN** a transient duplicate exists (worker crashed between insert-dump and delete-main, then retried successfully on the next tick BUT a query lands in the brief window) so the same `ParquetFileMeta.id` appears in both main table and dump
- **THEN** the merge dedups by `id` and the query returns exactly one row for that id

#### Scenario: Dump parquet load latency tracked
- **WHEN** a cross-boundary query loads N dump parquets (cache miss path)
- **THEN** each load duration is observed in the `parquet_file_meta_dump_query_load_seconds` histogram; cache-hit path does NOT contribute observations

### Requirement: ParquetFileMeta Dump Metrics

The system SHALL export Prometheus metrics for the dump subsystem: `parquet_file_meta_dump_partitions_written_total` (Counter), `parquet_file_meta_dump_rows_written_total` (Counter), `parquet_file_meta_dump_partitions_skipped_total` with a `reason` label (`empty | locked | error | duplicate_id`), `parquet_file_meta_dump_query_hits_total` (Counter), `parquet_file_meta_dump_query_load_seconds` (Histogram), `parquet_file_meta_dump_delete_partitions_rewritten_total` (Counter), `parquet_file_meta_dump_delete_partitions_dropped_total` (Counter).

#### Scenario: Each successful dump increments two counters
- **WHEN** the worker dumps a partition with 50 ParquetFileMeta rows
- **THEN** `parquet_file_meta_dump_partitions_written_total += 1` and `parquet_file_meta_dump_rows_written_total += 50`

#### Scenario: Skip reasons are distinguishable
- **WHEN** the worker encounters an empty partition (already fully retention-deleted), a partition whose advisory lock is already held, and a transient PG error in the same tick
- **THEN** `parquet_file_meta_dump_partitions_skipped_total{reason="empty"}`, `parquet_file_meta_dump_partitions_skipped_total{reason="locked"}` and `parquet_file_meta_dump_partitions_skipped_total{reason="error"}` each increment by 1

#### Scenario: delete_by_time_range metrics distinguish rewrite vs drop
- **WHEN** `ParquetFileMetaDumpService::delete_by_time_range` processes one fully-overlapping dump (no rows kept) and one partially-overlapping dump (some rows kept and rewritten)
- **THEN** `parquet_file_meta_dump_delete_partitions_dropped_total` increments by 1 (full overlap) and `parquet_file_meta_dump_delete_partitions_rewritten_total` increments by 1 (partial overlap)

### Requirement: ParquetFileMeta Dump Range Deletion

`ParquetFileMetaDumpService::delete_by_time_range(org_id, stream, stream_type, target_range)` SHALL provide an atomic dump-side deletion path for retention and compliance workflows. The implementation SHALL: (1) open a database transaction and `SELECT … FOR UPDATE` every `parquet_file_meta_dump` row matching `(org, stream, stream_type)` whose `deleted = false AND [time_start_micros, time_end_micros]` overlaps `target_range`; (2) for each selected dump, download the columnar parquet and partition its rows into `to_keep = rows whose [time_start, time_end] does NOT lie entirely within target_range` and `to_delete = the rest`; (3) if `to_keep` equals the full set, leave the dump untouched; (4) if `to_keep` is empty, mark the existing `parquet_file_meta_dump` row `deleted = true`, delete its `parquet_file_meta_dump_stats` row, and enqueue the underlying object for asynchronous deletion; (5) otherwise, serialize `to_keep` into a new columnar parquet, upload it to a new object key `{base_path}/{partition_key}.r{n}.parquet` where `n` is `max(existing rewrite_seq for this partition_key) + 1`, insert a new `parquet_file_meta_dump` row (`deleted = false`), insert a new `parquet_file_meta_dump_stats` row with recomputed aggregates, mark the old `parquet_file_meta_dump` row `deleted = true`, delete the old `parquet_file_meta_dump_stats` row, and enqueue the old object for asynchronous deletion. Steps (3)-(5) for every affected dump SHALL commit in the same database transaction. Asynchronous object deletion failures SHALL be retried by the existing object-cleanup sweep and SHALL NOT block subsequent dump or query operations.

#### Scenario: Full-overlap dump is dropped, not rewritten
- **WHEN** `delete_by_time_range` selects a dump whose every row's `[time_start, time_end]` lies inside `target_range`
- **THEN** no new parquet is produced; the existing `parquet_file_meta_dump` row is updated to `deleted = true`; its `parquet_file_meta_dump_stats` row is deleted; the underlying object is enqueued for async deletion; `parquet_file_meta_dump_delete_partitions_dropped_total += 1`

#### Scenario: Partial-overlap dump rewrites with new rewrite_seq
- **WHEN** `delete_by_time_range` selects a dump whose rows split into 4 kept and 6 to-delete, and the partition has no prior rewrite
- **THEN** a new parquet is uploaded to `{base}/{partition_key}.r1.parquet` containing the 4 kept rows; a new `parquet_file_meta_dump` row is inserted (`deleted = false`); a new `parquet_file_meta_dump_stats` row records `rows_total = 4`; the old `parquet_file_meta_dump` row is set `deleted = true` and its stats row is deleted; `parquet_file_meta_dump_delete_partitions_rewritten_total += 1`; the old object is enqueued for async deletion

#### Scenario: No-overlap dump is left intact
- **WHEN** `delete_by_time_range` selects no dumps overlapping `target_range`
- **THEN** the transaction commits with zero mutations; no metrics increment

#### Scenario: Concurrent reader observes only one consistent set
- **WHEN** a concurrent `ParquetFileMetaRepository::find(time_range)` runs during the rewrite transaction
- **THEN** the reader sees either (a) the old `parquet_file_meta_dump` row (pre-commit isolation) or (b) the new row with `deleted = false` plus the old row with `deleted = true` (post-commit); in both cases exactly one dump's rows are merged for the affected partition

### Requirement: ParquetFileMeta Dump Aggregate Stats Table

The system SHALL maintain a `parquet_file_meta_dump_stats` table with one row per live dump object holding `(object_key, rows_total, files_total, time_start_micros, time_end_micros, storage_size_bytes, updated_at_micros)`. The writer and rewriter SHALL keep this table consistent with the `parquet_file_meta_dump` table via foreign-key cascade and same-transaction inserts/deletes. Stream stats consumers (UI, API, periodic reporters) SHALL read aggregated dump-tier metrics from this table without opening any dump parquet object.

#### Scenario: Stats row appears alongside every dump row
- **WHEN** the worker successfully dumps a partition with 30 rows totalling 12 MiB
- **THEN** the same transaction inserts a `parquet_file_meta_dump_stats` row with `rows_total = 30 AND files_total = 30 AND storage_size_bytes = 12 * 1024 * 1024`

#### Scenario: Stats row is removed via FK cascade
- **WHEN** the `parquet_file_meta_dump` row is deleted (or marked `deleted = true` and cleaned by the sweep)
- **THEN** the corresponding `parquet_file_meta_dump_stats` row is removed in the same transaction

#### Scenario: Aggregate query never opens dump parquet
- **WHEN** a stream stats consumer asks for total cold-tier rows and bytes for `(orgA, log_app, logs)`
- **THEN** the implementation issues a single `SELECT SUM(rows_total), SUM(storage_size_bytes) FROM parquet_file_meta_dump_stats JOIN parquet_file_meta_dump …` query; no `ProductionObjectStore::get` calls are made; no parquet readers are constructed

### Requirement: ParquetFileMeta Dump Partition Level Configuration

`[storage.parquet_file_meta_dump] partition_level` SHALL accept the literal strings `"daily"` (default) or `"hourly"` and the worker SHALL key new dumps by `YYYY-MM-DD` or `YYYY-MM-DD-HH` respectively. Existing dumps written under the prior partition level SHALL remain readable; changing `partition_level` SHALL only affect partitions produced after the change. Mixing partition levels in the same `(org, stream, stream_type)` namespace is permitted by the schema and the query path SHALL select dumps regardless of their `partition_level` value.

#### Scenario: Daily default writes daily partition keys
- **WHEN** the worker dumps a partition under `partition_level = "daily"`
- **THEN** the `partition_key` is `YYYY-MM-DD` and the object key suffix matches `…/{stream}/2026-01-15.parquet`

#### Scenario: Hourly opt-in writes hour-suffixed partition keys
- **WHEN** the worker dumps a partition under `partition_level = "hourly"`
- **THEN** the `partition_key` is `YYYY-MM-DD-HH` and the object key suffix matches `…/{stream}/2026-01-15-13.parquet`

#### Scenario: Query joins across mixed partition levels
- **WHEN** a stream has 20 daily dumps from before the switch and 14 hourly dumps from after the switch, and a query's `time_range` spans both
- **THEN** the dump-index query selects rows of both `partition_level = "daily"` and `partition_level = "hourly"`; the loader processes each dump under its own partition level; the merged result covers the full range without gaps or duplicates

### Requirement: Profile Archive Object Layout

The storage layer SHALL archive each ingested profile as a zstd-compressed pprof object at object_store key `profiles/<org_id>/<service>/<profile_type>/<yyyymmdd>/<profile_id>.pprof.zst`, and SHALL remove the archived object together with its profile metadata when retention expires.

#### Scenario: Archive key format

- **WHEN** a profile for org `o1`, service `api`, type `cpu` is archived for date 2026-06-18
- **THEN** the object key is `profiles/o1/api/cpu/20260618/<id>.pprof.zst`

#### Scenario: Retention removes blob and metadata together

- **WHEN** a profiles stream partition passes its retention window
- **THEN** both the parquet metadata rows and the archive objects they reference are deleted
