## Why

`web-feature-parity` shipped two Pipeline pages — `pipelines/History.tsx` and `pipelines/Backfill.tsx` — that today render an `EmptyState awaitingBackend` because the corresponding endpoints (`GET /api/v1/scheduled_pipelines/{id}/runs` and `POST /api/v1/scheduled_pipelines/{id}/backfill`) were never built. Pipeline operators have no way to inspect prior runs or re-process a time window from the UI without using SQL directly against the underlying tables.

## What Changes

### New endpoints

- `GET /api/v1/scheduled_pipelines/{id}/runs` — return the recent N runs of a pipeline (id / pipeline_id / org_id / state / started_at_micros / finished_at_micros / scanned_rows / error). OrgAdmin+ for write paths; StreamRead for this list path.
- `POST /api/v1/scheduled_pipelines/{id}/backfill` — submit a backfill job over a `{start_micros, end_micros}` window. Returns `{job_id, monitor}` (reusing the search-job submission shape so the existing front-end poll loop works). OrgAdmin+.

### Backend impl

- Add a `pipeline_runs` table (id, pipeline_id, org_id, state, started_at_micros, finished_at_micros, scanned_rows, error). The scheduler that already executes pipelines writes a row per attempt.
- Extend `ScheduledPipelineRepository` (or a new `PipelineRunRepository`) with `list_runs(org_id, pipeline_id, limit, before_micros)` + `record_run(row)`.
- Backfill route wraps the existing search-job submission path (`search_jobs.create`) with a synthetic `QueryRequest` derived from the pipeline definition and the user-supplied time window.

### Frontend wire-up

- `pipelines/History.tsx` drops `awaitingBackend` and renders the list returned by the new endpoint.
- `pipelines/Backfill.tsx` drops `awaitingBackend`, renders a date-range picker + `Submit` button, calls the new endpoint, and shows the returned `job_id` with a link to the existing search-job monitor route.

## Capabilities

### New Capabilities

- `pipeline-runs`: persistence + read API for pipeline execution history (`pipeline_runs` table + list endpoint).
- `pipeline-backfill`: HTTP submission of a one-shot backfill that runs the pipeline over a user-chosen time window, persisted as a search-job for monitoring.

### Modified Capabilities

- `pipeline`: existing capability gains the two HTTP endpoints. No behavioral change to the existing pipeline scheduler itself beyond writing a `pipeline_runs` row per attempt.

## Impact

- **Backend code**: new `pipeline_runs` table migration; new `PipelineRunRepository` (or extension on the existing repo); new HTTP handlers under `routes/scheduled_pipelines.rs` (same module, two new handlers).
- **State**: `AppState` gains `pipeline_runs: Arc<dyn PipelineRunRepository>`.
- **Scheduler write path**: every existing scheduled-pipeline tick records a run row. Cost ~1 INSERT per tick — negligible.
- **Frontend**: two TSX files lose their `awaitingBackend` blocks; one new tiny client `api/pipelineRuns.ts`.
- **i18n**: existing `pipelines` namespace keys are sufficient; add only what's missing for History columns + Backfill copy.
- **Risk**: backfill window can match a very large time range. Backend MUST cap the window (e.g. 31 days) and reject larger requests with `400`; document the cap in the spec.
- **OSS / enterprise**: pure OSS — no license gate.
