## MODIFIED Requirements

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

## ADDED Requirements

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
