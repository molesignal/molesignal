# Backend distributed tracing

MoleSignal pins backend instrumentation and the `CanonicalSpan` contract to
[OpenTelemetry Semantic Conventions 1.43.0](https://opentelemetry.io/docs/specs/semconv/).
Changing this version requires an explicit schema review, fixture update, and
round-trip compatibility test. The pin is also exposed as
`shared::trace_normalization::SEMCONV_VERSION`.

## Field contract

Standard OpenTelemetry fields keep their standard names and meanings. MoleSignal
does not redefine a standard key under a private name.

| Concern | Standard field | MoleSignal extension |
|---|---|---|
| Service identity | `service.namespace`, `service.name`, `service.version`, `service.instance.id` | `molesignal.execution.role`, `molesignal.node.id`, `molesignal.cluster.id` |
| HTTP | `http.request.method`, `http.route`, `http.response.status_code`, `server.address`, `server.port` | `molesignal.request.id`, `molesignal.auth.subject_id` |
| RPC | `rpc.system`, `rpc.service`, `rpc.method`, `rpc.grpc.status_code` | `molesignal.rpc.peer_role` |
| Database | `db.system.name`, `db.operation.name`, `db.collection.name`, `db.response.returned_rows` | `molesignal.db.query.fingerprint`, `molesignal.db.pool_wait_ms` |
| Object storage | `cloud.provider`, `server.address` | `molesignal.object.operation`, `molesignal.object.backend`, `molesignal.object.category`, `molesignal.object.key_fingerprint`, `molesignal.object.bytes`, `molesignal.cache.hit` |
| Sampling | OpenTelemetry trace flags are preserved but are not authoritative for local retention | `molesignal.sampling.reason`, `molesignal.sampling.policy_version`, `molesignal.trace.partial_reasons` |
| Async/streaming | OpenTelemetry Span Links and Events | `molesignal.queue.name`, `molesignal.retry.count`, `molesignal.stream.segment`, `molesignal.stream.messages` |

Private keys must start with `molesignal.`. They may contain opaque internal IDs,
bounded counts, sizes, durations, normalized operation names, or keyed
fingerprints. They must not contain credentials, cookies, request/response
bodies, query strings, email/name data, SQL values, complete object keys,
License packages/signatures, prompts, model output, or tool arguments/results.

## Canonical storage

All public OTLP and process-local spans normalize to `CanonicalSpan` before
sampling or persistence. Each row contains the complete bounded Resource,
Instrumentation Scope, attributes, Events, Links, dropped counts, schema and
semantic-convention versions, sampling reason, and partial diagnostics. Common
query dimensions such as `trace_id`, `span_id`, `service.name`, `name`, timing,
and status are also materialized as top-level columns.

The current schema version is `1`. This development-stage contract intentionally
has no legacy-row reader, compatibility alias, or historical backfill. A fresh
`_sys/traces/_molesignal` stream is initialized from the canonical fixture and
then evolves only within the current contract.

## Runtime architecture

Every HTTP, Tonic, OTLP-gRPC, and Flight SQL server installs one global context
boundary. The boundary validates or creates `X-Request-Id`, extracts W3C
`traceparent`/`tracestate`, creates the server Span, and returns
`X-Request-Id`/`X-Trace-Id`. The CORS layer exposes both response headers.
Malformed inbound context is replaced rather than rejected.

Finished application Spans become bounded `CanonicalSpan` candidates. The
candidate router selects exactly one healthy tail-sampler owner with rendezvous
hashing on `trace_id`; it does not reuse stream/WAL sharding. Delivery is a
bounded, authenticated cluster RPC and is fail-open for the business request.
There is no Trace WAL or owner replication, so owner failure can lose the
current decision window and increments the unresolved-loss metric.

The owner assembles a Trace in memory and binds it to the policy version active
when its first Span arrived. A decision happens after the 30-second default
window, one second after the root ends, or early under capacity pressure. Every
retained Trace then enters one fan-out stage:

```text
instrumentation -> candidate queue -> rendezvous owner -> tail sampler
                                                     -> self-ingest queue
                                                     -> external OTLP queue
```

The two sink queues have independent batching, timeout, retry, drop, and health
state. Failure of either sink never blocks the other sink or the application.
Self-ingest is recursion-suppressed and writes to `_sys/traces/_molesignal`.

## Span catalog

Span names and dimensions are intentionally low-cardinality:

| Area | Main spans | Safe dimensions |
|---|---|---|
| HTTP | `http.server`, `http.client` | method, route template, status, sanitized host/port |
| gRPC and Flight | `rpc.server`, `rpc.client`, `stream.session` | service, method, status, bounded segment/message counts |
| SQL | `db.transaction`, `db.query` | operation, normalized collection, keyed fingerprint, rows |
| Object storage | `object_store.operation`, multipart session | backend, operation, object category, bytes, retry Events |
| Ingestion | `ingest.batch`, pipeline/mask/WAL/buffer/flush stages | protocol, stream type, counts and bytes |
| Query | planning/DataFusion/PromQL/distributed/shard spans | language, stage, bounded shard role, row/byte counts |
| Background | compaction, reports, search jobs, service graph, replay, sync | static worker/operation names and result counts |
| Intelligence | provider/tool stages and linked stream segments | provider, model, stage, tool name, token counts |

Parallel shards are sibling spans. Retry attempts and multipart parts are
bounded Events on one logical Span. SSE, streaming HTTP, AI provider streams,
gRPC, and Flight streams use a handshake Span plus linked independent session
roots, rolling over every 30 seconds or 1,000 messages. Cancellation, terminal
error, duration, and message count are recorded; message content is not.

Queued or delayed work serializes only a bounded `Trace Link`. A consumer starts
a new root and links it to the producer; it never treats the link, Baggage, or
an `org.id` hint as authorization. Short-lived in-process tasks opt into the
task-local context helper.

## Context trust

External sampled flags are diagnostic hints only and cannot force retention.
External `org.id` Baggage is discarded; authentication overwrites it with the
opaque authorized organization ID. Only `org.id` and `request.id` may be sent as
Baggage, and only to explicitly internal targets. Third-party HTTP/gRPC targets
receive Trace Context but no internal Baggage, force marker, or debug token.

Trusted cluster force markers and short-lived debug tokens can force retention.
Debug tokens are system-scoped, bounded by expiration and use count, stored only
as digests, and audited on issue/use/revoke.

## Tail-sampling policy

Decision order is fixed:

1. deployment force-disable or runtime disable;
2. trusted internal/debug force;
3. any error Span;
4. any slow child or root Span;
5. ordered explicit rules;
6. deterministic normal ratio.

Production defaults to a 10% normal ratio; development/test defaults to 100%.
Slow thresholds are typed for HTTP, query, batch ingestion, database, object
storage, external calls, and background work. Under pressure, ordinary traces
are decided first while observed error/slow traces remain preferred. Late Spans
reuse the decision cache and can never resurrect a dropped Trace. Identical
duplicates and conflicting `(trace_id, span_id)` payloads have separate
diagnostics.

Each Span defaults to 128 attributes, 128 Events, 128 Links, and 4 KiB per
string. Each Trace defaults to 1,000 Spans. High-fanout overflow keeps error and
slow Spans first, emits an aggregate Span, and marks `span_limit`.

Capacity planning should budget approximately:

```text
tail memory ~= concurrent unresolved traces * average canonical bytes per trace
sink memory ~= queue capacity * average retained trace bytes
```

Set `tail_max_traces` and `tail_memory_bytes` together. An 80% queue/tail-cache
occupancy is the default alert threshold.

## Privacy boundary

One recursive sanitizer is shared by Span normalization, process logs, and
audit persistence. It removes forbidden nested keys and replaces credential,
email, private-key, and complete-URL patterns before enqueue. The Trace sink
performs a second non-mutating invariant check before self-ingest or external
export. SQL values, raw paths/query strings, full object keys, notification
recipients/content, License packages/signatures, prompts/responses, and Tool
arguments/results are never attributes.

`MS_TRACE_FINGERPRINT_KEY` may enable HMAC-SHA-256 fingerprints for SQL shapes
or object keys. It must contain at least 16 bytes, is read only from the process
environment, and is never returned in APIs, config diffs, logs, or Spans. There
is no unkeyed fallback.

## Metrics, health, and alerts

Prometheus families use only closed labels such as stage, result, reason, sink,
queue, and component:

- `molesignal_trace_spans_total`
- `molesignal_trace_decisions_total`
- `molesignal_trace_retries_total`
- `molesignal_trace_export_batches_total`
- `molesignal_trace_queue_depth` / `molesignal_trace_queue_capacity`
- `molesignal_trace_tail_cache`
- `molesignal_trace_system_load_status`
- `molesignal_trace_latency_seconds`

No Trace ID, organization ID, raw route, host path, or object key is a metric
label. `GET /api/v1/system/telemetry` returns detailed sampler/router/sink
health and the four default alert definitions: sustained exporter failure,
queue or tail-cache occupancy above 80%, observed drops above 1%, and `_sys`,
License, or dynamic-policy load failure. These conditions report `degraded` in
the detailed system view but do not fail otherwise healthy `/healthz` or
`/readyz`.

## `_sys`, platform administration, and License

`_sys` is a single permanent system organization. Its typed `_molesignal`
streams are system-owned and protected by domain, repository, and PostgreSQL
guards. They cannot be renamed, moved, deleted, assigned memberships, or
mutated through public ingest/stream APIs. Per-signal retention remains an
approved capacity update; Trace retention defaults to seven days.

The configured root user is the only platform administrator. Startup
reconciles that identity under a database uniqueness constraint, so the root
assignment cannot be granted or revoked through an API. Root can discover and
select `_sys` plus every enabled tenant organization, including organizations
created later. Root receives every current permission in the database catalog;
ordinary users remain bound to their organization Membership and role grants.

Only a system-scoped interactive root JWT (maximum one hour) can discover or
select `_sys` and call the APIs below. Tenant JWTs and `ms_*` API tokens receive
`404`, without leaking system metadata. Ordinary organization mutation, public
ingest, remote profiling, and `ms_*` API-token creation remain unavailable in
system scope.

- `GET /api/v1/system/platform-admins`
- `GET/PUT /api/v1/system/telemetry`
- `GET /api/v1/system/telemetry/policies`
- `GET/POST /api/v1/system/telemetry/debug-tokens`
- `DELETE /api/v1/system/telemetry/debug-tokens/{id}`
- `GET /api/v1/system/license`
- `GET/POST /api/v1/system/license/versions`
- `POST /api/v1/system/license/versions/{id}/activate`

License versions are immutable, and activation changes a single transactional
pointer before replacing the process `LicenseHolder`. Startup re-verifies the
active package and expiration. An invalid active version degrades to Community
and raises system-load health; environment/file import is allowed only for
explicit first bootstrap or disaster fallback. The old `/api/v1/license` route
does not exist.

## Configuration and failure semantics

Trace instrumentation and self-ingest are enabled by code default. Effective
enablement precedence is:

1. deployment `telemetry.trace.force_disabled`;
2. persisted `_sys` runtime policy;
3. code/config default.

`telemetry.trace.filter` is independent of `RUST_LOG` and
`telemetry.log_level`. Runtime APIs can change enablement, ratios, ordered
rules, thresholds, decision windows, and soft limits atomically for new Traces.
Exporter endpoint, protocol (`grpc` or `http/protobuf`), TLS/mTLS, compression,
Resource identity, and authentication remain static deployment settings and
require restart. Sensitive exporter headers must be `env:` or `secret:`
references.

Invalid static exporter/security configuration fails startup. Runtime collector
failure, owner loss, full queues, sanitizer rejection, and shutdown timeout are
fail-open for business traffic and visible in bounded metrics/health. Graceful
shutdown stops candidates, resolves the tail cache, drains both sink queues for
up to ten seconds, records residue on timeout, then proceeds to normal ingestion
drain.

## Validation and performance gates

The standalone acceptance path starts PostgreSQL with testcontainers and follows
one correlated request through HTTP, a business Span, SQL, object storage,
self-ingest, and `_sys` trace queries. It also exercises
system-scope switching, tenant-facing `404` boundaries, permanent system
resources, immutable License history, and final-platform-administrator
protection:

```text
MS_RUN_IT=1 cargo test --test bootstrap_it_distributed_tracing
```

The release performance gate measures a 100,000-event ingestion batch, 100,000
PromQL samples, and an 8 MiB object-store put/get with default Trace capture
disabled and enabled in alternating order. It fails above 5% process CPU
overhead or 3% P95 wall-latency overhead:

```text
scripts/check_distributed_tracing_overhead.sh
MS_TRACE_PERF_SAMPLES=80 scripts/check_distributed_tracing_overhead.sh
```

Run this gate on an otherwise idle, CPU-pinned release runner. The ordinary test
suite keeps it ignored because shared debug runners cannot produce reliable
percentage comparisons. Environment-gated results and exclusions are recorded
in `openspec/changes/add-backend-distributed-tracing/validation.md`.

## One-release rollout

Ship the schema, propagation, sampler, sinks, system scope, and License changes
together. For production:

1. deploy with `telemetry.trace.force_disabled = true`;
2. verify `_sys`, License/policy load, metrics, and CORS correlation headers;
3. remove force-disable on a small canary set;
4. validate privacy, queue occupancy, drop rate, sampling decisions, and both
   sink retained sets;
5. enable through the persisted `_sys` policy for the remaining instances;
6. roll back immediately with the deployment force-disable if a gate regresses.
