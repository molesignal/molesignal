## ADDED Requirements

### Requirement: Pipeline Runs Persistence

The system SHALL persist a row in a Postgres table `pipeline_runs` for every scheduled-pipeline execution attempt. Columns: `(id TEXT PRIMARY KEY, pipeline_id TEXT NOT NULL, org_id TEXT NOT NULL, state TEXT NOT NULL, started_at_micros BIGINT NOT NULL, finished_at_micros BIGINT, scanned_rows BIGINT NOT NULL DEFAULT 0, error TEXT)`. `state` takes one of `running | succeeded | failed | cancelled`. A row SHALL be inserted with `state = 'running'` before execution begins and updated to its terminal state when execution ends.

#### Scenario: Successful run produces succeeded row

- **WHEN** a scheduled pipeline tick runs for pipeline `p1` and completes successfully
- **THEN** `pipeline_runs` contains a row with `pipeline_id = 'p1'`, `state = 'succeeded'`, and a non-null `finished_at_micros`

#### Scenario: Failed run captures error

- **WHEN** a pipeline tick fails with `Error::internal("pg timeout")`
- **THEN** the row's `state = 'failed'` and `error = 'pg timeout'`

#### Scenario: Currently running row visible mid-execution

- **WHEN** a long-running pipeline is mid-execution
- **THEN** its `pipeline_runs` row has `state = 'running'` and `finished_at_micros IS NULL`

### Requirement: Pipeline Runs List Endpoint

The system SHALL expose `GET /api/v1/scheduled_pipelines/{id}/runs?limit=&before_micros=` returning the most recent runs of a pipeline scoped to the caller's org. The endpoint requires `StreamRead` permission. Pagination uses `before_micros` as a `started_at_micros` cursor.

#### Scenario: List returns recent runs sorted

- **WHEN** an authenticated user GETs `/api/v1/scheduled_pipelines/p1/runs?limit=20`
- **THEN** the response is a JSON array of up to 20 rows for `p1` in the caller's org sorted by `started_at_micros DESC`

#### Scenario: Cross-org list denied

- **WHEN** a user GETs `/api/v1/scheduled_pipelines/p1/runs` and pipeline `p1` belongs to another org
- **THEN** the response is `404 Not Found` (no `org_id` leakage)
