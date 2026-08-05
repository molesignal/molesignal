## ADDED Requirements

### Requirement: Reserved `_molesignal` System Stream

The exact stream name `_molesignal` SHALL be reserved in every `StreamType`. Public HTTP, OTLP, Prometheus, compatibility, connector, profile, and user-facing gRPC ingestion paths SHALL reject writes targeting that name with `403 Forbidden`. Stream create, update, delete, retention/settings mutation, and pipeline target APIs SHALL also reject that name. Authorized platform administrators in `_sys` system scope SHALL still be able to list and query the system streams. Other names beginning with `_` SHALL not become reserved by this change.

#### Scenario: Public ingest cannot spoof self telemetry

- **WHEN** an authenticated client submits a logs batch targeting `_molesignal`
- **THEN** the request returns `403 Forbidden`
- **AND** no event is written

#### Scenario: System stream cannot be deleted or transformed

- **WHEN** a user attempts to delete `logs/_molesignal` or configure a pipeline targeting it
- **THEN** the operation is rejected as a protected system stream

#### Scenario: System scope can query system telemetry

- **WHEN** an authorized platform administrator in `_sys` system scope queries `traces/_molesignal`
- **THEN** system telemetry read authorization applies
- **AND** matching self traces are returned

### Requirement: Trusted Internal Ingest Origin

The application ingestion boundary SHALL distinguish trusted self-telemetry writes from public writes without accepting an origin flag from user-controlled payloads. Trusted internal batches MAY target `_molesignal` and SHALL continue through schema validation/evolution, configured masking, WAL durability, and drain semantics. They SHALL bypass user pipelines and public request billing/quota gates, and SHALL carry an explicit suppression scope so telemetry emitted by their processing is not recursively exported.

#### Scenario: Internal batch reaches the protected stream

- **WHEN** the self-telemetry worker submits a trusted internal logs batch
- **THEN** the batch is accepted for `logs/_molesignal`
- **AND** schema validation, masking, and WAL persistence are applied
- **AND** no user pipeline is executed for the batch

#### Scenario: Public payload cannot assert internal origin

- **WHEN** a public client includes fields or metadata that claim an internal origin
- **THEN** the public adapter still treats the request as external
- **AND** a target of `_molesignal` is rejected

#### Scenario: Drain still closes internal writes

- **WHEN** node drain has begun
- **THEN** a new internal telemetry batch is rejected by the same drain gate as other writes
- **AND** shutdown ordering is responsible for stopping and flushing self-telemetry producers before drain
