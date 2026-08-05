## ADDED Requirements

### Requirement: Profiles Stream Type Ingestion

The ingestion pipeline SHALL accept `StreamType::Profiles` batches through the same `IngestService::ingest` path as other signals — honoring drain gating, schema-on-write stream creation, and masking — while treating profiles streams as ineligible pipeline transform targets (`allowed_as_pipeline_target` returns `false`).

#### Scenario: Profiles batch flows through the unified pipeline

- **WHEN** a profiles metadata batch is ingested for a not-yet-existing stream
- **THEN** the stream is auto-created via schema-on-write
- **AND** the metadata rows are persisted with `stream_type = Profiles`

#### Scenario: Profiles stream rejected as pipeline target

- **WHEN** a pipeline is configured to target a profiles stream
- **THEN** the configuration is rejected because profiles are not an allowed pipeline target

#### Scenario: Draining node rejects profiles writes

- **WHEN** the node is draining and a profiles batch arrives
- **THEN** ingestion returns `503` and persists nothing
