## ADDED Requirements

### Requirement: Parquet Footer Metadata Cache

Reading a parquet file's footer metadata SHALL go through the `caching::parquet_meta` cache so the same `object_key` is fetched at most once per process per TTL (`caching.parquet_meta.ttl_secs`, default 600s).

#### Scenario: Two queries share a footer fetch
- **WHEN** two concurrent queries each need the metadata of `orgA/app/.../X.parquet` and neither has been seen before
- **THEN** exactly one object-store `get_range` for the footer is issued; the second query waits on the same future and both receive the resulting `Arc<ParquetMetaData>`

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

### Requirement: Object Store Static Credential Sources

The system SHALL accept S3-style static credentials (`access_key` + `secret_key`) from three sources in this precedence: (1) `MS_OBJECT_STORE_ACCESS_KEY` / `SECRET_KEY` environment variables (highest), (2) `[object_store].credentials_file` pointing to a key-value file with `access_key=...` and `secret_key=...` lines, (3) inline `[object_store].access_key` / `secret_key` in TOML (lowest). Cloud-native credential chains (IAM role, managed identity, workload identity) are NOT supported in this change.

#### Scenario: Environment variable wins over file
- **WHEN** both env vars and `credentials_file` are set
- **THEN** the env var pair is used and a `object_store_credentials_source="env"` info log is emitted at startup

#### Scenario: Credentials file missing key fails fast
- **WHEN** `credentials_file` exists but lacks `secret_key`
- **THEN** `wire::build_state` returns `Err("object_store credentials_file missing secret_key")` and the process exits

### Requirement: Object Store Health Check

`wire::build_state` SHALL perform a startup probe (PUT → GET → DELETE a 128-byte object under `_health/<uuid>`); failure SHALL abort startup. The HTTP server SHALL additionally run a background probe every `object_store.health_probe_interval_secs` (default 30s); three consecutive failures SHALL flip `/api/v1/healthz` to `503 Service Unavailable` with body `{ "status": "degraded", "reason": "object store unreachable" }` while `/metrics` continues to serve.

#### Scenario: Startup probe failure aborts boot
- **WHEN** the configured bucket does not exist or credentials are wrong
- **THEN** `main()` returns an error before role subsystems start; the exit log includes the probe key and error

#### Scenario: Three consecutive runtime failures degrade health
- **WHEN** the background probe fails three times in a row
- **THEN** `/api/v1/healthz` returns `503` until the next successful probe; `/metrics` still returns `200`

### Requirement: Cross-Backend Object Store Metrics

The system SHALL export `object_store_operations_total{backend, op}`, `object_store_bytes_total{backend, op, direction}`, `object_store_errors_total{backend, op, reason}`, `object_store_op_duration_seconds_bucket{backend, op}`, and `object_store_health_check_duration_seconds_bucket{backend}` via the global `prometheus::Registry`. `backend` label values are `"local" | "s3" | "azure" | "gcs"`.

#### Scenario: Metrics reflect mixed backend
- **WHEN** the configured backend is `s3` and the process has performed 100 PUTs and 200 GETs
- **THEN** `/metrics` exposes `object_store_operations_total{backend="s3",op="put"} 100` and `..."get"} 200`

### Requirement: Compactor Failure Tolerance

When the compactor encounters a transient error during `ParquetFileMetaRepository::replace`, the merged-target object SHALL be deleted from the object store, the source files SHALL remain referenceable for the next compaction tick, and the failure SHALL be recorded with `compactor_failures_total{reason}`.

#### Scenario: replace() rolls back, target object cleaned up
- **WHEN** the merged parquet is uploaded successfully but the `replace` transaction fails (e.g., constraint violation, lock timeout)
- **THEN** the just-uploaded merged object is deleted from the object store, `compactor_failures_total{reason="replace_failed"}` increments by one, and the original source `ParquetFileMeta` rows remain intact and eligible for the next compaction attempt

#### Scenario: Object delete failure after successful replace
- **WHEN** `replace` commits but deleting one of the source objects fails (e.g., S3 transient 503)
- **THEN** the metadata swap is preserved (sources are already tombstoned), the orphaned source object is logged with key + reason, and a separate `retention` sweep on the next tick reattempts the deletion

## MODIFIED Requirements

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

### Requirement: Tantivy Inverted Index

The ingester SHALL build a Tantivy index per parquet file for any field marked `indexed = true`, serialize it as a tar+zstd archive, store it alongside the parquet in the object store at key `{object_key}.tantivy.tar.zst`, and the querier SHALL use it to skip files whose index proves no matches exist for a `MATCH(field, term)` predicate. Index extraction MUST stream into a temp dir whose lifetime is bound to the request; the opened `Index` MAY be cached via `caching::parquet_meta`-like LRU keyed by `object_key`.

#### Scenario: Index skips file with no match
- **WHEN** a SQL query includes `MATCH(message, 'panic')` and a candidate file's tantivy index contains no posting for `panic`
- **THEN** that file is excluded from the `ParquetExec` for that query and a `tantivy_pruned_files_total` counter increments

#### Scenario: Missing index falls back to full scan
- **WHEN** a `MATCH` predicate targets a field but the parquet has no companion Tantivy archive (e.g., field was not yet `indexed=true` when the file was written)
- **THEN** that file is kept in the candidate set and the `MATCH` is evaluated row-by-row by DataFusion, with `tantivy_missing_archive_total` incrementing

#### Scenario: Index archive size bounded
- **WHEN** the ingester writes a Tantivy archive for a parquet that is `S` bytes
- **THEN** the archive size MUST be less than `S * 0.20`; if it exceeds 20% the writer logs a warning with the field names and proceeds (no hard fail)
