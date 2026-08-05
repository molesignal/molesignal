## ADDED Requirements

### Requirement: Dashboard CRUD

The system SHALL expose `GET/POST /api/v1/dashboards`, `GET/PUT/DELETE /api/v1/dashboards/:id`, backed by `DashboardRepository`, with `org_id` derived from the authenticated session.

#### Scenario: Create and round-trip
- **WHEN** an Editor POSTs a dashboard with title `"Latency"`
- **THEN** a follow-up GET on the returned id yields the same `model`, `title`, `tags`, `folder_id`, with `version = 1` and `created_by` set to the caller

#### Scenario: Update increments version
- **WHEN** an Editor PUTs an updated `model`
- **THEN** the stored `version` becomes the prior version + 1 and `updated_by` is set to the caller

### Requirement: Folder CRUD

The system SHALL expose `GET/POST /api/v1/folders` and `DELETE /api/v1/folders/:id`, refusing deletion when dashboards still reference the folder.

#### Scenario: Delete blocked when not empty
- **WHEN** a folder still owns at least one dashboard
- **THEN** the response is `409 Conflict` with `{ "error": "folder not empty" }` and the folder remains

### Requirement: Grafana JSON Import

`POST /api/v1/dashboards/import/grafana` SHALL accept a body `{ "json": "<grafana dashboard json>", "folder_id": "..." }`, parse it via `DashboardModel::from_grafana_json`, create a new `Dashboard`, and return its server-assigned `id` and `uid`.

#### Scenario: Round-trip preserves unknown fields
- **WHEN** an imported dashboard JSON contains panel fields not modeled explicitly (collected by `#[serde(flatten)] extra`)
- **THEN** GET-ing the dashboard and serializing back to JSON yields a document that includes those same fields verbatim

#### Scenario: Invalid JSON rejected
- **WHEN** the body's `json` field is not valid Grafana JSON
- **THEN** the response is `400 Bad Request` with `{ "error": "invalid grafana json: ..." }` and nothing is persisted

#### Scenario: UID assignment
- **WHEN** the imported model has an empty `uid`
- **THEN** the system generates a stable `uid` and uses it for the new row
