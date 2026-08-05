## ADDED Requirements

### Requirement: Query plan dump endpoint

The system SHALL expose `POST /api/v1/query/inspect` accepting the same body as `/query` and returning the DataFusion logical + physical plan as JSON, plus per-stage estimated cost, without executing the query.

#### Scenario: Inspect returns plan only

- **WHEN** a user POSTs a SQL query to `/api/v1/query/inspect`
- **THEN** the response carries `{ "logical_plan": "<text>", "physical_plan": "<text>", "estimated_cost": <num>, "executed": false }`

### Requirement: Execution profile capture

When a query is executed via `POST /api/v1/query?inspect=true`, the system SHALL collect per-stage wall time + rows scanned + bytes read and return them in `meta.profile` alongside results.

#### Scenario: Profile attached

- **WHEN** a query runs with `?inspect=true`
- **THEN** the response carries `meta.profile: { stages: [{ name, wall_ms, rows_scanned, bytes_read }] }`
