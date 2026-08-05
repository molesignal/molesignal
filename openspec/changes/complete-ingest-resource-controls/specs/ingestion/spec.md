## MODIFIED Requirements

### Requirement: In-Memory Buffer and Periodic Flush

The ingester SHALL keep accepted events in a per-stream in-memory Arrow buffer and SHALL detach the current buffer generation for a parquet flush only when its estimated raw size reaches that stream's current adaptive threshold, its oldest active row reaches `ingester.flush_interval_secs`, a prior detached generation requires retry, or a replay/drain caller explicitly forces a flush. A normal ingest notification by itself MUST NOT flush an under-size, under-age buffer. A new stream's raw threshold SHALL start at `ingester.buffer_max_mb`; after each successful Parquet file, the threshold SHALL be derived from an EWMA of encoded/raw size toward `ingester.rotation.target_file_size_mb` and clamped to `[ingester.rotation.min_buffer_mb, ingester.buffer_max_mb]`. Disabling adaptation SHALL keep the raw threshold fixed at `ingester.buffer_max_mb`. Flushes SHALL be atomic: parquet upload → Tantivy archive upload (when any field is `indexed=true`) → `ParquetFileMetaRepository::insert` → WAL truncation up to the detached generation's high-watermark sequence index, in that order; any failure aborts subsequent steps and the detached generation is retained for retry.

#### Scenario: Buffer flush produces parquet + ParquetFileMeta + Tantivy archive
- **WHEN** a buffer reaches its current size threshold and at least one field has `indexed = true`
- **THEN** the system uploads the parquet and Tantivy archive, inserts the `ParquetFileMeta` row, and only then truncates WAL records covered by that detached generation

#### Scenario: Small write waits for maximum age
- **WHEN** a stream receives a small batch below its current size threshold
- **THEN** the write notification does not immediately create a parquet file
- **AND** the buffer becomes flushable when its oldest row reaches `ingester.flush_interval_secs`

#### Scenario: Compressible stream raises its raw threshold
- **WHEN** a successful file is substantially smaller than its estimated raw generation and the encoded target would require more raw bytes
- **THEN** the next raw threshold increases toward the encoded target without exceeding `ingester.buffer_max_mb`

#### Scenario: Incompressible stream lowers its raw threshold
- **WHEN** a successful file's encoded/raw ratio would otherwise exceed the encoded target
- **THEN** the next raw threshold decreases without going below `ingester.rotation.min_buffer_mb`

#### Scenario: Flush failure retains data and memory reservation
- **WHEN** the object store `put` call fails on either parquet or Tantivy archive
- **THEN** the detached generation, its memory reservation, and WAL are retained, `ingester_flush_errors_total{step="…"}` increments, and the generation is eligible for retry without requiring another write

#### Scenario: ParquetFileMeta insert failure deletes orphan objects
- **WHEN** parquet and Tantivy archive both upload successfully but `ParquetFileMetaRepository::insert` fails
- **THEN** both objects are deleted from the object store, the detached generation and its memory reservation are retained, and the original DB error is bubbled to the scheduler
