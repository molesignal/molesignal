# Pipeline Backfill Capability

## Purpose

HTTP submission of a one-shot scheduled-pipeline backfill that runs the pipeline's transform over a user-chosen historical time window. The backfill is persisted as a search-job so it inherits the existing cancel / monitor / progress semantics of the query runtime.

## Requirements

### Requirement: Backfill Submission Endpoint

The system SHALL expose `POST /api/v1/scheduled_pipelines/{id}/backfill` (OrgAdmin+) accepting a JSON body `{start_micros: number, end_micros: number}`. The handler SHALL:

1. Validate `end_micros > start_micros`.
2. Reject windows wider than `31 * 24 * 3600 * 1_000_000` micros with `400 Bad Request` and a message naming the cap.
3. Load the pipeline definition for `id` scoped to the caller's org (404 if not found).
4. Synthesize a `QueryRequest` combining the pipeline's source query and the user-supplied time window.
5. Insert a `search_jobs` row referencing the pipeline id and return `{job_id, monitor: "/api/v1/query/jobs/{id}"}` with HTTP `202 Accepted`.

#### Scenario: Valid window queues backfill

- **WHEN** an OrgAdmin POSTs `{start_micros: 1730000000000000, end_micros: 1730086400000000}` to `/api/v1/scheduled_pipelines/p1/backfill`
- **THEN** the response is `202 Accepted` with `{job_id: "<ksuid>", monitor: "/api/v1/query/jobs/<ksuid>"}`
- **AND** the inserted `search_jobs` row has `request_json` containing `pipeline_id = 'p1'`

#### Scenario: Window too wide rejected

- **WHEN** an OrgAdmin POSTs a window of `60` days
- **THEN** the response is `400 Bad Request` with `{ "error": "invalid_argument", "message": "backfill window must be <= 31 days" }`

#### Scenario: Inverted window rejected

- **WHEN** the request has `end_micros <= start_micros`
- **THEN** the response is `400 Bad Request`

#### Scenario: Cancellation rides on search-job cancel

- **WHEN** an OrgAdmin POSTs `/api/v1/query/{job_id}/cancel` for the returned `job_id`
- **THEN** the backfill aborts at the next batch boundary (same behavior as any search-job cancel from `query-runtime-control`)
