## ADDED Requirements

### Requirement: Issue API token endpoint

The system SHALL expose `POST /api/v1/auth/tokens` accepting `{ name, role?, expires_in_days? }` and returning the **one-time plaintext** token of form `ms_<16-prefix>_<32-secret>`. Only the argon2 hash of secret is persisted. The role defaults to the caller's role; cannot exceed it.

#### Scenario: Issue token returns plaintext once

- **WHEN** an authenticated user POSTs `{ "name": "ci-deploy", "role": "editor", "expires_in_days": 365 }`
- **THEN** response is `201 { id, token: "ms_aB3kZ1xT9pQrU7nM_dFg...", prefix: "aB3kZ1xT9pQrU7nM", role: "editor", expires_at_micros }`
- **AND** the `token` field appears only this once; subsequent GET returns no plaintext

#### Scenario: Role escalation rejected

- **WHEN** a user with role `Viewer` POSTs `{ "role": "Owner" }`
- **THEN** response is `403 Forbidden` with body `{"error": "token role cannot exceed caller role"}`

### Requirement: List tokens endpoint

The system SHALL expose `GET /api/v1/auth/tokens` returning the caller's org tokens with `{ id, prefix, name, role, expires_at_micros?, last_used_at_micros?, revoked, created_at_micros }`. The secret hash MUST NOT be returned in any form.

#### Scenario: List omits secret

- **WHEN** a user GETs `/api/v1/auth/tokens`
- **THEN** response items include `prefix` but no `secret`, `secret_hash`, or full token
- **AND** revoked tokens still appear (with `revoked: true`)

### Requirement: Revoke token endpoint

The system SHALL expose `DELETE /api/v1/auth/tokens/{id}` setting `revoked=TRUE`. Once revoked, subsequent middleware requests using this token return 401.

#### Scenario: Revoke takes effect immediately

- **WHEN** a token T was previously working
- **AND** an Owner DELETEs `/api/v1/auth/tokens/<id>`
- **THEN** the next request with `Authorization: Bearer ms_<T>` returns 401 within 5 seconds (cache invalidation grace)

### Requirement: Middleware accepts `ms_*` Bearer tokens

The auth middleware SHALL recognize `Authorization: Bearer ms_<prefix>_<secret>` and:
1. Look up `api_tokens` by `prefix` (DB unique index — O(1)).
2. Verify secret with argon2; mismatch → 401.
3. Check `revoked=FALSE` and `expires_at_micros IS NULL OR > now`; else 401.
4. Inject `AuthContext { user_id, org_id, role }` from row.
5. Async update `last_used_at_micros` via `tokio::spawn` (best-effort, does not block response).

#### Scenario: Valid API token authenticates

- **WHEN** a request carries `Authorization: Bearer ms_<valid prefix and secret>`
- **THEN** middleware injects `AuthContext` matching the token's row and request proceeds

#### Scenario: Expired API token rejected

- **WHEN** the token's `expires_at_micros < now`
- **THEN** middleware returns 401 with body `{"error": "token expired"}`

#### Scenario: Tampered secret rejected

- **WHEN** a request carries a valid prefix but garbage secret
- **THEN** argon2 verify fails; middleware returns 401

### Requirement: Token storage uses argon2id

The system SHALL hash secret with argon2id (existing `argon2` workspace dep) at insert time. The raw secret SHALL NOT be persisted, logged, or returned outside the create response.

#### Scenario: secret_hash is argon2 PHC string

- **WHEN** a token is inserted into `api_tokens`
- **THEN** the `secret_hash` column contains a string starting with `$argon2id$v=19$...`
