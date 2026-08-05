## ADDED Requirements

### Requirement: Action CRUD

The system SHALL expose `/api/v1/actions` for create / list / get / update / delete of executable actions. Each action carries `{ id, org_id, name, kind: webhook|script|workflow, config, enabled }`. Action operations require `license.has_feature("actions")`; OSS build returns 403.

#### Scenario: OSS build returns 403

- **WHEN** the binary is OSS (no `enterprise` feature) and a user GETs `/api/v1/actions`
- **THEN** the system returns 403 with body `{ "error": "actions feature not licensed" }`

### Requirement: Action invocation from alert

Alert escalation policy steps SHALL be able to reference an action by id. When the step fires, the dispatcher SHALL invoke the action with the incident as context `{ rule, incident, value, threshold, evaluated_at }` and record the result in `action_executions` table.

#### Scenario: Webhook action fires on incident

- **WHEN** an alert escalation step `{ kind: "action", action_id: "a1" }` triggers
- **AND** action `a1` is `{ kind: "webhook", config: { url, headers } }`
- **THEN** the system POSTs the rendered context to the URL and records `action_executions { status: success|failed, http_status, response_body, duration_ms }`

### Requirement: Script execution sandbox

Actions of kind `script` SHALL execute in a sandboxed runtime (initial implementation: VRL-only; future: Lua / wasm). CPU time / memory / wall time limits SHALL be enforced.

#### Scenario: Script timeout enforced

- **WHEN** a script action exceeds the wall_time_ms limit
- **THEN** execution is aborted, `action_executions.status` is `failed` with reason `timeout`
