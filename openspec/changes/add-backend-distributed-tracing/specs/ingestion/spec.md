## ADDED Requirements

### Requirement: System Trace Internal Ingestion

Only the non-user-serializable internal self-telemetry path SHALL write retained MoleSignal spans to `_sys/traces/_molesignal`. Public HTTP, OTLP, Prometheus, compatibility, connector, profile, gRPC, pipeline, and system-scoped administrator requests SHALL NOT claim the internal origin or write directly to any `_molesignal` stream. Internal Trace ingestion SHALL preserve CanonicalSpan fields, schema evolution, masking, WAL durability, and drain behavior while bypassing tenant billing, quota, and user Pipeline execution.

#### Scenario: Retained internal Trace reaches WAL
- **WHEN** the tail sampler retains a CanonicalSpan and invokes trusted internal ingestion
- **THEN** the Span is validated and appended to the WAL for `_sys/traces/_molesignal`
- **AND** no tenant quota or user Pipeline is evaluated

#### Scenario: Platform administrator cannot impersonate self ingest
- **WHEN** a valid system-scoped administrator sends an OTLP or JSON ingest request targeting `_molesignal`
- **THEN** the request is rejected
- **AND** no internal-origin flag can be supplied over the public API

### Requirement: Permanent System Resource Protection

The ingestion and stream-management domain SHALL treat `_sys` and every typed `_sys/_molesignal` stream as permanent system resources. Stream rename, delete, organization reassignment, system-marker removal, public schema replacement, and Pipeline target/source mutation SHALL be rejected in domain validation, Repository methods, and database triggers. Authorized system telemetry policy MAY update retention and approved capacity-related properties only.

#### Scenario: Repository delete is blocked
- **WHEN** any application service calls `StreamRepository::delete` for a typed `_sys/_molesignal` stream
- **THEN** the Repository returns a system-resource error without issuing a destructive SQL operation

#### Scenario: Direct SQL update is blocked
- **WHEN** the application database role attempts to rename `_molesignal` or move it to another organization
- **THEN** the database rejects the transaction

#### Scenario: Retention update succeeds
- **WHEN** an authorized system telemetry policy changes Trace retention from seven to fourteen days
- **THEN** the retention column is updated
- **AND** every permanent identity field remains unchanged

### Requirement: Trace Storage Preserves Canonical Structure

The Trace ingestion adapter and inferred/evolved schema SHALL preserve nested Events, Links, Resource, Instrumentation Scope, dropped counts, sampling reason, and partial metadata defined by CanonicalSpan. Trace storage SHALL accept the new development-stage schema without attempting to interpret or migrate legacy self-trace rows.

#### Scenario: Nested Trace data remains queryable
- **WHEN** an internal CanonicalSpan contains Links, Events, scope attributes, and dropped counts
- **THEN** WAL, Parquet, and query reconstruction preserve those fields

#### Scenario: No legacy migration path runs
- **WHEN** the new `_sys/traces/_molesignal` stream is prepared
- **THEN** only the new canonical schema is required
- **AND** startup does not scan or rewrite old development Trace files

