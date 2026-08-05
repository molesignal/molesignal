# Release notes draft — `harden-wal-followups`

## ⚠️ Breaking durability behaviour change — WAL fsync now actually runs

Prior versions ignored `[wal].sync_interval_ms` and ran the WAL with `FsyncPolicy::None` (page cache only) despite the ingestion spec promising fsync-on-interval semantics. Crash durability was effectively *no stronger than `BufWriter::flush`* — power loss / kernel panic would lose any record written but not yet fsynced.

Starting this release, the WAL honors `[wal].flush_strategy` / `sync_level` / `batch_max_pending` / `batch_max_delay_ms` end-to-end.

### New defaults

```toml
[wal]
flush_strategy = "batch"          # was effectively "none"
sync_level = "data"               # = fdatasync
batch_max_pending = 64
batch_max_delay_ms = 50           # alias of legacy sync_interval_ms
```

Effect: `fdatasync` is now invoked at most every 50 ms or every 64 records (whichever comes first), per-segment.

Expected impact:

- p99 ingest latency: **< 5%** typical regression (the fsync is decoupled from the per-record write).
- Disk fsync errors are now surfaced via `wal_fsync_errors_total{kind}` — value should be zero in healthy environments.
- Crash durability significantly improved: at most a 50 ms / 64-record window can be lost on power loss.

### How to opt out (not recommended)

Operators who explicitly want the legacy "never fsync" behaviour can set:

```toml
[wal]
flush_strategy = "none"
sync_level = "none"
```

This restores byte-for-byte the old behaviour.

### How to harden further (high-durability deployments)

```toml
[wal]
flush_strategy = "every_write"
sync_level = "all"
```

This calls `sync_all` (data + metadata) plus the WAL directory parent `sync_all` on every record. Single-record p99 latency rises to ~ms level on rotational disks; only recommended for low-QPS workloads.

### Backward compatibility

The legacy field `[wal].sync_interval_ms = N` continues to be accepted via serde alias and is automatically mapped to `batch_max_delay_ms`. No TOML edits required for existing deployments — but the durability *behaviour* will change on next restart.

## New metrics

| Name | Type | Labels | Meaning |
|---|---|---|---|
| `wal_append_lock_wait_seconds` | Histogram | `stream_type` | per-key mutex wait time in `WalPool::append`. Buckets `[0.1ms, 1ms, 10ms, 100ms, 1s]`. |
| `wal_append_inflight` | IntGauge | `stream_type` | concurrent holders of the per-key WAL mutex. |
| `wal_fsync_errors_total` | Counter | `kind` (`batch_flush` / `every_write` / `segment_rotate`) | `sync_data` / `sync_all` failures by trigger path. |

Label cardinality on the first two is bounded at `|StreamType| = 4` — no per-org / per-stream-name explosion.

## Forward-compatible: raft term injection seam

`WalPool::new` now accepts `Arc<dyn TermSource>`. OSS injects `StaticTermSource(1)`. Future consensus integration will provide a `RaftTermSource` whose `current_term()` returns the raft node's current term, and wire-up will be a one-line swap at `crates/bootstrap/src/wire.rs` — `WalPool::new` / `SegmentWal::new` / WAL record header format remain unchanged.

## Upgrade checklist

1. **Read** the spec for `[wal].flush_strategy` defaults; decide whether your environment needs the explicit `flush_strategy = "none"` opt-out before deploy.
2. **Monitor** `wal_fsync_errors_total{kind=*}` on first 24h staging — should stay at zero.
3. **Monitor** `wal_append_lock_wait_seconds{stream_type}` p95 on a hot stream — if it persists above 5 ms, file follow-up "WAL per-key mutex bottleneck".
4. **Acknowledge** the p99 ingest latency curve baseline shift; share with on-call.
