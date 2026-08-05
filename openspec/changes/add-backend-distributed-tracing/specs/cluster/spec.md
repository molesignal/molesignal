## ADDED Requirements

### Requirement: Trace-ID Affinity Routing

Cluster routing SHALL select one active tail-sampler owner by consistent/rendezvous hashing of `trace_id`, independent of the existing `(org_id, stream)` WAL routing key. All CanonicalSpans with the same Trace ID from router, ingester, querier, compactor, alert-manager, standalone, and federation boundaries SHALL target the same owner while preserving producer Resource identity.

#### Scenario: Same Trace remains co-located
- **WHEN** spans for one Trace arrive from four roles in arbitrary order
- **THEN** each role selects the same active sampler owner
- **AND** the owner can evaluate error, slow, rule, and ratio policy over the combined Trace

#### Scenario: Different traces distribute
- **WHEN** many independent Trace IDs are produced
- **THEN** ownership is distributed across active sampler nodes according to the hash ring
- **AND** no organization or raw route identifier appears in routing metrics

### Requirement: Authenticated CanonicalSpan Transport

Inter-node Trace candidate transport SHALL use a cluster-authenticated RPC that accepts the bounded CanonicalSpan contract and cannot be invoked as trusted self telemetry by a public client. The receiving owner SHALL retain the producer's node, role, service, and instrumentation identity rather than replacing them with receiver identity.

#### Scenario: Public caller cannot submit an internal candidate
- **WHEN** a caller without cluster authentication reaches the internal Trace transport
- **THEN** the request is rejected before enqueue

#### Scenario: Producer identity survives routing
- **WHEN** a querier sends a CanonicalSpan to an ingester sampler owner
- **THEN** stored and exported data identifies the querier as the producer
- **AND** receiver identity appears only in internal delivery diagnostics

### Requirement: Owner Change and Bounded Delivery

Sampler ownership changes SHALL not block application producers. Candidate delivery SHALL use bounded queues, timeout, age, and exponential-backoff retry. When an owner disappears, new candidates SHALL rehash to a live owner; unresolved in-memory traces on the failed owner MAY be lost for at most one decision window and SHALL be reported through health metrics.

#### Scenario: Owner leaves the cluster
- **WHEN** the current owner becomes unavailable
- **THEN** new candidate deliveries select another live owner after membership refresh
- **AND** producers do not wait indefinitely

#### Scenario: No owner is available
- **WHEN** a producer has no live sampler owner and its bounded retry/age limits expire
- **THEN** candidates are dropped with reason `no_owner`
- **AND** the producer's business operation continues

