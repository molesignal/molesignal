## Why

Structural label limits and size/age rotation prevent single-request explosions, but they do not bound the combinatorial number of active metric series, total Arrow plus detached-flush memory, or the mismatch between raw buffer bytes and compressed Parquet size. A slow object store or a burst of novel label sets can therefore still exhaust an ingester before existing QPS or storage quotas react.

## What Changes

- Add Prometheus active-series admission with per-organization, per-metric, and process-wide caps, an organization new-series rate cap, and idle-series expiry.
- Run series admission for the complete remote-write request before stream creation, schema evolution, WAL append, or partial chunk persistence; rejected requests return `429`.
- Add process-wide buffer-memory reservation before WAL append for every ingester protocol. Reservations cover active builders and detached flush generations until the full flush transaction succeeds.
- Reject new batches with `429` when the memory budget is exhausted while allowing flush/replay to make forward progress.
- Adapt each stream's raw rotation threshold from an EWMA of observed Parquet encoded/raw size, clamped by configured minimum and hard maximum bounds.
- Add low-cardinality rejection, active-series, reserved-memory, compression-ratio, and adaptive-target metrics plus documented, validated configuration.

## Capabilities

### New Capabilities

- `ingest-resource-admission`: Active-series and process-memory admission contracts, including expiry, rejection semantics, and bounded observability.

### Modified Capabilities

- `ingest-protocols`: Prometheus remote-write gains complete-request active-series admission before persistence.
- `ingestion`: Parquet rotation becomes compression-adaptive and the buffer/WAL path gains memory reservations.

## Impact

- Affected code: ingester configuration, Prometheus HTTP adapter/state, Arrow `BufferPool`, `IngesterSink`, flush worker, bootstrap storage wiring, and ingester metrics.
- API behavior: structurally valid writes may now receive `429 Too Many Requests` for new-series or memory pressure; existing accepted payload formats are unchanged.
- Storage formats, WAL encoding, Parquet schema, and database schema remain unchanged.
- No new external dependency or per-series PostgreSQL hot-path operation is introduced.
