## MODIFIED Requirements

### Requirement: Audit Query Endpoint

The system SHALL expose `GET /api/v1/audit?from=&to=&actor_kind=&actor=&action=&target_kind=&target_id=&limit=&cursor=` returning cursor-paginated `audit_events` rows ordered by `ts DESC, id DESC`; access is gated by `Permission::AuditRead` granted to Admin and Owner roles. The response SHALL include `{ items, next_cursor }`. `cursor` SHALL be opaque to clients and encode the last `(ts_micros, id)` pair needed for stable pagination.

#### Scenario: Owner queries last 24h audit

- **WHEN** an Owner GETs `/api/v1/audit?from=now-24h&to=now`
- **THEN** the response is `200 OK` with `{ items, next_cursor }`, ordered by `ts DESC, id DESC`

#### Scenario: Admin filters AI provider events

- **WHEN** an Admin GETs `/api/v1/audit?action=ai.provider.rotate_key&target_kind=ai_provider`
- **THEN** the response contains only matching events in the caller's org

#### Scenario: Cursor fetches next page

- **WHEN** the first audit query returns `next_cursor = C`
- **AND** the client GETs `/api/v1/audit?cursor=C&limit=50`
- **THEN** the response returns the next page after the previous page's final `(ts_micros, id)` row

#### Scenario: Viewer rejected

- **WHEN** a Viewer GETs `/api/v1/audit`
- **THEN** the response is `403 Forbidden`

## ADDED Requirements

### Requirement: AI lifecycle audit events

The system SHALL record audit events for AI provider, prompt, chat, tool-call, and archive lifecycle operations. Audit payloads SHALL include stable identifiers, target kinds, status, prompt version/hash when relevant, object keys/checksums for archived artifacts, and sanitized metadata. Audit payloads MUST NOT contain plaintext provider keys, full prompts when they exceed the audit payload limit, raw tool result bodies, or full chat transcripts.

#### Scenario: Prompt update audited

- **WHEN** an Admin updates an org-scoped prompt template
- **THEN** an audit event exists with `action = "ai.prompt.update"`, `target_kind = "ai_prompt"`, `target_id = <prompt_id>`, and payload containing version and rendered-template hash metadata

#### Scenario: Tool call audited without raw rows

- **WHEN** anomaly chat calls `query_logs`
- **THEN** an audit event exists with `action = "ai.tool.call"`, `target_kind = "ai_chat_session"`, status metadata, tool name, row count, and optional evidence object key
- **AND** the audit payload does not contain raw log rows

#### Scenario: Provider key excluded from audit

- **WHEN** an Admin creates or rotates an AI provider key
- **THEN** the audit payload contains provider id and masked key metadata
- **AND** the audit payload does not contain the plaintext key or encrypted ciphertext bytes
