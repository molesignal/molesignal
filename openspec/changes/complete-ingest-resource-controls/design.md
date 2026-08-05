## Context

The first ingest hardening change added complete-request structural label validation, bounded Prometheus chunks, real size/age rotation, cross-stream flush concurrency, and per-stream single-flight ordering. Three resource gaps remain:

1. valid labels can still form an unbounded number of distinct active series;
2. per-stream limits do not bound aggregate Arrow memory while object-store writes are slow;
3. a fixed raw-byte threshold cannot produce a stable Parquet file-size distribution across compressible and incompressible streams.

The router consistently selects an ingester from the organization key for an ingest request, so one healthy ingester is the normal authority for an organization's Prometheus admission state. The design must keep the hot path database-free, never retain raw label values in the admission registry, reject before WAL whenever possible, and preserve replay progress even when a configured live-ingest budget is exceeded.

## Goals / Non-Goals

**Goals:**

- Bound new and active Prometheus series per metric, organization, and ingester process.
- Reject a complete remote-write request before any stream/schema/WAL mutation when series admission fails.
- Bound aggregate active plus detached-flush memory and reject before WAL append.
- Keep recovered durable WAL data flushable even when it exceeds the live-ingest memory limit.
- Converge raw rotation thresholds toward a configured encoded Parquet target.
- Expose only fixed-enum or `stream_type` metric labels.

**Non-Goals:**

- A cross-region strongly consistent series database or a PostgreSQL operation for every new series.
- Exact process RSS accounting for request bodies, allocator fragmentation, query caches, or Parquet encoder scratch space.
- Dropping labels, folding overflow series into `__other__`, or sampling away accepted metric series.
- Changing WAL, Parquet, or protobuf formats.

## Decisions

### 1. Track hashed series at the current ingester owner

A shared `PrometheusSeriesAdmission` is constructed during storage bootstrap and exposed through `AppState`. The remote-write adapter creates canonical identities from metric name plus sorted non-`__name__` labels and passes only fixed-size SHA-256-derived fingerprints to the registry. Raw metric names, label names, and label values are not retained.

Each organization has an independent lock and state containing:

- active fingerprint → metric fingerprint and last-seen monotonic time;
- per-metric active counts;
- an expiry heap with one record per active series;
- a fixed one-minute new-series counter.

Admission deduplicates the complete request, refreshes existing series, checks per-metric, per-organization, new-series-rate, and process caps, then inserts all new identities atomically for that organization. Rejections contain only a fixed reason and happen before per-sample cloning or `IngestService`.

The process cap uses an atomic reservation across organization locks. Idle expiry decrements both organization/metric counts and the process reservation. Router ownership makes this exact during a healthy node epoch; failover starts a fresh conservative epoch rather than adding a database round trip to every new series.

Alternative: PostgreSQL rows for every series. Rejected because the cardinality guard would turn the ingest hot path into a high-write metadata workload and make database availability a prerequisite for existing-series samples.

### 2. Reserve buffer memory before WAL append

`BufferPool` owns a process-wide atomic memory budget. `IngesterSink` serializes the batch once for WAL, reserves that payload size, and only then appends the WAL record. A rejected reservation returns `resource_exhausted` before durable mutation.

After WAL success, the reservation is committed to the current `RecordBuilder` generation. `finish_and_clear` transfers the accounted-byte total alongside the detached `RecordBatch`; it remains globally charged throughout Parquet upload, metadata insert, and WAL truncation. Failure restoration returns the same accounted bytes to the builder without double charging. Full flush success releases the reservation.

Replay uses a force-reservation path because already durable data must make progress even when it temporarily exceeds the live limit. New live ingest remains rejected until replay/flush releases enough memory.

Alternative: sum every buffer on each request. Rejected because it is O(number of streams), races detached generations, and still cannot atomically prevent concurrent over-admission.

### 3. Adapt raw thresholds from encoded feedback

The hard raw maximum remains `ingester.buffer_max_mb`. Adaptive rotation adds a minimum raw threshold, an encoded target, and an EWMA alpha. Each successful file provides:

`observed_ratio = parquet_size_bytes / estimated_raw_generation_bytes`

Per stream, the worker updates the EWMA and sets:

`next_raw_threshold = target_encoded_bytes / ewma_ratio`

The result is clamped to `[min_buffer_mb, buffer_max_mb]`. A new stream starts at the hard maximum, so the feature has no extra early-flush penalty before the first observation. Age and retry triggers are unchanged. Disabling adaptation makes the threshold exactly `buffer_max_mb`.

Alternative: use only the previous file's ratio. Rejected because one anomalous file would cause threshold oscillation.

### 4. Keep metrics bounded

Series rejection counters use only `reason ∈ {process_active, org_active, metric_active, new_series_rate}`. Memory rejections, reserved bytes, compression ratios, adaptive targets, rotations, and in-flight gauges use either no label or `stream_type`. No organization, metric, stream, or label identifier is emitted.

## Risks / Trade-offs

- **[Failover resets active-series history]** → The new owner immediately applies new-series and process caps; operators size per-node limits conservatively for the replica count.
- **[Hash collision undercounts series]** → Use a 128-bit prefix of SHA-256 over length-delimited canonical input; collision probability is negligible for configured caps.
- **[Serialized WAL bytes are an approximation of Arrow residency]** → The budget is deliberately conservative for repeated labels and includes detached generations; metrics allow tuning from production observations.
- **[An idle-expiry pass can process many entries]** → A min-heap processes only due series and holds one expiry record per active series, avoiding unbounded touch records.
- **[Adaptive targets react after one file]** → Start at the existing hard maximum and use EWMA smoothing plus min/max clamps.

## Migration Plan

1. Deploy with documented defaults and observe rejection, memory, ratio, and adaptive-target metrics.
2. Size process and organization series caps by the number of ingest owners and expected failover headroom.
3. Tune `max_buffer_memory_mb` below the container memory limit, leaving room for encoding and runtime overhead.
4. Roll back by disabling cardinality/adaptive controls or reverting the binary; no stored data migration is required.

## Open Questions

None for this change. A future control-plane feature may persist warm series summaries across planned ownership transfers, but it is not required for bounded operation.
