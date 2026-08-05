## MODIFIED Requirements

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

`ParquetFileMetaRepository::find(time_range)` SHALL transparently merge results from the main `parquet_file_meta` table and any relevant `parquet_file_meta_dump` parquet files. When the requested `time_range.start` is older than `today() - cold_after_days`, the implementation SHALL: (1) query the main table for hot rows in range; (2) query `parquet_file_meta_dump` for index rows whose `deleted = false AND time_end_micros >= time_range.start AND time_start_micros <= time_range.end`; (3) for every dump object load it through `read_dump_filtered(store, object_key, time_range)` which SHALL (3a) consult an in-process LRU cache keyed by `(org_id, stream, stream_type, partition_level, partition_key)` returning `Arc<Vec<ParquetFileMeta>>` on hit; (3b) on miss, download via `ProductionObjectStore` (passing through `parquet_meta` and `parquet_disk_cache` layers), open the parquet with an Arrow predicate `time_end_micros >= time_range.start AND time_start_micros <= time_range.end` registered as a row filter, decode columns directly into `ParquetFileMeta` (no JSON deserialization of the row), write the unfiltered `Arc<Vec<ParquetFileMeta>>` into the cache; (4) merge with the hot results, deduplicate by `ParquetFileMeta.id`, and sort by `time_range.start`. The merged result SHALL be indistinguishable from what a pre-dump main-table query would have returned. The query path SHALL NOT attempt to read any object whose corresponding `parquet_file_meta_dump` row has `deleted = true`.

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

## ADDED Requirements

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
