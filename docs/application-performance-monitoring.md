# Application Performance Monitoring

MoleSignal APM derives bounded service, Transaction, dependency, backend-error,
and version aggregates from unique `CanonicalSpan` candidates after
trace-owner deduplication and before tail sampling. Sampled-out traces still
contribute to RED metrics; duplicate OTLP retries do not. APM projection is
non-blocking, and an APM failure does not change Trace ingest readiness.

## Data Model

- SERVER and CONSUMER spans contribute service and Transaction RED.
- CLIENT and PRODUCER spans contribute dependency RED.
- Missing-kind, parentless spans are a compatibility fallback for service RED.
- Latency uses mergeable fixed-boundary histogram schema `v1`; cross-bucket
  percentiles are computed after merging bucket counts.
- Owner-local minute buckets are absolute, sequenced snapshots. PostgreSQL
  conditionally replaces only newer `snapshot_seq` values.
- Closed minute buckets roll up idempotently to hourly buckets. Defaults retain
  minute data for 24 hours and hourly data for 30 days.
- Error fingerprints omit the representative message. Messages and stack
  frames are sanitized and bounded before persistence.

APM does not add a telemetry stream and does not retain request bodies, response
bodies, URL query values, headers, SQL statements, or SQL parameters.

## Prometheus Exemplars

Prometheus remote_write v1 `TimeSeries.exemplars` are retained beside their
owning metric series without becoming ordinary PromQL samples. Exemplar labels
such as `trace_id` and `span_id`, the observed value, and event timestamp remain
available through:

```text
GET|POST /api/v1/prometheus/api/v1/query_exemplars
```

The endpoint follows the Prometheus HTTP response shape and accepts a PromQL
expression plus `start` and `end` as epoch seconds or RFC3339. It applies the
expression's vector/matrix selector matchers, deduplicates identical
remote_write retries, and returns at most 10,000 unique Exemplars with a warning
when truncated. Metrics Explorer renders matching Exemplars on the active time
range; a marker carrying `trace_id` opens the existing Trace detail.

## API

All endpoints are under `/api/v1/apm`:

| Endpoint | Purpose |
| --- | --- |
| `GET /overview` | RED summary/trend, service health, impact rankings, recent versions |
| `GET /services` | Service catalog, instrumentation metadata, RED, stable cursor |
| `GET /services/{service}` | Consistent service workbench composition |
| `GET /transactions` | Transaction RED and total-time ranking |
| `GET /transactions/{transaction}` | Transaction RED trend, errors, versions, and signal links |
| `GET /dependencies` | Safe caller/dependency aggregate ranking |
| `GET /errors` | Sanitized backend error groups |
| `GET /errors/{fingerprint}` | Error trend, impact, stack, and bounded samples |
| `GET /versions/compare` | Baseline/candidate RED deltas and regression breakdown |
| `GET /health` | Tenant projection boundary, gaps, and non-readiness health |

Common query parameters are epoch-microsecond `from` and `to`,
`namespace`, `service`, `environment`, `version`, `resolution`
(`auto`, `minute`, or `hour`), `sort`, `direction`, `limit` (1–200), and
an opaque `cursor`. Transaction detail accepts `kind` to disambiguate a shared
name; version comparison additionally requires `baseline` and `candidate`.

Every response carries a consistent `meta` object with the effective range and
resolution, `projection_started_at`, `last_complete_bucket_at`,
`activation_boundary`, and `data_quality { partial, gaps,
overflow_dimensions }`. Clients must not treat a partial range as a complete
zero.

Organization callers require `streams.query`; system-scope callers require
`sys.telemetry.read`. Every lookup includes the active organization predicate,
and a cross-organization entity is returned as not found.

## Default Capacity Limits

| Setting | Default |
| --- | --- |
| Queue / flush | `65,536` facts; `5s`; at most `10,000` snapshots |
| Shutdown / late grace | `10s` drain; `5m` late grace |
| Service identities | 200 per organization/hour |
| Transactions | 32 per service/environment/hour |
| Dependencies / error groups | 16 each per service/environment/hour |
| Versions / instances | 16 / 256 per service/environment/hour |
| Evidence | 3 exemplars per bucket; 8 error samples per group |
| Version comparison | 1,000 service requests on each side |
| Query / retention | 30-day max range; 24h minute; 30d hourly |

Service overflow is rejected and recorded as a projection gap. Transaction and
dependency overflow maps to `__other__`; error overflow maps to one explicit
overflow group; version and instance overflow suppresses detail while retaining
the service total.

## Recorded Capacity Gate

The 2026-07-30 release benchmark used PostgreSQL 17.10. It measured 10,000
owner snapshots for each of the service, Transaction, dependency, and error
tables, with indexes included. The finalized 24-hour minute and 30-day hourly
defaults produced:

| Gate | Result | Budget |
| --- | ---: | ---: |
| Aggregate / enqueue p99 | `2.041µs` / `1.500µs` | `25µs` / `5µs` |
| Projection queue drops | `0%` | `<0.1%` |
| 10,000-row flush p95 (20 samples) | `452.343ms` | `500ms` |
| Overview p95 (10,000 owner rows, 20 samples) | `51.762ms` | `500ms` |
| Heavy-organization storage projection | `14.852 GiB` | `16 GiB` |
| 40,000-row hourly rollup | `621.681ms` | `60s` |

The deterministic rounded model in `scripts/apm_capacity_spike.mjs` projects
`4,838,400` hot rows, `9,360,000` rollup rows, and `14.91 GiB`. Bucket identity
uses a 32-byte PostgreSQL `BYTEA` hash; storing the equivalent 64-character hex
key exceeded the storage gate.

Repeatedly replacing every open snapshot can allocate reusable PostgreSQL pages
before autovacuum catches up. During staged rollout and steady-state operation,
monitor dead tuples and table/index size as well as live row counts; investigate
autovacuum lag before increasing cardinality or retention.

## Metrics And Health

Monitor:

- `molesignal_apm_facts_total{result=...}`
- `molesignal_apm_cardinality_total{reason=...}`
- `molesignal_apm_queue{resource="depth"|"capacity"}`
- `molesignal_apm_flushes_total{result=...}`
- `molesignal_apm_flush_duration_seconds`
- `molesignal_apm_rollups_total{result=...}`
- `molesignal_apm_rollup_rows_total{kind=...}`
- `molesignal_apm_lag_seconds{stage="projection"|"rollup"}`
- `molesignal_apm_health{component="projector"|"repository"|"rollup"}`
- `molesignal_apm_api_duration_seconds{endpoint=...}`

Alert when queue depth remains above 80%, any queue/flush/cardinality gap is
recorded, projection lag exceeds two complete buckets, repository/rollup health
is zero, or endpoint latency exceeds the capacity gate. `/api/v1/apm/health`
provides tenant context. APM degradation intentionally does not fail general
readiness.

## Rollout Runbook

APM has no enable or kill-switch setting. The projector starts automatically on
Trace candidate-owner roles, and the rollup worker starts automatically on the
alert-manager role. Deploy the registered migration before or with the matching
binary and keep the tables during a binary rollback.

### 1. Staged Deployment

Roll the binary through a small representative set of candidate-owner and
alert-manager instances first. Run fixed synthetic HTTP, RPC, messaging,
database, error, retry, and sampled-out traffic. Compare expected service
request/error counts and merged histogram percentiles. Require no unexplained
gaps, queue drops below 0.1%, healthy projector/repository metrics, and
query/flush latency within the recorded capacity gate.

### 2. Complete Deployment

Continue the normal rolling deployment after the staged criteria pass. Verify:

1. organization and system-scope permissions;
2. activation-boundary and partial-data UI states;
3. stable cursor pagination and version sample sufficiency;
4. `/rum/*` and `/services*` compatibility redirects;
5. hourly rollup count/histogram equivalence and expected row reduction.

Continue watching table/index size, hot/hourly row counts, overflow reasons,
queue saturation, lag, and endpoint P95.

### Binary Rollback

If user-visible data is misleading, stop the rollout and restore the previous
binary through the normal deployment mechanism. There is no configuration
switch to disable APM independently. Keep the migration and aggregate data so a
corrected binary can resume projection without a destructive rollback. Trace
and RUM ingest, Logs, Metrics, Traces, and Profiles remain available through the
restored release, and existing compatibility routes must remain active.

After rollback, record the affected range as partial, identify whether the
cause was queue capacity, cardinality, repository, flush, late data, or rollup,
and rerun staged validation before deploying the corrected release.
