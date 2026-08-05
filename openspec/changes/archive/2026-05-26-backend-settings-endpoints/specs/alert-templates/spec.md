## ADDED Requirements

### Requirement: Alert Template CRUD

The system SHALL expose `GET /api/v1/alerts/templates`, `POST /api/v1/alerts/templates`, and `DELETE /api/v1/alerts/templates/{id}` backed by an `AlertTemplateRepository` over a Postgres `alert_templates` table with columns `(id TEXT PK, org_id TEXT, name TEXT, body TEXT, format TEXT DEFAULT 'text', created_at_micros BIGINT, updated_at_micros BIGINT, UNIQUE(org_id, name))`. All routes require `OrgAdmin+`; reads are scoped to the caller's `org_id`.

#### Scenario: List returns this org only

- **WHEN** an OrgAdmin GETs `/api/v1/alerts/templates`
- **THEN** the response is a JSON array of every template whose `org_id` matches the caller's org
- **AND** templates from other orgs are never included

#### Scenario: Create rejects duplicate names

- **WHEN** an Admin POSTs a template with a `name` that already exists in the same org
- **THEN** the response is `409 Conflict` with `{ "error": "template name already exists" }`
- **AND** no row is inserted

#### Scenario: Delete is idempotent

- **WHEN** an Admin DELETEs `/api/v1/alerts/templates/{id}` for an unknown id
- **THEN** the response is `200 OK` with `{ "deleted": true }` — repeated deletes succeed without error

### Requirement: Template Body Format

Each template carries a `format` field with allowed values `text | markdown | html`. Format is opaque to the backend (no rendering on create / list); the alert dispatch path consumes it later to pick the matching encoder. Unknown formats SHALL be rejected on create with `400 Bad Request`.

#### Scenario: Markdown format accepted

- **WHEN** an Admin POSTs `{ name: "incident", body: "**{{title}}** fired", format: "markdown" }`
- **THEN** the row persists with `format = 'markdown'`

#### Scenario: Bad format rejected

- **WHEN** an Admin POSTs `{ name: "x", body: "y", format: "yaml" }`
- **THEN** the response is `400 Bad Request` with `{ "error": "format must be one of: text | markdown | html" }`
