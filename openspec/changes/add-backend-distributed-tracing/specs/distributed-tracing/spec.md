## ADDED Requirements

### Requirement: End-to-End Backend Span Coverage

The system SHALL create OpenTelemetry spans for every backend boundary and critical processing stage, including Axum HTTP, Tonic gRPC, Arrow Flight, cluster and federation calls, outbound HTTP/gRPC, SQL, object storage, ingestion, query, Pipeline, alerting, notifications, scheduled reports, AI/LLM calls, compaction, and background workers. Span names SHALL use low-cardinality route, RPC, operation, or task templates and custom attributes SHALL use the `molesignal.*` namespace.

#### Scenario: Request crosses backend boundaries
- **WHEN** one authenticated HTTP query fans out through a querier, SQL metadata lookup, Arrow Flight shard request, and object-store reads
- **THEN** the stored Trace contains one connected hierarchy covering each boundary and critical query stage
- **AND** no Span name contains a raw URL, object key, SQL text, user ID, or trace ID

#### Scenario: Batch ingestion stays bounded
- **WHEN** one ingest request contains 100,000 events
- **THEN** the system creates spans for the request, batch, and processing stages
- **AND** it does not create one Span per ingested event

### Requirement: W3C Context Propagation and Trust

The system SHALL extract and inject W3C `traceparent` and `tracestate` across HTTP, gRPC, Flight, cluster, and federation boundaries. Baggage SHALL be restricted to validated `org.id` and `request.id`; authenticated organization context SHALL override inbound `org.id`. Internal Baggage SHALL only be forwarded to configured trusted internal targets and SHALL be removed for third-party targets. B3 and Jaeger propagation formats SHALL NOT be enabled.

#### Scenario: Internal call continues a Trace
- **WHEN** an authenticated HTTP request calls an internal gRPC service
- **THEN** the gRPC server Span has the calling Span as its remote parent
- **AND** the authenticated `org.id` and validated `request.id` are available on both spans

#### Scenario: Spoofed baggage is replaced
- **WHEN** an external client sends a valid `traceparent` but a Baggage `org.id` different from its authenticated organization
- **THEN** the request continues with the supplied trace identity
- **AND** the untrusted organization value is replaced by the authenticated organization ID
- **AND** a bounded-cardinality security metric increments

#### Scenario: Third-party call strips internal baggage
- **WHEN** a traced request calls a target not present in the internal-target allowlist
- **THEN** the outbound request may contain `traceparent` and `tracestate`
- **AND** it does not contain `org.id`, `request.id`, `user.id`, or `api_token.id` Baggage

### Requirement: Sampling Flags Cannot Be Forced by Untrusted Callers

An untrusted caller's sampled flag SHALL NOT force local retention. The system SHALL continue the inbound Trace Context while applying the local tail-sampling policy. Only a trusted internal identity or a platform-authorized, scoped, rate-limited debug token SHALL force retention.

#### Scenario: External sampled flag is only a hint
- **WHEN** an external caller sends `traceparent` with the sampled bit set
- **THEN** the local Trace is still evaluated by the configured tail-sampling rules
- **AND** the caller cannot bypass rate, memory, or sampling limits

#### Scenario: Scoped debug token forces retention
- **WHEN** a platform administrator uses an unexpired debug token whose organization and route scope match the request
- **THEN** the Trace is retained with sampling reason `debug_forced`
- **AND** token use is rate-limited and audited without storing the token value

### Requirement: Asynchronous and Streaming Trace Semantics

Short-lived in-process work SHALL inherit the active parent. Queued, delayed, retried, or long-running work SHALL start a new Trace linked to the originating context. Scheduled work SHALL start a new root Trace. SSE, streaming HTTP, and streaming gRPC/Flight sessions SHALL separate handshake spans from session spans and SHALL roll linked session segments at configurable time or message limits.

#### Scenario: Queued work uses a Link
- **WHEN** an HTTP request enqueues work that begins after the request has completed
- **THEN** the worker creates a new root Span with a Link to the producer context
- **AND** queue wait time is not counted as child latency of the completed HTTP request

#### Scenario: Streaming session rolls segments
- **WHEN** an SSE or streaming RPC remains open longer than 30 seconds
- **THEN** the handshake Span has already ended
- **AND** the session creates linked segments no longer than the configured 30-second or 1,000-message limit
- **AND** a late stream error can cause its current segment Trace to be retained

### Requirement: Logical Retry, Multipart, and Cache Modeling

One logical downstream or object-store operation SHALL create at most one operation Span. Retry attempts and multipart parts SHALL be recorded as bounded Events and aggregate attributes. Cache hit/miss SHALL normally be recorded on the parent operation; only slow loads, backing-store calls, and failures SHALL create additional spans.

#### Scenario: Object-store retry does not multiply spans
- **WHEN** one `object_store.get` succeeds after two transient failures
- **THEN** the Trace contains one logical object-store operation Span
- **AND** that Span contains retry count, bounded failure Events, backoff duration, transferred bytes, cache state, and the final successful status

#### Scenario: Multipart upload is summarized
- **WHEN** one large object is uploaded in 500 parts
- **THEN** the Trace does not contain 500 part child spans
- **AND** the logical upload Span records part count, total bytes, bounded part-error Events, and final result

### Requirement: Distributed Tail Sampling

The system SHALL perform one tail-sampling decision for all spans sharing a `trace_id`. Decision priority SHALL be: trusted force-keep, any ERROR Span, any slow Span, ordered explicit rules, then deterministic `trace_id` ratio sampling. Production normal traffic SHALL default to 10% and development/test SHALL default to 100%. A slow child Span SHALL retain the entire Trace even when the root is below its threshold.

Default slow thresholds SHALL be configurable and SHALL initially be: ordinary HTTP/gRPC 1 second, query 5 seconds, batch ingest 2 seconds, database 200 milliseconds, object store 500 milliseconds, external call 1 second, and background task 30 seconds.

#### Scenario: Error Trace is retained
- **WHEN** any Span observed before the decision deadline has status ERROR
- **THEN** all available spans for that Trace are retained with sampling reason `error`

#### Scenario: Slow child retains the Trace
- **WHEN** an object-store child Span takes 700 milliseconds but its root HTTP request completes within 1 second
- **THEN** the full Trace is retained with sampling reason `slow`

#### Scenario: Normal sampling is stable
- **WHEN** the same normal `trace_id` is evaluated on different sampler instances with a 10% policy
- **THEN** every instance computes the same keep or drop result

### Requirement: Bounded Tail-Sampling Lifecycle

The sampler SHALL use a configurable decision window defaulting to 30 seconds and constrained to 5–120 seconds. A completed root SHALL wait a one-second grace period before early decision. Decisions SHALL be cached so late spans follow the same result; spans arriving after the applicable decision cache SHALL NOT resurrect a dropped Trace. The cache SHALL be bounded by configurable memory and trace/span limits.

#### Scenario: Root completion triggers early decision
- **WHEN** a root Span ends and all known children arrive during the next one second
- **THEN** the sampler decides without waiting for the full 30-second window

#### Scenario: Late error does not create an orphan Trace
- **WHEN** a normal Trace was dropped and an error Span arrives after the decision window
- **THEN** the late Span follows the cached drop result
- **AND** no partial Trace is recreated
- **AND** the late-span and potential missed-error metrics increment

#### Scenario: Sampler pressure degrades without blocking
- **WHEN** the tail cache reaches its configured capacity
- **THEN** already-observed error and slow Traces receive priority
- **AND** normal Traces are decided early by deterministic ratio or dropped
- **AND** application request handling does not wait for cache capacity

### Requirement: Trace-Affinity Ownership and Failure Semantics

In a distributed deployment, all spans for the same `trace_id` SHALL be routed through consistent hashing to one sampler owner. Owner changes SHALL affect new routing without blocking producers. The system MAY lose unresolved traces held by a failed owner for at most one decision window and SHALL NOT require sampler replication or a Trace WAL.

#### Scenario: Cross-role Trace reaches one owner
- **WHEN** router, querier, ingester, and object-store spans share a trace ID
- **THEN** all four producers route their CanonicalSpans to the same active owner
- **AND** one decision controls both internal and external sinks

#### Scenario: Owner failure is bounded
- **WHEN** a sampler owner terminates with unresolved traces
- **THEN** new spans are rehashed to an available owner
- **AND** core request processing continues
- **AND** the estimated unresolved loss is reported through metrics and an alert

### Requirement: Shared Sampling with Isolated Sinks

Self-ingest and external OTLP SHALL receive the same retained Trace set and sampling reason. Each sink SHALL have an independent bounded queue, retry state, timeout, drop accounting, and health state. Failure or backpressure in one sink SHALL NOT delay or disable the other.

#### Scenario: External collector fails while self-ingest works
- **WHEN** the external OTLP collector is unavailable but internal ingestion is healthy
- **THEN** retained traces continue to appear in `_sys/traces/_molesignal`
- **AND** only the external sink retries and drops
- **AND** business responses are unaffected

### Requirement: Trace Privacy and Cardinality Limits

The system SHALL sanitize Trace data before enqueue and before sink export. It MUST NOT record credentials, cookies, request/response bodies, query strings, names, email addresses, SQL parameters, complete object keys, LLM prompts/responses, Tool arguments/results, or notification contents. It SHALL record only normalized routes/operations, SQL fingerprints, object categories or optional keyed fingerprints, opaque internal IDs, sizes, counts, durations, and status fields.

Each Span SHALL default to at most 128 attributes, 128 Events, 128 Links, and 4 KiB per string. Each Trace SHALL default to at most 1,000 spans. Truncation SHALL preserve error/status/semantic-convention fields first and SHALL expose dropped counts and a partial reason.

#### Scenario: Sensitive values never reach a sink
- **WHEN** a request contains Authorization, email, SQL bind parameters, an S3 key with a user filename, and an LLM prompt
- **THEN** neither `_molesignal` nor external OTLP contains any original sensitive value
- **AND** the Trace contains allowed operation metadata and explicit redaction/truncation counters

#### Scenario: High-fanout Trace is summarized
- **WHEN** a query would produce more than 1,000 storage and shard spans
- **THEN** error and slow spans are retained first
- **AND** excess operations are represented by aggregate count, duration, and byte fields
- **AND** the Trace is marked partial with reason `span_limit`

### Requirement: Non-Blocking Performance and Release Gate

Trace generation, sampling, and export SHALL be fail-open for runtime dependency failures and SHALL use bounded non-blocking paths. Under default sampling, CPU overhead SHALL remain at or below 5% and P95 request latency overhead SHALL remain at or below 3% in the agreed benchmark. The automated release gate SHALL verify context continuity, coverage, sampling, sink isolation, privacy, limits, shutdown, and failure behavior.

#### Scenario: Export queues are exhausted
- **WHEN** both Trace sinks are unavailable and their queues are full
- **THEN** new Trace records are dropped with reason metrics
- **AND** application requests continue according to their business result

#### Scenario: Performance budget is enforced
- **WHEN** the representative ingestion/query/object-store benchmark runs with default Trace settings
- **THEN** measured CPU and P95 latency overhead remain within the documented limits
- **AND** a regression beyond either limit fails the release gate

