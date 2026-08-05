## ADDED Requirements

### Requirement: Multi-Stream SQL Search

The system SHALL support SQL that references multiple streams in a single statement (e.g., `SELECT a.trace_id, b.host FROM app_logs a JOIN host_metrics b ON a.host = b.host`). The planner SHALL collect all referenced streams, fetch each one's `ParquetFileMeta` set independently, apply the multi-tenant rewrite to each scan, and let DataFusion execute the join over the union.

#### Scenario: Two-stream JOIN executes

- **WHEN** a user submits `SELECT a.level, count(*) FROM app_logs a JOIN host_metrics b ON a.host = b.host WHERE _timestamp BETWEEN ... GROUP BY a.level`
- **THEN** the response contains aggregated rows; each scan was org-scoped independently; `scanned_rows` reflects both streams' files

#### Scenario: Cross-org reference in JOIN rejected

- **WHEN** the JOIN references a stream belonging to another org
- **THEN** the rewrite pass returns 403 with body `{ "error": "stream not found: <name>" }`

### Requirement: Search Jobs Auto-Conversion

When a `POST /api/v1/query` request's predicted scan exceeds a threshold (`[querier].auto_async_threshold_rows`, default 10M) or carries `Prefer: respond-async` header, the system SHALL automatically convert to an async search job (see `search-jobs` capability) and return `202 Accepted` with `{ "job_id": "<ksuid>", "monitor": "/api/v1/query/jobs/<id>" }` instead of executing inline.

#### Scenario: Large query auto-converts

- **WHEN** a query's planner estimate exceeds `auto_async_threshold_rows`
- **THEN** response is `202` with `job_id`; the caller polls via `/jobs/<id>` to retrieve

#### Scenario: Header-forced async

- **WHEN** the request carries `Prefer: respond-async` regardless of size
- **THEN** the system always converts to async, even for small queries
