## ADDED Requirements

### Requirement: Org-Level Quota Storage and CRUD

The system SHALL maintain a `quotas` row per organization with `{ org_id PK, max_ingest_qps, max_query_qps, max_storage_bytes, max_streams, soft_warn_ratio: f32 (default 0.8), updated_at }`. `GET/PUT /api/v1/orgs/:id/quota` (Owner-only) reads and updates the row.

#### Scenario: Quota auto-created with org
- **WHEN** a new organization is created
- **THEN** a `quotas` row with default values (`max_ingest_qps = 10_000, max_query_qps = 100, max_storage_bytes = 100 GiB, max_streams = 100`) is inserted in the same transaction

#### Scenario: Owner updates quota
- **WHEN** the Owner PUTs new limits
- **THEN** the row updates atomically; the next quota refresh tick (default 30s) propagates new limits to all ingester / querier in-memory rate limiters

### Requirement: Ingest and Query Rate Limiting

The system SHALL enforce `max_ingest_qps` at every ingest entry (HTTP `/ingest/*`, OTLP, Prometheus remote_write, Loki, ES `_bulk`, Syslog) via a per-org token-bucket; over-limit requests return `429 Too Many Requests` with `Retry-After`. Similarly `max_query_qps` is enforced at `/api/v1/query` and Saved-View run endpoints.

#### Scenario: Ingest QPS exceeded
- **WHEN** ingests for `orgA` exceed `max_ingest_qps` over a 1-second window
- **THEN** subsequent requests within that window return `429 Too Many Requests` with `Retry-After: 1`, `quota_rejected_total{org, dimension="ingest_qps"} += 1`

#### Scenario: Soft-warn metric below hard limit
- **WHEN** usage reaches `>= soft_warn_ratio * max_*` (e.g., 80% of `max_storage_bytes`)
- **THEN** `quota_soft_warn_total{org, dimension}` increments at most once per minute per dimension; ingest is NOT rejected yet

### Requirement: Storage Bytes Enforcement

Compactor SHALL recompute `(org_id → sum(parquet_file_meta.size_bytes WHERE deleted = false))` every `quotas.recompute_interval_secs` (default 300). When the sum exceeds `max_storage_bytes` for an org, subsequent ingest for that org returns `413 Payload Too Large` until storage drops below the limit (via retention).

#### Scenario: Storage cap blocks ingest
- **WHEN** org `A` reaches `max_storage_bytes`
- **THEN** ingest returns `413 Payload Too Large` with `{ "error": "org storage cap reached", "current_bytes": X, "limit_bytes": Y }` and an audit row with `action="quota.block"` is written
