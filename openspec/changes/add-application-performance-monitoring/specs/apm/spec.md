## ADDED Requirements

### Requirement: Pre-Sampling APM Projection

The system SHALL derive APM facts from sanitized CanonicalSpan candidates at the trace-affinity owner after duplicate/conflict classification and before the tail sampler discards unretained traces. A unique Span SHALL contribute at most once even when an OTLP exporter retries it, and the APM projection result SHALL NOT depend on whether the containing Trace is retained.

#### Scenario: Sampled-out request remains in APM metrics
- **WHEN** a unique SERVER Span is accepted by the candidate owner and its Trace is subsequently dropped by ratio sampling
- **THEN** the Span contributes one request and its duration/status to the matching APM service and transaction bucket
- **AND** the raw Trace remains absent from Trace search

#### Scenario: OTLP retry does not double count
- **WHEN** two identical candidates carry the same `(org_id, trace_id, span_id)`
- **THEN** the candidate disposition marks the second as an identical duplicate
- **AND** APM request, error and duration aggregates increase only once

#### Scenario: Projection cannot block Trace ingest
- **WHEN** the APM projection queue or repository is unavailable
- **THEN** Trace candidate routing and tail sampling continue without waiting for APM storage
- **AND** the dropped projection fact and affected time range are exposed through APM health metrics

### Requirement: Application Service Catalog

The system SHALL maintain an organization-scoped APM service catalog from observed Resource attributes. A service identity SHALL consist of normalized `service.namespace`, `service.name` and `deployment.environment.name`; missing namespace or environment values SHALL use stable explicit fallback values. The catalog SHALL track first/last seen time, observed versions, runtime language, telemetry SDK name/version and recent instance count without treating version or instance ID as part of the stable service identity.

#### Scenario: First Span creates a service
- **WHEN** the first valid Span arrives with `service.namespace=shop`, `service.name=checkout`, `deployment.environment.name=prod` and `service.version=2.4.0`
- **THEN** the catalog contains one `shop/checkout` service in `prod`
- **AND** version `2.4.0`, first/last seen and SDK metadata are recorded

#### Scenario: New version does not duplicate the service
- **WHEN** the same service and environment later emits `service.version=2.5.0`
- **THEN** the existing service identity remains unchanged
- **AND** both versions are available as dimensions and comparison candidates

#### Scenario: Standalone service remains discoverable
- **WHEN** a service emits only root/server Spans and has no cross-service edge
- **THEN** it appears in the APM service catalog
- **AND** its visibility does not depend on Service Graph parent/child pairing

### Requirement: Service RED Metrics

The system SHALL aggregate request rate, error count/rate and duration distributions for SERVER and CONSUMER Spans, with parentless Spans used only as a documented fallback when Span kind is absent. Metrics SHALL be bucketed by organization, service identity, environment and event-time minute; `service.version` SHALL be an optional comparison dimension. Duration percentiles SHALL be computed from mergeable histogram buckets rather than by taking the maximum or average of precomputed percentiles.

#### Scenario: Service request counted once
- **WHEN** one SERVER Span calls three downstream dependencies through three CLIENT Spans
- **THEN** the service request count increases by one
- **AND** dependency counts increase independently without inflating service throughput

#### Scenario: Window percentile merges distributions
- **WHEN** a query covers multiple minute buckets with different duration distributions
- **THEN** P50/P95/P99 are calculated from the merged histogram counts for the complete window
- **AND** the result is not the maximum of the individual bucket percentiles

#### Scenario: Protocol status contributes to error rate
- **WHEN** a service Span has OTLP status ERROR, HTTP status at least 500, or a non-OK RPC status
- **THEN** the matching service and transaction error counts increase

### Requirement: Transaction Aggregation

The system SHALL group low-cardinality request operations into Transactions. Transaction name precedence SHALL be HTTP method plus route template, RPC service/method, messaging operation plus destination template, then a sanitized Span name fallback. Raw URL paths, query strings, user identifiers and unbounded operation values MUST NOT become Transaction dimensions.

#### Scenario: HTTP requests group by route template
- **WHEN** requests for `/orders/123` and `/orders/456` carry `http.route=/orders/{id}` and method `GET`
- **THEN** both contribute to Transaction `GET /orders/{id}`
- **AND** neither raw order ID is stored as a Transaction name

#### Scenario: Transaction list exposes actionable ranking
- **WHEN** a caller queries Transactions for one service and time range
- **THEN** the response can be sorted by throughput, error rate, P95 latency or total time
- **AND** each row includes request/error counts, latency percentiles and a filtered Trace-search link

### Requirement: Dependency Performance Aggregation

The system SHALL derive dependency metrics from CLIENT and PRODUCER Spans and classify dependencies as service, database, cache, messaging or external HTTP/RPC using OpenTelemetry semantic attributes. Each dependency SHALL expose request count, error rate and duration distribution from the calling service's perspective.

#### Scenario: Database operation is grouped safely
- **WHEN** CLIENT Spans contain `db.system.name=postgresql`, `db.namespace=orders` and different parameterized SQL statements
- **THEN** they are grouped under the normalized PostgreSQL `orders` dependency and operation fingerprint
- **AND** raw SQL text and parameters are not persisted in APM tables

#### Scenario: External endpoint excludes high-cardinality URL data
- **WHEN** an HTTP CLIENT Span contains a full URL with path and query parameters
- **THEN** dependency identity uses sanitized scheme/host/port or an explicit peer service name
- **AND** full URL, path and query values are excluded

### Requirement: Backend Error Grouping

The system SHALL create organization-scoped backend error groups from ERROR Spans and `exception.*` Events. A stable fingerprint SHALL be derived from service identity, environment, exception/error type, normalized top application frame and Transaction name. Each group SHALL expose first/last seen, occurrence trend, affected Transactions and versions, a sanitized representative message/stack and bounded representative Trace references.

#### Scenario: Variable message values share one group
- **WHEN** two exceptions have the same type, normalized application frame and Transaction but messages contain different request IDs
- **THEN** they resolve to the same error fingerprint
- **AND** the occurrence count increases by two

#### Scenario: Representative Trace availability is explicit
- **WHEN** an error occurrence refers to a retained Trace
- **THEN** the error detail exposes a Trace deep link
- **AND** an occurrence whose Trace is unavailable is marked without presenting a broken link

### Requirement: Version Detection And Comparison

The system SHALL detect service versions from `service.version`, record each version's first/last seen time, and compare two versions of the same service/environment using request count, error rate and merged duration percentiles. Comparisons SHALL report sample counts and an explicit insufficient-data state when either side is below the configured minimum.

#### Scenario: Candidate version regression is visible
- **WHEN** candidate version `2.5.0` has sufficient traffic and a materially higher error rate or P95 than baseline `2.4.0`
- **THEN** the comparison response includes absolute and relative deltas
- **AND** identifies the Transactions contributing most to the regression

#### Scenario: Sparse version is not overstated
- **WHEN** a version has fewer observations than the configured comparison minimum
- **THEN** the API marks the comparison as insufficient data
- **AND** the UI does not label it as improved or regressed

### Requirement: APM Query API

The system SHALL expose organization-scoped `/api/v1/apm/*` read endpoints for overview, service catalog/detail, Transactions, Dependencies, error groups/detail and version comparison. All list endpoints SHALL accept bounded time ranges, environment/version/service filters, stable sorting and cursor pagination; responses SHALL include the last complete bucket and data-quality metadata.

#### Scenario: Service overview is filtered consistently
- **WHEN** a Viewer requests `/api/v1/apm/services/{service}` with a time range and environment/version filters
- **THEN** every KPI, trend, Transaction, dependency and error summary in the response uses the same filters

#### Scenario: Cross-organization lookup is hidden
- **WHEN** a user in organization A requests an APM service or error fingerprint belonging only to organization B
- **THEN** the API returns `404`
- **AND** no aggregate, catalog metadata or existence signal from organization B is disclosed

#### Scenario: Partial projection is visible
- **WHEN** projection drops overlap the requested range
- **THEN** the response reports `data_quality.partial=true`, the affected interval and last complete bucket
- **AND** it does not silently present incomplete values as exact

### Requirement: Cardinality And Privacy Boundaries

The APM projector SHALL enforce configurable per-organization and per-service limits for service, Transaction, dependency, version, error-group and instance dimensions. Values beyond a dimension limit SHALL be folded into deterministic `__other__` buckets or dropped according to dimension policy, with counters identifying the reason. Existing centralized Trace sanitization and organization masking rules SHALL apply before APM facts are created.

#### Scenario: Route explosion is bounded
- **WHEN** one service emits more unique Transaction names than its configured hourly limit
- **THEN** additional names contribute to the service's `__other__` Transaction bucket
- **AND** projection memory and persisted row counts remain bounded

#### Scenario: Forbidden fields never reach APM storage
- **WHEN** a Span contains authorization data, cookies, URL query values, SQL parameters or other forbidden attributes
- **THEN** none of those values appear in APM dimensions, error samples, database rows, API responses or diagnostic logs

### Requirement: APM Retention And Rollup

The system SHALL retain recent one-minute APM buckets and compact closed buckets into mergeable longer-resolution rollups for the configured APM retention period. Compaction SHALL preserve exact request/error counts and merge latency histogram counts. Projection snapshots and compaction writes SHALL be idempotent.

#### Scenario: Rollup preserves aggregate semantics
- **WHEN** sixty closed one-minute buckets are compacted into an hourly bucket
- **THEN** request/error counts equal the sum of the source buckets
- **AND** P50/P95/P99 computed from the hourly histogram are equivalent to merging the source histograms

#### Scenario: Retry does not duplicate a bucket
- **WHEN** a projection flush or rollup transaction is retried after an ambiguous completion
- **THEN** the persisted owner snapshot or rollup is replaced/idempotently recognized
- **AND** counts are not added twice

### Requirement: APM Operational Health

The system SHALL automatically start APM projection on every role that owns Trace candidates and APM rollup on every alert-manager role, without an APM enable or kill-switch setting. Roles that own neither responsibility SHALL NOT allocate either worker. The system SHALL publish low-cardinality metrics and detailed health state for projection accepted/dropped facts, queue depth/capacity, dimension overflow, late facts, flush/rollup latency, repository failures, latest complete bucket and API query latency. APM degradation SHALL NOT fail general readiness.

#### Scenario: Relevant roles start APM without opt-in
- **WHEN** a node starts with the ingester, querier, or alert-manager role
- **THEN** its applicable APM projector or rollup worker starts automatically
- **AND** no APM enable or force-disable configuration is required or accepted

#### Scenario: Projection backlog becomes diagnosable
- **WHEN** the projection queue remains above 80% capacity or drops facts
- **THEN** APM health is marked degraded and metrics identify the bounded reason
- **AND** application ingest endpoints remain available
