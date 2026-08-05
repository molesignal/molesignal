# Web Pipeline Editor Capability

## Purpose

Provides the web UI for creating, editing, importing/exporting, and operating data pipelines (sources → VRL transform → sinks), including run history and backfill.

## Requirements

### Requirement: Pipeline Visual Editor

The web app SHALL expose `/pipelines/new` and `/pipelines/:id/edit` for creating and editing pipelines via a full-page orchestration workbench (sources → VRL transform → sinks). The form persists via `POST /api/v1/scheduled_pipelines` / `PUT /api/v1/scheduled_pipelines/:id`.

#### Scenario: Edit existing pipeline

- **WHEN** the user opens `/pipelines/abc/edit`
- **THEN** the editor loads via `GET /api/v1/scheduled_pipelines/abc`
- **AND** prefills source list, VRL body, sink list, retry policy
- **AND** "Save" PUTs back with the updated body

### Requirement: Pipeline Health List And Detail

The web app SHALL keep `/pipelines` focused on cross-pipeline runtime health and SHALL open a
dedicated `/pipelines/:id` detail page. The detail page SHALL separate overview, topology, run
history, and configuration into tabs instead of stacking them in the list page.

#### Scenario: Open a pipeline from the health list

- **WHEN** the user clicks a row on `/pipelines`
- **THEN** the app navigates to `/pipelines/:id`
- **AND** the list response supplies the latest run state plus real 24-hour run counters
- **AND** the detail page loads runtime history from `/scheduled_pipelines/:id/runs`

### Requirement: Pipeline Import / Export

The web app SHALL expose `/pipelines/import` accepting a YAML / JSON pipeline definition (Vector-compatible) and persisting it via `POST /api/v1/scheduled_pipelines/import`. The list view SHALL include an "Export" action that downloads the pipeline as YAML.

#### Scenario: Import Vector YAML

- **WHEN** the user pastes a Vector config YAML and clicks Import
- **THEN** the page POSTs to `/api/v1/scheduled_pipelines/import`
- **AND** on success navigates to `/pipelines/<new-id>/edit` with the parsed config prefilled

### Requirement: Pipeline Run History

The web app SHALL expose `/pipelines/:id/history` listing recent pipeline executions (start time, duration, records_in / records_out / errors, status). Pulls from `GET /api/v1/scheduled_pipelines/:id/runs`.

#### Scenario: History shows last 50 runs

- **WHEN** the user opens `/pipelines/abc/history`
- **THEN** the page issues `GET /api/v1/scheduled_pipelines/abc/runs?limit=50`
- **AND** failed runs are highlighted red with error message expandable

#### Scenario: Backend endpoint pending

- **WHEN** the backend has not yet implemented `/scheduled_pipelines/:id/runs`
- **THEN** the page renders an "Awaiting backend endpoint" empty state with a link to the tracking issue
- **AND** typecheck / lint still pass (the request fails gracefully via the standard 404 path)

### Requirement: Pipeline Backfill

The web app SHALL expose `/pipelines/:id/backfill` with a time-range picker that triggers a historical replay via `POST /api/v1/scheduled_pipelines/:id/backfill` with `{ from, to }`.

#### Scenario: Backfill the last 7 days

- **WHEN** the user picks a 7-day window and clicks Backfill
- **THEN** the page POSTs `{ from: -7d, to: now }` to the backfill endpoint
- **AND** the response includes a `job_id` linked to `/pipelines/abc/history?run=<job_id>`
