# Audit Capability

## Purpose

对所有可变操作与登录/Token 事件落 `audit_events` 行，best-effort 写入避免影响业务路径，Owner/Admin 可分页查询。

## Requirements

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

### Requirement: Audit events for auth secret operations

The audit middleware SHALL persist `audit_events` rows for:
- `action = "jwt.rotate"` —— on successful `POST /api/v1/auth/jwt/rotate`; payload includes `{ new_kid, retired_kid }`; actor = caller's user_id
- `action = "api_token.issue"` —— on successful `POST /api/v1/auth/tokens`; payload `{ token_id, prefix, role, expires_at_micros? }` (NO secret in payload)
- `action = "api_token.revoke"` —— on successful `DELETE /api/v1/auth/tokens/{id}`; payload `{ token_id, prefix }`

#### Scenario: jwt.rotate audit row created

- **WHEN** an Owner POSTs `/api/v1/auth/jwt/rotate` and returns 200
- **THEN** an audit_events row is inserted within 1 second with `action = "jwt.rotate"`, `actor_id = <Owner user_id>`, payload includes both `new_kid` and `retired_kid`
- **AND** the row's `org_id` matches the Owner's org

#### Scenario: api_token issue audit excludes secret

- **WHEN** a user POSTs `/api/v1/auth/tokens` and gets back plaintext token
- **THEN** the audit_events row's `payload` field SHALL contain `prefix` but SHALL NOT contain the secret portion or the full `ms_*` string

#### Scenario: api_token revoke audit

- **WHEN** an Owner DELETEs `/api/v1/auth/tokens/<id>`
- **THEN** an audit_events row with `action = "api_token.revoke"` and payload `{ token_id, prefix }` is written
