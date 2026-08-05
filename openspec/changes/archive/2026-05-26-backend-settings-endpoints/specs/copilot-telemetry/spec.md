## ADDED Requirements

### Requirement: AI Toolset Registry CRUD

The system SHALL expose `GET /api/v1/ai_toolsets`, `POST /api/v1/ai_toolsets`, and `DELETE /api/v1/ai_toolsets/{id}` backed by an `AiToolsetRepository` trait. The trait has two impls:

- `EmptyAiToolsetRepository` (OSS default) — `list` returns `Ok(vec![])`; `create` / `delete` return `Err::forbidden("ai_toolsets requires enterprise license")`.
- `PgAiToolsetRepository` (enterprise crate `enterprise/crates/ai_toolsets/`) — backed by an `ai_toolsets` Postgres table with `(id TEXT PK, org_id TEXT, name TEXT, schema JSONB, enabled BOOL, created_at_micros, updated_at_micros, UNIQUE(org_id, name))`.

All routes require `OrgAdmin+`. The OSS `GET` succeeds with an empty list so the Settings UI can render "no toolsets" without 403'ing.

#### Scenario: OSS GET returns empty list

- **WHEN** the OSS build is running and an OrgAdmin GETs `/api/v1/ai_toolsets`
- **THEN** the response is `200 OK` with `[]`

#### Scenario: OSS write rejected

- **WHEN** an OrgAdmin POSTs to `/api/v1/ai_toolsets` on the OSS build
- **THEN** the response is `403 Forbidden` with `{ "error": "ai_toolsets requires enterprise license" }`

#### Scenario: Enterprise create persists

- **WHEN** the enterprise build is running and an Admin POSTs `{ name: "logs.search", schema: {...}, enabled: true }`
- **THEN** the row persists in `ai_toolsets` and is returned by subsequent GETs
