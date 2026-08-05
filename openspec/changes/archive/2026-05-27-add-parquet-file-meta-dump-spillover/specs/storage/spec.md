## ADDED Requirements

### Requirement: ParquetFileMeta Dump Spillover

When `[storage.parquet_file_meta_dump] enabled = true` (default), the system SHALL run a periodic worker that, for every `(org, stream, stream_type, date)` partition whose `date` is older than `today() - storage.parquet_file_meta_dump.cold_after_days` (default 30 days), serializes every live `ParquetFileMeta` row of that partition into a parquet file uploaded to `{org}/_parquet_file_meta_dump/{stream_type}/{stream}/{date}.parquet`, inserts an index row into the `parquet_file_meta_dump` table containing `(org, stream, stream_type, date, object_key, rows_in_dump, created_at)`, and deletes the corresponding rows from the main `parquet_file_meta` table. The dump-and-delete operation SHALL satisfy: (a) object upload happens before any database mutation; (b) `parquet_file_meta_dump` insert and `parquet_file_meta` delete run in a single database transaction; (c) failure at any step leaves the main table untouched and a retry on the next worker tick is safe. The worker SHALL respect `storage.parquet_file_meta_dump.max_partitions_per_tick` (default 100) and `storage.parquet_file_meta_dump.interval_secs` (default 3600).

#### Scenario: Cold partition gets dumped end-to-end
- **WHEN** a partition `(orgA, log_app, logs, 2026-01-15)` has `date < today - 30` and 12 live ParquetFileMeta rows
- **THEN** the worker uploads a single `orgA/_parquet_file_meta_dump/logs/log_app/2026-01-15.parquet` containing those 12 rows, inserts one `parquet_file_meta_dump` index row with `rows_in_dump = 12`, and deletes the 12 main-table rows in the same transaction

#### Scenario: Object upload fails — main table untouched
- **WHEN** the dump parquet upload to object_store fails (network error)
- **THEN** no `parquet_file_meta_dump` row is inserted, no `parquet_file_meta` row is deleted, `parquet_file_meta_dump_partitions_skipped_total{reason="error"}` increments by 1, and the partition stays eligible for the next tick

#### Scenario: Transaction failure after upload — re-dumpable next tick
- **WHEN** the upload succeeds but the `INSERT parquet_file_meta_dump + DELETE parquet_file_meta` transaction fails
- **THEN** the uploaded object remains in the bucket (becomes a transient orphan), no main table rows are deleted, and the next tick re-uploads the same content (overwriting the orphan); `parquet_file_meta_dump_partitions_skipped_total{reason="error"}` increments

#### Scenario: Worker honors max_partitions_per_tick
- **WHEN** 500 partitions are eligible and `max_partitions_per_tick = 100`
- **THEN** exactly 100 partitions are dumped this tick and the remaining 400 are dumped over the following ticks

#### Scenario: Worker disabled
- **WHEN** `[storage.parquet_file_meta_dump] enabled = false`
- **THEN** the worker does not run, no dumps are produced, and the main `parquet_file_meta` table is the sole source for `ParquetFileMetaRepository::find`

### Requirement: ParquetFileMeta Dump Query Path

`ParquetFileMetaRepository::find(time_range)` SHALL transparently merge results from the main `parquet_file_meta` table and any relevant `parquet_file_meta_dump` parquet files. When the requested `time_range.start` is older than `today() - cold_after_days`, the implementation SHALL: (1) query the main table for hot rows in range; (2) query `parquet_file_meta_dump` for index rows whose `(date)` overlaps the range; (3) download the referenced dump parquet objects (passing through the `parquet_meta` and `parquet_disk_cache` layers when enabled); (4) deserialize them into `Vec<ParquetFileMeta>`, filter by `time_range`, and merge with the hot results, deduplicated by `ParquetFileMeta.id` and sorted by `time_range.start`. The merged result SHALL be indistinguishable from what a pre-dump main-table query would have returned.

#### Scenario: Pure-hot query bypasses dump path
- **WHEN** `time_range.start >= today - cold_after_days`
- **THEN** only the main `parquet_file_meta` table is queried; no dump parquet downloads occur; `parquet_file_meta_dump_query_hits_total` is NOT incremented

#### Scenario: Cross-boundary query merges sources
- **WHEN** `time_range = (today - 45 days)..(today - 5 days)` and the partition `(today - 45 ... today - 30)` has been dumped
- **THEN** the implementation queries both the main table (for `today - 30 ... today - 5`) and dumps (for `today - 45 ... today - 30`), merges, dedups by `id`, and returns the full set in `time_range.start` order; `parquet_file_meta_dump_query_hits_total` is incremented

#### Scenario: Cold-only query loads only dumps
- **WHEN** `time_range = (today - 60 days)..(today - 35 days)` and the entire range has been dumped
- **THEN** the main-table query returns 0 rows; dump parquet files are downloaded and deserialized; results are returned in `time_range.start` order

#### Scenario: Duplicate id between hot and cold survives one row
- **WHEN** a transient duplicate exists (worker crashed between insert-dump and delete-main, then retried successfully on the next tick BUT a query lands in the brief window) so the same `ParquetFileMeta.id` appears in both main table and dump
- **THEN** the merge dedups by `id` and the query returns exactly one row for that id

#### Scenario: Dump parquet load latency tracked
- **WHEN** a cross-boundary query loads N dump parquets
- **THEN** each load duration is observed in the `parquet_file_meta_dump_query_load_seconds` histogram

### Requirement: ParquetFileMeta Dump Metrics

The system SHALL export Prometheus metrics for the dump subsystem: `parquet_file_meta_dump_partitions_written_total` (Counter), `parquet_file_meta_dump_rows_written_total` (Counter), `parquet_file_meta_dump_partitions_skipped_total` with a `reason` label (`empty | locked | error | duplicate_id`), `parquet_file_meta_dump_query_hits_total` (Counter), `parquet_file_meta_dump_query_load_seconds` (Histogram).

#### Scenario: Each successful dump increments two counters
- **WHEN** the worker dumps a partition with 50 ParquetFileMeta rows
- **THEN** `parquet_file_meta_dump_partitions_written_total += 1` and `parquet_file_meta_dump_rows_written_total += 50`

#### Scenario: Skip reasons are distinguishable
- **WHEN** the worker encounters an empty partition (already fully retention-deleted) and a transient PG error in the same tick
- **THEN** `parquet_file_meta_dump_partitions_skipped_total{reason="empty"}` and `parquet_file_meta_dump_partitions_skipped_total{reason="error"}` each increment by 1
