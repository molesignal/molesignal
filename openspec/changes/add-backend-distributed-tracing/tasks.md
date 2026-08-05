## 1. Baseline and Contract Setup

- [x] 1.1 Complete and record the remaining validation for `ingest-self-telemetry`; make its internal-ingest, suppression, queue, and shutdown behavior the tested prerequisite for this change.
- [x] 1.2 Pin the OpenTelemetry Semantic Conventions version used by the backend and document the mapping between standard fields and `molesignal.*` custom fields.
- [x] 1.3 Add a Trace test fixture package containing canonical HTTP, gRPC, SQL, object-store, async Link, streaming, error, slow, duplicate, and high-fanout traces.
- [x] 1.4 Remove any assumption that development-stage legacy self-trace rows must be read or migrated; make fresh CanonicalSpan schema initialization deterministic.

## 2. `_sys` System Organization and Permanent Resource Protection

- [x] 2.1 Add database fields/constraints needed to identify the single system organization and system-owned streams.
- [x] 2.2 Add an idempotent bootstrap transaction that creates or validates `Organization { name: "_sys", slug: "_sys", system: true }` on every startup.
- [x] 2.3 Add domain validation that forbids `_sys` rename, slug change, deletion, Membership, team membership, and ordinary organization mutation.
- [x] 2.4 Add Repository guards for every organization mutation path so `_sys` cannot be changed or deleted before SQL is issued.
- [x] 2.5 Add PostgreSQL triggers/constraints that prevent the application database role from renaming or deleting `_sys`.
- [x] 2.6 Update `_molesignal` typed-stream creation to target `_sys` and mark each stream system-owned.
- [x] 2.7 Add domain and Repository guards that permanently forbid `_molesignal` rename, org reassignment, system-marker removal, schema replacement from public APIs, and deletion.
- [x] 2.8 Add PostgreSQL protection for all typed `_sys/_molesignal` streams while permitting retention and approved capacity-property updates.
- [x] 2.9 Add focused unit/integration tests for concurrent bootstrap, tampered system identity, every forbidden mutation path, and permitted retention updates.

## 3. Platform Administrator and System Scope

- [x] 3.1 Add persistent platform-administrator assignments and repositories independent of organization Membership/Role.
- [x] 3.2 Bootstrap the configured root user as the first platform administrator only when no assignment exists.
- [x] 3.3 Enforce transactionally that the final active platform administrator cannot be revoked, deleted, disabled, or otherwise made unusable.
- [x] 3.4 Extend JWT claims with explicit ordinary/system scope and resolve fine-grained platform capabilities into `IamContext` server-side.
- [x] 3.5 Implement `_sys` selection without Membership and issue system-scoped JWTs with a maximum one-hour lifetime.
- [x] 3.6 Update organization list/get/search/select behavior so only platform administrators can discover and select `_sys`.
- [x] 3.7 Add `SystemTelemetryRead`, `SystemTelemetryManage`, `LicenseRead`, `LicenseWrite`, `PlatformAdminManage`, and `TraceDebug` authorization checks.
- [x] 3.8 Add `/api/v1/system/platform-admins` list/grant/revoke endpoints and hide them with `404` from non-system scopes.
- [x] 3.9 Ensure `ms_*` API tokens and ordinary organization JWTs can never obtain or exercise platform permissions.
- [x] 3.10 Add authentication/authorization tests for system switching, expiry, hidden discovery, absent Membership, last-admin protection, and tenant-token denial.
- [x] 3.11 Materialize the `_sys` platform role and load its display name and permissions from the IAM database without JWT or application-role compatibility fields.

## 4. System-Scoped License Persistence and Management

- [x] 4.1 Add immutable License-version storage under `_sys` and a transactional single active-version pointer.
- [x] 4.2 Add Repository/database protection that prevents editing or deleting any License version.
- [x] 4.3 Load and re-verify the active persisted License at startup; fall back to Community with high-priority degraded health on invalid/corrupt data.
- [x] 4.4 Implement explicit first-bootstrap and opt-in disaster fallback from the configured file/environment License source.
- [x] 4.5 Implement atomic upload, verification, activation, historical reactivation, and runtime `LicenseHolder` replacement.
- [x] 4.6 Move License APIs to `/api/v1/system/license*` with `LicenseRead/LicenseWrite`; remove the ordinary `/api/v1/license` route without an alias.
- [x] 4.7 Return `404` and no License metadata for all tenant-scoped or otherwise unauthorized requests.
- [x] 4.8 Add tests for persistence across restart, valid/invalid signature, expired version, immutable history, transactional activation, Community degradation, system-only visibility, and rollback.

## 5. CanonicalSpan Model and Trace Storage Contract

- [x] 5.1 Define CanonicalSpan with IDs/flags/state, timing/status/kind, Resource, Instrumentation Scope, attributes, Events, Links, dropped counts, schema version, sampling reason, and partial metadata.
- [x] 5.2 Refactor public OTLP trace conversion and internal finished-span conversion to use the same CanonicalSpan adapter.
- [x] 5.3 Preserve CanonicalSpan nested structures through internal RPC, WAL, Parquet, query reconstruction, and OTLP re-export.
- [x] 5.4 Add configurable per-Span limits for attributes, Events, Links, and strings; preserve status/error/semantic fields before lower-priority fields.
- [x] 5.5 Add the configurable per-Trace Span cap, keep error/slow spans first, aggregate excess operations, and mark partial reason `span_limit`.
- [x] 5.6 Add `(org_id, trace_id, span_id)` deduplication with configurable decision cache and separate identical-duplicate/conflict diagnostics.
- [x] 5.7 Extend Trace query responses with Links, Events, Scope, dropped counts, sampling reason, partial/truncation reasons, and late/conflict diagnostics.
- [x] 5.8 Add round-trip fixtures proving public OTLP and internal spans produce equivalent canonical rows and preserve all supported OTLP fields.

## 6. Context Propagation and Request Correlation

- [x] 6.1 Install W3C Trace Context extraction/injection for Axum HTTP while safely replacing malformed context.
- [x] 6.2 Add equivalent propagation interceptors for Tonic gRPC, Arrow Flight, cluster RPC, and federation calls.
- [x] 6.3 Implement the Baggage whitelist (`org.id`, `request.id`), override org from authentication, and regenerate invalid request IDs.
- [x] 6.4 Add internal-target allowlisting so third-party calls retain Trace Context but strip internal Baggage.
- [x] 6.5 Prevent untrusted inbound sampled flags from forcing retention; add trusted internal/debug-force markers with rate limits.
- [x] 6.6 Add `trace_id/span_id` to correlated logs without automatically copying ordinary logs into Span Events.
- [x] 6.7 Echo validated `X-Request-Id` and active `X-Trace-Id` in HTTP responses and add Trace ID metadata to gRPC errors.
- [x] 6.8 Update applicable CORS response-header exposure and add HTTP/gRPC propagation, spoofing, malformed-context, and third-party-stripping tests.

## 7. Distributed Tail Sampler and Cluster Ownership

- [x] 7.1 Implement trace-ID consistent/rendezvous owner selection independent of existing `(org_id, stream)` WAL sharding.
- [x] 7.2 Add a cluster-authenticated bounded CanonicalSpan candidate RPC that preserves producer Resource identity and rejects public callers.
- [x] 7.3 Implement the bounded in-memory Trace assembly table, configurable 30-second decision window, root-end one-second grace, and decision cache.
- [x] 7.4 Implement ordered decisions for force-keep, ERROR, typed/route slow thresholds, explicit rules, and deterministic default ratio.
- [x] 7.5 Add the agreed default thresholds and environment defaults (production normal 10%, development/test 100%) with validation.
- [x] 7.6 Bind each unresolved Trace to one immutable policy version while allowing new policies to take effect atomically for new Traces.
- [x] 7.7 Implement pressure degradation that prioritizes observed error/slow traces, decides normal traces early, never blocks producers, and records every reason.
- [x] 7.8 Implement late-span behavior that reuses cached decisions and never resurrects a dropped Trace after the decision boundary.
- [x] 7.9 Implement owner-change/no-owner retry and drop behavior without replication or Trace WAL; expose estimated unresolved loss.
- [x] 7.10 Add deterministic unit/property tests and split-role integration tests for affinity, ordering, pressure, owner churn, late spans, crashes, and policy updates.

## 8. Unified Trace Fan-Out and External OTLP

- [x] 8.1 Route every retained Trace through one fan-out stage so self-ingest and external OTLP share sampling reason and retained set.
- [x] 8.2 Give self-ingest and external OTLP independent bounded queues, batches, timeouts, retries, drop counters, and health state.
- [x] 8.3 Add explicit OTLP `grpc` and `http/protobuf` protocol configuration, defaulting to gRPC and never guessing from URL.
- [x] 8.4 Add custom headers/metadata, gzip, custom CA, optional mTLS, and environment/secret-reference credential resolution.
- [x] 8.5 Validate all static exporter/security settings at startup and keep runtime collector failures fail-open.
- [x] 8.6 Detect external endpoints that target the same MoleSignal cluster; reject by default and support audited `allow_self_export` with suppression/deduplication.
- [x] 8.7 Add bounded in-memory retry/backoff with no local disk queue and independent failure tests for both sinks.
- [x] 8.8 Add graceful shutdown ordering that stops candidates, resolves/flushes traces, waits up to ten seconds, records residue, and then starts normal ingestion drain.
- [x] 8.9 Add mock gRPC and HTTP/protobuf collector integration tests for auth, TLS/mTLS, batch export, matching sampled sets, failure isolation, loop rejection, and shutdown.

## 9. Trace Configuration, Health, and Audit

- [x] 9.1 Add a dedicated `trace.filter` independent from `RUST_LOG` and `telemetry.log_level`, including per-layer filtering tests.
- [x] 9.2 Make Trace instrumentation code-default enabled while routing Trace self-ingest through the shared `telemetry.self_collect.enabled` opt-in, preserving the Metrics sub-switch and independent external OTLP configuration.
- [x] 9.3 Add the deployment-level non-overridable Trace force-disable and effective-precedence reporting.
- [x] 9.4 Persist runtime enablement, sampling rules, thresholds, decision settings, and soft capacity limits under `_sys`.
- [x] 9.5 Add `/api/v1/system/telemetry` read/update APIs protected by `SystemTelemetryRead/Manage` and atomic policy publication.
- [x] 9.6 Split self-telemetry retention by signal with Trace defaulting to seven days and update `_molesignal` retention idempotently.
- [x] 9.7 Register bounded-cardinality metrics for generation, sampling, dedupe, late/partial traces, tail cache, decisions, queues, retries, exports, drops, and latency.
- [x] 9.8 Add detailed degraded health without changing otherwise-successful liveness/readiness and add default alert-rule definitions for exporter failure, >80% occupancy, >1% drops, and system-load failures.
- [x] 9.9 Audit platform-role changes, system-scope issuance, License actions, Trace policy changes, and debug-token lifecycle/use with strict payload redaction.
- [x] 9.10 Add system-audit query isolation so tenant audit never exposes `_sys` operations.

## 10. Standard Boundary Instrumentation

- [x] 10.1 Add low-cardinality HTTP server spans with route templates, method/status/error semantics, authenticated opaque IDs, and probe-route suppression.
- [x] 10.2 Add gRPC/Flight server/client spans with service/method/status and streaming handshake semantics.
- [x] 10.3 Add a shared outbound HTTP client wrapper with Context injection, destination sanitization, timeout/status/error fields, and third-party Baggage stripping.
- [x] 10.4 Add SQL transaction/query/pool-wait instrumentation using operation, collection/module, fingerprint, rows, timing, and sanitized errors without SQL values.
- [x] 10.5 Replace direct production `Arc<dyn ObjectStore>` escape paths with one instrumented ObjectStore decorator covering put/get/get_range/head/list/delete/copy/rename/multipart.
- [x] 10.6 Add logical object operation spans with backend/bucket/category/bytes/cache/retry/result fields and no complete object key.
- [x] 10.7 Model cache hit/miss on parent spans and create additional spans only for slow loading, backing-store work, or failures.
- [x] 10.8 Add contract tests for HTTP/gRPC status mapping (server 5xx/error vs normal 4xx), SQL sanitization, object-key sanitization, retry/multipart bounds, and cache semantics.

## 11. Full Business-Module Instrumentation

- [x] 11.1 Instrument all ingestion protocols by request/batch/stage, including parse, validate, pipeline, masking, WAL, buffering, flush, Parquet, and internal OTLP paths without per-event spans.
- [x] 11.2 Instrument query planning, admission, metadata lookup, Tantivy pruning, DataFusion/PromQL execution, distributed shard fan-out, Flight streaming, and result serialization.
- [x] 11.3 Instrument Pipeline scheduling/execution/backfill, functions runtime, extend tables, retry, and result persistence.
- [x] 11.4 Instrument alert evaluation, incident grouping, mute/escalation, notification delivery, and connector dispatch with recipient/content redaction.
- [x] 11.5 Instrument scheduled reports, renderer, object archive, delivery attempts, and cleanup workers.
- [x] 11.6 Instrument Compactor, file-meta dump, service-graph workers, search jobs, WAL replay, cluster sync, quota/admission refresh, and other scheduled/background workers.
- [x] 11.7 Instrument intelligence/AI provider streaming and Tool execution with provider/model/token/stage/tool metadata while excluding prompts, responses, arguments, and results.
- [x] 11.8 Instrument auth/identity/system/License management paths without recording credentials, tokens, email/name, License packages, or signatures.
- [x] 11.9 Add explicit suppression around self-telemetry ingest/export/diagnostics so the complete instrumented stack cannot recursively trace its own persistence path.
- [x] 11.10 Add a coverage inventory test or lintable registry proving every registered HTTP/gRPC route, configured worker, SQL wrapper, object-store client, and outbound client has an intentional tracing policy.

## 12. Async, Retry, Fan-Out, and Streaming Semantics

- [x] 12.1 Add helpers that inherit Context for bounded in-process tasks and persist/link Context for queued, delayed, and retry work.
- [x] 12.2 Update all queue/job records that need correlation to carry a bounded serialized Trace Context without making it an authorization source.
- [x] 12.3 Ensure scheduled tasks start new roots and retries start linked execution traces rather than extending completed requests.
- [x] 12.4 Model parallel shard/worker fan-out as sibling spans with bounded aggregate fallback after the Trace Span cap.
- [x] 12.5 Split SSE, streaming HTTP, gRPC, Flight, and AI streams into handshake plus linked 30-second/1,000-message session segments.
- [x] 12.6 Convert retry attempts and multipart parts to bounded explicit Events on one logical Span.
- [x] 12.7 Add async/streaming tests for parent inheritance, Link persistence, authorization independence, retries, segment rollovers, cancellation, timeout, and late errors.

## 13. Privacy, Limits, and Security Verification

- [x] 13.1 Implement a centralized Trace sanitizer and a second pre-sink invariant check for forbidden keys and values.
- [x] 13.2 Add allowlists/normalizers for HTTP, RPC, SQL, object storage, identity, notifications, AI/LLM, and License attributes.
- [x] 13.3 Implement safe truncation and HMAC-based optional fingerprints without exposing the fingerprint key.
- [x] 13.4 Add fuzz/property tests for hostile headers, Baggage, URLs, SQL, object keys, errors, nested Events/Links, and oversized strings.
- [x] 13.5 Add end-to-end assertions that neither `_sys/_molesignal`, external OTLP, logs, audit, nor config diffs contain credentials or forbidden content.
- [x] 13.6 Verify all Trace/health/alert metrics remain bounded-cardinality under many organizations, routes, object keys, and Trace IDs.

## 14. End-to-End, Performance, Documentation, and Release

- [x] 14.1 Add standalone end-to-end coverage from HTTP ingress through SQL/object storage/business stages to `_sys/_molesignal` query with logs and `X-Trace-Id` correlation.
- [x] 14.2 Add split-role end-to-end coverage across router, querier, ingester, Flight/federation, tail owner, both sinks, and role-aware service graph.
- [x] 14.3 Add error/slow/ratio/rule/debug sampling acceptance tests, including child-slow retention, late error boundary, duplicates, conflicts, and partial traces.
- [x] 14.4 Add failure-injection tests for `_sys` preparation, sampler owner loss, no owner, self-ingest failure, external failure, queue/cache overflow, policy load, and shutdown timeout.
- [x] 14.5 Add permission/end-to-end tests for permanent `_sys/_molesignal`, system switching, platform admin management, License isolation/history, and all `404` boundaries.
- [x] 14.6 Add representative ingestion/query/object-store benchmarks and enforce CPU overhead <=5% and P95 latency overhead <=3% under default settings.
- [x] 14.7 Document architecture, Span catalog, semantic fields, context trust, sampling decisions, capacity sizing, privacy, metrics/alerts, `_sys` access, License operations, and failure semantics.
- [x] 14.8 Update configuration and API references for default-enabled Trace, deployment force-disable, `/api/v1/system/*`, OTLP protocols/TLS/auth, per-signal retention, and removed `/api/v1/license`.
- [x] 14.9 Prepare one-release rollout instructions: ship all functionality together, force-disable in production first, canary selected instances, verify gates, then enable through `_sys` policy.
- [x] 14.10 Run formatting, focused tests, full `cargo test`, `cargo clippy --all-targets --all-features`, OpenSpec validation, and record any environment-gated exclusions before implementation sign-off.
