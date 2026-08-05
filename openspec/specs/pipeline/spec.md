# Pipeline Capability

## Purpose

Function / Pipeline / Extend / ScheduledPipeline / SavedView 的 CRUD 与运行时：在 ingest 时通过 VRL/JS 函数链对事件做转换、富化、过滤、加密，并支持周期性批处理与已存查询。

## Requirements

### Requirement: Function CRUD

The system SHALL expose `GET/POST /api/v1/functions` and `GET/PUT/DELETE /api/v1/functions/:id` backed by `FunctionRepository`. A `Function` carries `{ id, org_id, name, language: "vrl" | "js", source, params_schema?, created_at, updated_at, version }`. Each successful update SHALL bump `version` monotonically.

#### Scenario: Create VRL function
- **WHEN** an Editor POSTs `{ "name": "redact_email", "language": "vrl", "source": "...VRL program..." }`
- **THEN** the response is `201 Created` with the persisted function (server-assigned `id`, `version = 1`), and the source is compiled once at create time so syntax errors are rejected with `400 Bad Request` and body `{ "error": "vrl compile failed: <message>" }`

#### Scenario: Update bumps version
- **WHEN** a PUT changes the `source`
- **THEN** the new row has `version = previous + 1`, the cached compiled program for the previous version is invalidated, and any in-flight pipeline using the old version completes its current batch on the old compiled code before switching

#### Scenario: JS language gated by feature flag
- **WHEN** a POST sets `"language": "js"` and the server was not built with the `js` feature
- **THEN** the response is `400 Bad Request` with `{ "error": "js function runtime not enabled in this build" }`

### Requirement: Pipeline CRUD

The system SHALL expose `GET/POST /api/v1/pipelines` and `GET/PUT/DELETE /api/v1/pipelines/:id` backed by `PipelineRepository`. A `Pipeline` carries `{ id, org_id, name, stream_targets: Vec<StreamTarget>, steps: Vec<{ function_id, params }>, enabled, created_at, updated_at }`. `StreamTarget = { stream_name, stream_type }`. The same stream MAY belong to at most one enabled pipeline per org; violating this returns `409 Conflict`.

#### Scenario: Create pipeline targeting two streams
- **WHEN** a POST targets `[{name: "app", type: "logs"}, {name: "nginx", type: "logs"}]` with two steps
- **THEN** the response is `201 Created` and subsequent ingests to either stream pass through the pipeline before WAL append

#### Scenario: Conflicting target rejected
- **WHEN** stream `app` is already in an enabled pipeline and a new pipeline targets `app` with `enabled = true`
- **THEN** the response is `409 Conflict` with `{ "error": "stream app already pipelined" }`

### Requirement: Pipeline Execution At Ingest Time

`IngestService::ingest` SHALL, before any schema validation, look up the enabled pipeline for the target stream, run each step's function on every event in order via the matching `FunctionRuntime`, and treat transform failures the same as schema-validation failures (event added to `rejected` with reason).

#### Scenario: Successful transform applied
- **WHEN** a batch of 100 events targets a stream with a one-step pipeline that lowercases `level`
- **THEN** all 100 events emerge with `level` lowercased before WAL append, the response shows `accepted: 100, rejected: 0`

#### Scenario: Single event transform error
- **WHEN** 1 of 100 events causes a VRL runtime error (e.g., type mismatch on undefined field)
- **THEN** the response is `accepted: 99, rejected: 1, errors: [{ index: 17, reason: "pipeline step 0 (fn redact_email): vrl runtime: ..." }]` and the other 99 events are written

#### Scenario: Sandbox limit exceeded
- **WHEN** a JS function exceeds `pipeline.wall_time_ms_limit` for a single event
- **THEN** that event is added to `rejected` with reason `"pipeline step X: exceeded wall_time_ms_limit"` and `pipeline_function_limit_exceeded_total{function_id, reason="wall_time"}` increments

### Requirement: Extend Table Lookup

The system SHALL support stream_type `extend`; rows in such streams have schema `(key TEXT, value JSONB)`. On ingester startup (and on extend-stream update), the table is materialized into an in-memory `HashMap<String, serde_json::Value>` keyed by `key`. A VRL built-in function `lookup(table_name, key)` SHALL return the matched value (or `null`) for use in pipeline steps.

#### Scenario: Create extend table
- **WHEN** an Editor POSTs `POST /api/v1/streams { name: "geo_ip", stream_type: "extend", schema: [{name:"key",type:"Utf8"},{name:"value",type:"Json"}] }` then ingests rows via `/api/v1/ingest/extend/geo_ip`
- **THEN** the rows are stored and the in-memory map is refreshed for extend lookup

#### Scenario: VRL lookup hits extend
- **WHEN** a VRL step is `. = merge(., lookup("geo_ip", .client_ip))`
- **THEN** the matched value is merged into the event; misses return `null` and the event is unchanged

### Requirement: Scheduled Pipeline Runner

The system SHALL persist scheduled pipelines in `scheduled_pipelines { id, org_id, name, source_stream, target_stream, function_steps_json, cron, lookback_secs, last_run_at?, enabled }` and run them via a `ScheduledPipelineRunner` task in the `alert_manager` role (default cron parser: `cron` crate). Each run reads `[now - lookback_secs, now]` from the source stream via the standard query path, applies the function steps row-by-row, and re-ingests into the target stream.

#### Scenario: Hourly rollup pipeline runs
- **WHEN** a scheduled pipeline with `cron = "0 * * * *"` and lookback 3600s fires
- **THEN** the runner queries `[now-1h, now]` of `source_stream`, transforms each row, and writes outputs to `target_stream`; `scheduled_pipeline_runs_total{id, status="ok"} += 1`

#### Scenario: Failed run retried on next tick
- **WHEN** the function chain panics partway
- **THEN** the run aborts with `last_error` recorded; the next cron tick attempts again from the current window (no replay backfill in the current implementation)

### Requirement: Saved View CRUD and Run

The system SHALL expose `GET/POST /api/v1/saved_views`, `GET/PUT/DELETE /api/v1/saved_views/:id`, and `POST /api/v1/saved_views/:id/run` backed by `SavedViewRepository`. A `SavedView` carries `{ id, org_id, owner_user_id, name, language: "sql" | "promql", statement, default_time_range_secs, stream?, tags: Vec<String>, pinned: bool, created_at, updated_at }`. Run requests accept optional `?end=<ts>&step=<secs>` to override the saved time range.

#### Scenario: Saved view runs as standard query
- **WHEN** a user POSTs `/api/v1/saved_views/:id/run` for a saved SQL view
- **THEN** the handler reads the row, builds a `QueryRequest { language, statement, time_range: [end - default_time_range_secs, end], stream }` (with `end` defaulting to `now`), and dispatches through the normal `QueryService::run` path, returning the same `QueryResult` shape with `cache_hit` honored

#### Scenario: Cross-org saved view returns 404
- **WHEN** a user from `orgA` requests a saved view owned by `orgB`
- **THEN** the response is `404 Not Found` and no row is returned

#### Scenario: Pin/unpin via PUT
- **WHEN** the owner PUTs `{ "pinned": true }`
- **THEN** the row's `pinned = true`; list responses honor a `?pinned=true` filter

### Requirement: Scheduled Pipeline Runs Recorded

The system SHALL record every scheduled-pipeline execution attempt (success, failure, or cancellation) into the new `pipeline_runs` table defined by the `pipeline-runs` capability. The recording SHALL happen at the boundary of `PipelineService::run_one` (or equivalent existing scheduler entry point) and SHALL NOT change the existing pipeline scheduling, retry, or dispatch semantics.

#### Scenario: Tick records a run row

- **WHEN** the scheduler executes a tick for pipeline `p1`
- **THEN** a row is inserted into `pipeline_runs` with `pipeline_id = 'p1'` and `state = 'running'` before the work begins
- **AND** the row is updated to `state = 'succeeded' | 'failed' | 'cancelled'` when the tick ends
- **AND** the existing pipeline outputs (downstream stream writes, alert dispatches, etc.) are unchanged
