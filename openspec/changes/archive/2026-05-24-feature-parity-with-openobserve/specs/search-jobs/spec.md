## ADDED Requirements

### Requirement: Async search job submission

The system SHALL expose `POST /api/v1/query/jobs` accepting the same body as `POST /api/v1/query` plus optional `{ ttl_secs }`. The response carries `{ "job_id": "<ksuid>", "state": "pending" }`. A background tokio task picks up pending jobs and runs them via the existing `QueryService::run`.

#### Scenario: Job goes through states

- **WHEN** a user submits a long query as a job
- **THEN** subsequent `GET /api/v1/query/jobs/<id>` returns state in order: `pending → running → done` (or `failed` on error)

### Requirement: Result pagination via persisted Parquet

Job results SHALL be written to object_store at `query_jobs/<org>/<job_id>.parquet` once `done`. `GET /api/v1/query/jobs/<id>/results?page=&page_size=` reads the Parquet and returns rows JSON-encoded with `{ rows, total_rows, next_page }`.

#### Scenario: Page through a large result

- **WHEN** a job result has 10000 rows and `?page=1&page_size=1000`
- **THEN** the response carries 1000 rows and `next_page: 2`

### Requirement: TTL cleanup

Jobs SHALL default to `ttl_secs = 7 * 86400`. A background `search_jobs_cleanup` task running every hour SHALL delete jobs whose `expires_at < now`, including the underlying Parquet object.

#### Scenario: Expired job removed

- **WHEN** a job's `expires_at` is in the past and cleanup runs
- **THEN** the job row is deleted, the Parquet object is removed, subsequent `GET .../jobs/<id>` returns 404
