## Context

MoleSignal stores accepted events in one Arrow builder per `(org, stream_type, stream)` and assigns one WAL sequence number to each `IngestBatch`. The current worker detaches a builder before object-store I/O, which already allows new writes to continue, but every successful write wakes the scheduler and every wake flushes every non-empty buffer. The configured `buffer_max_mb` and `flush_parallelism` therefore do not control runtime behavior.

Prometheus remote write currently decodes a complete request, maps `__name__` to a stream, clones all remaining labels into every sample, and only then enters schema evolution and persistence. Duplicate, oversized, reserved, or excessively numerous labels have no protocol-boundary validation.

The change must preserve WAL high-watermark ordering, per-organization isolation, raw metric label semantics, and the existing upload → `ParquetFileMeta` → WAL truncation sequence. Existing production files that already approach the repository's size limit must not acquire unrelated responsibilities.

## Goals / Non-Goals

**Goals:**

- Make size-or-age rotation and configured cross-stream flush parallelism effective.
- Keep at most one flush in flight for a stream, including direct/test calls outside the scheduler.
- Bound the number of samples represented by one Prometheus `IngestBatch` and WAL sequence.
- Reject structurally unsafe Prometheus labels before schema evolution or persistence.
- Add diagnostics whose metric label sets remain statically bounded.

**Non-Goals:**

- Active-series or per-label-value cardinality tracking.
- Distributed cardinality state, failover handoff, relabeling, or `__other__` folding.
- A global ingester memory admission controller, WAL-only spill mode, adaptive compression targets, or event-time partition splitting.
- Changing the Prometheus remote-write protobuf or the internal gRPC protocol.

## Decisions

### 1. Put protocol limits under `[ingester.prometheus]`

`IngesterSettings` gains a nested `PrometheusIngestSettings` with defaults for label count, label-name bytes, label-value bytes, and maximum samples per internal batch. These limits are validated as non-zero during settings validation.

This keeps deployment tuning with the ingester that owns schema/WAL state and avoids adding protocol settings to every `AppState` fixture. The HTTP adapter reads the already initialized process settings; pure conversion helpers accept explicit settings so unit tests remain deterministic.

Alternative: hard-coded constants. Rejected because safe limits vary by deployment and cannot be rolled out gradually.

### 2. Preflight the complete remote-write request, then ingest bounded chunks

The adapter first scans every `TimeSeries` without cloning sample fields and validates:

- exactly one non-empty `__name__`;
- no duplicate label names;
- no empty or reserved storage column names;
- configured count and byte limits.

Only after the request passes preflight does it consume samples. It maintains one partial chunk per metric and submits a chunk whenever it reaches `max_samples_per_batch`; residual chunks are submitted at the end. Each submitted chunk becomes an independent `IngestBatch` and therefore receives an independent WAL sequence.

This ordering prevents a permanently invalid label late in a request from causing earlier chunks to be persisted. Chunking before the WAL boundary also avoids the unsafe alternative of splitting one WAL sequence across independently committed Parquet files.

Alternative: rotate the Arrow builder while pushing one oversized `IngestBatch`. Rejected because the current WAL truncation unit is the batch sequence; independently truncating one fragment could remove WAL protection for unflushed fragments carrying the same sequence.

### 3. Separate due checks from forced flushes

`RecordBuilder` records a monotonic first-write instant and exposes a due decision containing a bounded reason:

- `size` when estimated bytes reach `buffer_max_mb`;
- `age` when the oldest active row reaches `flush_interval_secs`;
- `retry` when a previously failed detached batch is pending.

`flush_one` remains the forced primitive used by startup replay, drain, and focused tests. The steady-state loop calls a due-aware wrapper. A successful detach clears the active generation's age; concurrent writes establish a new age. A restored failed batch remains immediately retryable on the next scheduler opportunity.

The write path wakes the scheduler only when the just-written stream is due. The periodic timer remains the maximum-age trigger, eliminating full-pool scans after every small write.

### 4. Parallelize across keys and serialize within a key

Each scheduler pass processes distinct buffer keys with `flush_parallelism` bounded concurrency. A per-key async mutex wraps the complete detach/upload/metadata/truncation sequence, so direct concurrent calls cannot let a later high-watermark commit before an earlier one.

The scheduler itself does not overlap passes; notifications received during a pass are coalesced and handled by the next pass. Concurrency is therefore bounded without an unbounded task or channel.

Alternative: a global semaphore without a per-key lock. Rejected because external/direct invocations could still overlap the same stream and violate WAL truncation order.

### 5. Keep observability low-cardinality

Rotation counters use only `{stream_type, reason}` where reason is a fixed enum. Flush in-flight gauges use only `stream_type`. Prometheus structural rejection counters use only a fixed rejection reason. Organization IDs, metric names, stream names, and label names are excluded.

## Risks / Trade-offs

- **[One remote-write request can still be large while protobuf-decoded]** → Chunking prevents a second full `RawEvent` copy and bounds WAL/Arrow generations; HTTP/decompressed-body streaming is deferred.
- **[Many metric names retain one partial chunk each]** → Existing stream quotas and a later new-stream admission change are still required; this change bounds each chunk but not metric-name cardinality.
- **[Size estimation differs from Arrow and compressed Parquet bytes]** → Treat `buffer_max_mb` as a raw soft threshold and retain a hard protocol chunk bound; adaptive encoded-size feedback remains future work.
- **[Object-store slowdown can accumulate active data while a stream flushes]** → Per-stream ordering remains correct, but global memory backpressure is explicitly deferred and must be added before claiming a hard process memory bound.
- **[Stricter label validation rejects payloads previously accepted with silent overwrites]** → Defaults are documented, errors are returned before persistence, and rejection reasons are observable.

## Migration Plan

1. Deploy with backward-compatible defaults and observe label rejection and rotation metrics.
2. Confirm Parquet file-size distribution, flush latency, and WAL growth before lowering limits.
3. Roll back by reverting the binary/config additions; existing WAL and Parquet formats remain compatible.

## Open Questions

- Active-series caps and new-series rate limits require capacity benchmarks against the in-memory PromQL grouping path and will be proposed separately.
- A global memory reservation layer should decide whether overload returns 429 before WAL or introduces a durable WAL-consumer mode.
