## MODIFIED Requirements

### Requirement: In-Memory Buffer and Periodic Flush

The ingester SHALL keep accepted events in a per-stream in-memory Arrow buffer and SHALL detach the current buffer generation for a parquet flush only when its estimated raw size reaches `ingester.buffer_max_mb`, its oldest active row reaches `ingester.flush_interval_secs`, a prior detached generation requires retry, or a replay/drain caller explicitly forces a flush. A normal ingest notification by itself MUST NOT flush an under-size, under-age buffer. Flushes SHALL be atomic: parquet upload → Tantivy archive upload (when any field is `indexed=true`) → `ParquetFileMetaRepository::insert` → WAL truncation up to the detached generation's high-watermark sequence index, in that order; any failure aborts subsequent steps and the detached generation is retained for retry.

#### Scenario: Buffer flush produces parquet + ParquetFileMeta + Tantivy archive
- **WHEN** a buffer reaches its configured size threshold and at least one field has `indexed = true`
- **THEN** the system uploads the parquet and Tantivy archive, inserts the `ParquetFileMeta` row, and only then truncates WAL records covered by that detached generation

#### Scenario: Small write waits for maximum age
- **WHEN** a stream receives a small batch below `ingester.buffer_max_mb`
- **THEN** the write notification does not immediately create a parquet file
- **AND** the buffer becomes flushable when its oldest row reaches `ingester.flush_interval_secs`

#### Scenario: Flush failure retains data
- **WHEN** the object store `put` call fails on either parquet or Tantivy archive
- **THEN** the detached generation and WAL are retained, `ingester_flush_errors_total{step="…"}` increments, and the generation is eligible for retry without requiring another write

#### Scenario: ParquetFileMeta insert failure deletes orphan objects
- **WHEN** parquet and Tantivy archive both upload successfully but `ParquetFileMetaRepository::insert` fails
- **THEN** both objects are deleted from the object store, the detached generation is retained, and the original DB error is bubbled to the scheduler

## ADDED Requirements

### Requirement: Bounded And Ordered Ingester Flush Concurrency

The ingester SHALL execute at most `ingester.flush_parallelism` flushes concurrently across distinct stream keys and MUST execute no more than one flush at a time for any single `(org, stream_type, stream)` key. A later high-watermark for a stream MUST NOT be committed or used for WAL truncation before an earlier detached generation for that stream has completed successfully.

#### Scenario: Different streams flush concurrently
- **WHEN** four distinct streams are due and `ingester.flush_parallelism = 2`
- **THEN** no more than two parquet flushes run concurrently and all four are eventually attempted

#### Scenario: Same stream remains single-flight
- **WHEN** two callers concurrently request a flush for the same stream
- **THEN** the second caller waits for the first caller's complete parquet, metadata, and WAL sequence before it can detach or commit a later generation
