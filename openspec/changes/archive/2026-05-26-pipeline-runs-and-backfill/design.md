## Context

The scheduler under `crates/app/src/pipeline/` already executes scheduled pipelines on a tick interval but doesn't record per-run history. The frontend has placeholder pages because nothing on the backend exposes runs. Backfill is conceptually a one-shot scheduled-pipeline run over a user-chosen time window — it can reuse the existing search-job submission flow.

Backend layers in this repo (familiar from `backend-settings-endpoints`):

```
crates/
├── shared
├── domain
├── infra        # Pg repos + migrations
├── app          # services
└── api          # axum routes + middleware
```

## Goals / Non-Goals

**Goals:**
- Two endpoints reachable in OSS, OrgAdmin+ on write.
- One new Postgres migration (`pipeline_runs`); scheduler writes a row per tick.
- Backfill returns `{job_id, monitor}` reusing the search-job pattern so the front-end poll loop already works.
- Frontend drops `awaitingBackend` from `pipelines/History.tsx` and `pipelines/Backfill.tsx`.

**Non-Goals:**
- No new scheduler engine. Existing pipeline-tick code stays unchanged except for a single `record_run` call at the boundary.
- No backfill cancellation UI — cancellation rides on `query/{id}/cancel` from `backend-settings-endpoints` since backfill is a search-job under the hood.
- No retention policy job for `pipeline_runs` in this change. The table grows unbounded; a follow-up can add an MV / rollup if it becomes an issue.

## Decisions

### D1: `pipeline_runs` table shape

```
CREATE TABLE pipeline_runs (
  id                 TEXT PRIMARY KEY,
  pipeline_id        TEXT NOT NULL,
  org_id             TEXT NOT NULL,
  state              TEXT NOT NULL,             -- 'running' | 'succeeded' | 'failed' | 'cancelled'
  started_at_micros  BIGINT NOT NULL,
  finished_at_micros BIGINT,
  scanned_rows       BIGINT NOT NULL DEFAULT 0,
  error              TEXT
);
CREATE INDEX idx_pipeline_runs_pipeline_started
  ON pipeline_runs(pipeline_id, started_at_micros DESC);
```

The list endpoint paginates by `(pipeline_id, started_at_micros DESC) LIMIT N` with optional `before_micros` cursor.

Alternative considered: a generic `task_runs` polymorphic table. Rejected — pipeline runs are the only writer in scope; another polymorphic abstraction adds joins for nothing.

### D2: Scheduler write boundary

The existing scheduler entry point (e.g. `PipelineService::run_one`) gets a `Guard` style RAII insert: row inserted with `state = 'running'` before the work begins, updated to `'succeeded' / 'failed'` on exit. Cancel handling rides on the existing `Result<()>` return — `Err::cancelled(...)` maps to `state = 'cancelled'`.

Alternative considered: write only on completion. Rejected — losing "currently running" visibility hides hung jobs from operators.

### D3: Backfill is a search-job under the hood

`POST /scheduled_pipelines/{id}/backfill { start_micros, end_micros }` flow:

1. Validate window: `end > start`, `end - start <= 31 days`. 400 on violations.
2. Load the pipeline via `scheduled_pipelines.get`.
3. Synthesize a `QueryRequest` from the pipeline's source query + the user window.
4. Insert into `search_jobs` with `state = pending`, `request_json` carrying the synthesized request, plus `pipeline_id` reference in the `request_json` body.
5. Return `{job_id, monitor: "/api/v1/query/jobs/{id}"}`.

The existing async search-job worker (already runs in the OSS bootstrap) picks it up and runs it. No new worker.

Alternative considered: dedicated `backfill_jobs` table. Rejected — duplicates search-job lifecycle entirely.

### D4: Window cap = 31 days (configurable later)

Hardcoded constant in the handler. A 31-day window over high-volume streams is already a non-trivial query — keeping the cap visible in code (not in config) reduces surprise. A config knob can land alongside an admin-tunable change if the cap becomes a frequent pain point.

### D5: AppState injection

`AppState.pipeline_runs: Arc<dyn PipelineRunRepository>`. The scheduler crate (`crates/app/src/pipeline`) takes the repo via its service constructor (no AppState dep). The HTTP layer reads via AppState.

## Risks / Trade-offs

**[R1] Scheduler write path latency**
→ Mitigation: single INSERT per tick; INSERT happens on the existing scheduler executor (already off the request path). Bench is well under 1 ms on Postgres.

**[R2] Unbounded `pipeline_runs` growth**
→ Mitigation: index is `(pipeline_id, started_at_micros DESC)` so list queries stay fast even at millions of rows. A retention rollup is a follow-up.

**[R3] Backfill window cap is hardcoded**
→ Mitigation: returns clear 400 with the cap in the message; admin docs note the limit. Follow-up promotes to config if abuse is observed.

**[R4] Cancel propagation for backfill**
→ Mitigation: backfill is a search-job, so the existing `POST /query/{id}/cancel` works (from `backend-settings-endpoints`). No new cancellation surface.

## Migration Plan

1. Land migration + repo + scheduler write hook + AppState wire-up first (backwards-compatible — `pipeline_runs` is a new table; existing scheduler keeps working).
2. Land list + backfill HTTP handlers.
3. Land frontend wiring (drop `awaitingBackend`); update `docs/web/sitemap-diff.md` if it tracks these rows.
4. Run `cargo check / test`, `pnpm -C web typecheck / lint / test:run`, `openspec validate ... --strict`.

Rollback: each commit is its own; the migration is additive, so reverting the route module alone restores the prior "awaiting backend" surface without data loss.
