## ADDED Requirements

### Requirement: Per-Org Storage Provider Routing

The system SHALL maintain a `org_storage_providers` table `{ org_id PK, backend, bucket, region, endpoint, access_key_secret_ref, prefix? }` allowing each organization to use its own object_store backend independent of the global `[store.object]` default. A `StorageRouter` service SHALL return the right `Arc<dyn ObjectStore>` given an `org_id`; the global store remains the fallback for orgs without an entry.

#### Scenario: Per-org bucket used

- **WHEN** org A has a row `{ backend: "s3", bucket: "acme-obs", region: "us-east-2" }` in `org_storage_providers`
- **AND** an ingester flushes a file for org A
- **THEN** the file lands in `s3://acme-obs/...` not the global default bucket

#### Scenario: Global fallback for unconfigured org

- **WHEN** org B has no `org_storage_providers` row
- **THEN** flushes for org B go to the global `[store.object]` bucket

### Requirement: Storage Provider CRUD HTTP

The system SHALL expose `/api/v1/clusters/storage_providers` Owner-only CRUD for the above table. `access_key` SHALL be referenced by Kubernetes Secret name (`secret_ref`), not stored inline.

#### Scenario: Inline access_key rejected

- **WHEN** an Owner POSTs `{ "access_key": "AKIASECRETSECRET" }` directly
- **THEN** the system returns 400 with body `{ "error": "access_key must be a Kubernetes secret_ref, not inline" }`
