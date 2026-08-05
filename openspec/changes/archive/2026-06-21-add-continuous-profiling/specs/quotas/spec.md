## ADDED Requirements

### Requirement: Profiles Ingest Quota Enforcement

Profiles ingestion SHALL be subject to the same per-org quota enforcement as other ingest paths: archived blob bytes SHALL count against `max_storage_bytes` and ingest rate against `max_ingest_qps`.

#### Scenario: Archive bytes count against storage

- **WHEN** a profile is ingested for an org under quota
- **THEN** the archived object's size is added to the org's storage usage

#### Scenario: Over-quota profile rejected

- **WHEN** an org is over `max_storage_bytes` and posts a profile
- **THEN** the response is `413 Payload Too Large`
- **AND** no archive object is written
