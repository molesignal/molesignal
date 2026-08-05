## ADDED Requirements

### Requirement: Audit Event Persistence

The system SHALL persist an `audit_events` row for every mutating HTTP handler invocation (POST/PUT/DELETE/PATCH on `/api/v1/*` excluding `/api/v1/ingest/*` and `/api/v1/query`), every successful or failed login, and every API-token issuance or revocation. Each row carries `{ id, org_id, actor_kind: "user"|"token"|"system", actor_id, action, target_kind, target_id, ip, user_agent, payload_json, status_code, ts }`. Writes SHALL be best-effort (fail-open if DB unavailable; metric `audit_write_failures_total` increments) so a degraded audit does not block the user action.

#### Scenario: Dashboard create generates audit row
- **WHEN** an Editor successfully creates a dashboard
- **THEN** an `audit_events` row exists with `action = "dashboard.create", actor_id = <user_id>, status_code = 201`

#### Scenario: Failed login still audited
- **WHEN** a login attempt returns `401 Unauthorized` on bad password
- **THEN** an `audit_events` row exists with `action = "auth.login", status_code = 401, actor_kind = "system", actor_id = "anonymous"`, `payload_json.email = <attempted_email>`

#### Scenario: Ingest path NOT audited per-call
- **WHEN** 1,000 `/api/v1/ingest/logs/app` calls are issued
- **THEN** no `audit_events` rows are written for them (volume too high; ingestion metrics suffice)

### Requirement: Audit Query Endpoint

The system SHALL expose `GET /api/v1/audit?from=&to=&actor=&action=&target_kind=&page=&page_size=` returning paginated `audit_events` rows; access is gated by `Permission::AuditRead` (granted to `Admin` and `Owner` roles).

#### Scenario: Owner queries last 24h audit
- **WHEN** an Owner GETs `/api/v1/audit?from=now-24h&to=now`
- **THEN** the response is `200 OK` with `{ items, total, page, page_size }`, ordered by `ts DESC`

#### Scenario: Viewer rejected
- **WHEN** a Viewer GETs `/api/v1/audit`
- **THEN** the response is `403 Forbidden`
