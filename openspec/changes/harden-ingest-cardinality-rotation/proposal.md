## Why

The ingester currently wakes the flush loop after every accepted batch but does not enforce the configured per-stream size threshold or flush parallelism, so bursts can create small Parquet files, serialize flushes globally, and amplify memory use. Metrics ingestion also turns every Prometheus label key into a schema column without structural bounds, allowing malformed or highly dimensional series to grow schemas and buffers before any admission decision.

## What Changes

- Enforce configurable structural limits for Prometheus metric labels before schema evolution, WAL append, or per-sample label cloning.
- Split accepted Prometheus samples into bounded ingest/WAL chunks so one oversized remote-write request cannot become a single unbounded Arrow generation.
- Rotate a stream buffer only when its size threshold or maximum age is reached, while still forcing pending data during replay and drain.
- Flush different streams with configured bounded concurrency while preserving FIFO, single-flight ordering within each stream.
- Publish low-cardinality metrics for rotation reasons, flush backlog, and structural label rejection.
- Keep raw metric semantics intact: labels are never silently folded into an overflow series.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `ingestion`: Make the documented size-or-age Parquet rotation policy effective, add bounded cross-stream flush concurrency, and preserve per-stream WAL ordering.
- `ingest-protocols`: Add structural Prometheus label limits and bounded remote-write chunking before the common ingestion pipeline.

## Impact

- Affects ingester configuration, Prometheus remote-write decoding, the Arrow buffer pool, the ingester flush worker, and ingester metrics.
- Adds configuration fields with backward-compatible defaults; no wire-format or database migration is required.
- Permanently invalid label shapes return a client error before any data is persisted. Capacity-based active-series admission remains a later change and is not introduced here.
