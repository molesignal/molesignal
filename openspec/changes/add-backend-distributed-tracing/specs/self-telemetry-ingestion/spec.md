## MODIFIED Requirements

### Requirement: Self-Telemetry Runtime Configuration

The system SHALL always bootstrap the immutable `_sys` system organization. Trace instrumentation SHALL be enabled by code default; deployment force-disable and persisted `_sys` runtime policy MAY disable it. Trace self-ingest SHALL target `_sys/traces/_molesignal` only when the shared `telemetry.self_collect.enabled` switch and the effective Trace capture policy are both enabled. External OTLP SHALL remain disabled until explicitly configured. Self logs and profiles SHALL follow `telemetry.self_collect.enabled`, while metrics MAY be disabled independently.

The target organization SHALL NOT be user-configurable. The configuration schema SHALL NOT expose `telemetry.self_collect.org_slug`, the legacy `telemetry.self_ingest` section, or `telemetry.trace.self_ingest_enabled`; runtime code SHALL resolve the immutable `_sys` identity from the system-organization constant. A temporary runtime failure to prepare `_sys` or a typed `_molesignal` stream SHALL degrade the affected self-telemetry signal, emit health/alert diagnostics, and SHALL NOT stop the core data plane. A structurally conflicting or tampered system identity SHALL fail startup.

#### Scenario: Enabled Trace self-ingest uses the system organization
- **WHEN** the server starts with `telemetry.self_collect.enabled = true`, without a deployment Trace force-disable, and with no persisted override
- **THEN** Trace instrumentation is enabled
- **AND** retained self traces target `_sys/traces/_molesignal`
- **AND** no external OTLP connection is attempted unless configured

#### Scenario: Deployment force-disable has no Trace storage side effects
- **WHEN** deployment Trace force-disable is true
- **THEN** no new application Trace candidates are generated or self-ingested
- **AND** `_sys` still exists for platform identity and License management
- **AND** self logs, metrics, and profiles retain their behavior under `telemetry.self_collect.enabled`

#### Scenario: Removed organization override is rejected
- **WHEN** configuration contains `telemetry.self_collect.org_slug`
- **THEN** configuration parsing fails with an unknown-field error
- **AND** no tenant organization can be selected as the self-telemetry target

#### Scenario: Removed Trace self-ingest switch is rejected
- **WHEN** configuration contains `telemetry.trace.self_ingest_enabled` or the legacy `telemetry.self_ingest` section
- **THEN** configuration parsing fails with an unknown-field error

#### Scenario: Temporary stream preparation failure is fail-open
- **WHEN** Trace and self telemetry are enabled but `_sys/traces/_molesignal` cannot be prepared due to a transient metadata-store failure
- **THEN** the Trace signal reports degraded and retries according to bounded policy
- **AND** core server roles continue starting

### Requirement: Four Typed System Streams

For every enabled self-telemetry signal, the system SHALL create or reuse a stream named `_molesignal` in `_sys` with the matching StreamType. Logs, metrics, traces, and profiles SHALL occupy four independently schematized streams identified as `(_sys_org_id, "_molesignal", stream_type)`. Each signal SHALL have an independently configurable retention; Trace retention SHALL default to seven days. The system stream identity and existence SHALL be permanently immutable, while authorized platform policy MAY change retention.

#### Scenario: All signals create distinct typed streams
- **WHEN** all four self-telemetry signals are enabled
- **THEN** system stream listing contains `logs/_molesignal`, `metrics/_molesignal`, `traces/_molesignal`, and `profiles/_molesignal` under `_sys`
- **AND** each stream has its own schema and retention

#### Scenario: A disabled signal creates no typed stream
- **WHEN** profiles collection is disabled
- **THEN** the runtime does not create `profiles/_molesignal` solely for profile collection
- **AND** enabled signal streams remain available

#### Scenario: System stream cannot be renamed or deleted
- **WHEN** any identity attempts to rename or delete one typed `_molesignal` stream
- **THEN** domain, Repository, and database protection reject the operation

### Requirement: Stable Resource Identity

Every self-telemetry record SHALL include `service.namespace = "molesignal"`, role-aware `service.name`, `service.version`, stable `service.instance.id`, `deployment.environment.name`, `node.id`, `cluster.id`, and available cloud region/zone. `service.instance.id` SHALL remain stable for the configured node instance rather than being regenerated independently by each sink. In a multi-role process, each Span SHALL record its actual execution role so Trace normalization can derive a role-specific effective service name.

#### Scenario: Two roles remain distinguishable
- **WHEN** an ingester and a querier emit telemetry to the same `_sys/_molesignal` stream
- **THEN** their records have stable node/instance identity
- **AND** effective service names distinguish ingester from querier

#### Scenario: Multi-role process does not create a combined graph node
- **WHEN** one process executes both router and querier spans
- **THEN** each Span carries the actual execution role
- **AND** topology derives role-specific nodes instead of `router+querier`

### Requirement: Self Log and Trace Collection

The tracing subscriber SHALL send accepted structured log events to `logs/_molesignal` and completed CanonicalSpans to the unified Trace pipeline without replacing console/file logging. Retained Trace candidates SHALL pass through one distributed tail-sampling decision before both `traces/_molesignal` storage and external OTLP export. Log records SHALL retain timestamp, level, target, message, permitted structured fields, source location when available, and active trace/span IDs. CanonicalSpans SHALL retain the complete bounded OTLP data contract.

Ordinary logs SHALL NOT automatically become Span Events. Only explicit business, retry, streaming, and error Events SHALL be attached to the Span.

#### Scenario: Correlated log and Trace are stored without duplication
- **WHEN** a request Span emits a normal structured info log and an explicit error Event
- **THEN** the log appears once in `logs/_molesignal` with matching trace/span IDs
- **AND** the retained Span contains the explicit error Event
- **AND** it does not contain a duplicate of the ordinary info log

#### Scenario: Existing sinks remain active
- **WHEN** console logging, self Trace ingest, and an external OTLP endpoint are configured
- **THEN** logs continue to reach the console
- **AND** one sampling decision fans retained spans to both Trace sinks
- **AND** a failure in either Trace sink does not affect the other

### Requirement: Role-Aware Routing and Lifecycle

Self logs, metrics, and profiles SHALL continue to use role-aware internal ingestion. Self Trace candidates from all roles SHALL route by `trace_id` affinity to one active sampler owner, retaining producer Resource identity. The owner SHALL make the shared sampling decision and fan retained spans to internal ingestion and the optional external sink.

On graceful shutdown, Trace candidate intake and sampler decisions SHALL stop/flush before self-sink and external-sink flush and before ingestion drain. The complete Trace flush SHALL be bounded by the configured timeout, defaulting to ten seconds; lack of an owner, ingester, or sink SHALL never prevent process shutdown.

#### Scenario: Querier Trace joins an ingester Trace
- **WHEN** querier and ingester spans share a Trace ID
- **THEN** both route to the same sampler owner
- **AND** the stored records preserve the original producer role and node
- **AND** one decision controls both sinks

#### Scenario: Shutdown is bounded
- **WHEN** shutdown begins with unresolved traces and queued sink batches
- **THEN** the runtime attempts decision and flush within the configured timeout
- **AND** records remaining after timeout are counted
- **AND** process drain and exit continue
