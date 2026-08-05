## ADDED Requirements

### Requirement: API Token Issuance and Authentication

The system SHALL expose `GET/POST /api/v1/auth/tokens` and `DELETE /api/v1/auth/tokens/:id`. A successful POST returns a one-time plaintext token of the form `ms_<16-char-id>_<32-char-secret>`; only the argon2id hash of the secret is persisted (column `api_tokens { id, org_id, user_id, prefix, secret_hash, role, expires_at?, last_used_at?, revoked, name, created_at }`). The auth middleware SHALL accept either a JWT or an `ms_*` token in `Authorization: Bearer <…>`; for API tokens it resolves `AuthContext { user_id, org_id, role }` from the row, updates `last_used_at` lazily (best-effort, no blocking write), and rejects revoked/expired tokens with `401 Unauthorized`.

#### Scenario: Token issued once
- **WHEN** a user POSTs `/api/v1/auth/tokens { name: "ci-ingest", role: "editor", expires_at?: ... }`
- **THEN** the response body is `{ id, prefix, name, role, expires_at, token: "ms_aBcD..._XyZ..." }`; subsequent GET of the same id returns the row WITHOUT the `token` field

#### Scenario: Token accepted by middleware
- **WHEN** a request carries `Authorization: Bearer ms_aBcD..._XyZ...`
- **THEN** the middleware looks up by prefix, verifies the secret against `secret_hash` (argon2id), injects `AuthContext`, and lets the request through

#### Scenario: Revoked token rejected
- **WHEN** an Owner DELETEs a token's id and then a request uses the same token
- **THEN** the response is `401 Unauthorized` with `{ "error": "token revoked" }`

## MODIFIED Requirements

### Requirement: User & Org Management

The system SHALL expose CRUD for users (`GET/POST /api/v1/users`, `GET/PUT/DELETE /api/v1/users/:id`), organizations (`GET/POST /api/v1/orgs`, `GET/PUT/DELETE /api/v1/orgs/:id`), memberships (`GET/POST /api/v1/orgs/:id/members`, `DELETE /api/v1/orgs/:id/members/:user_id`), and teams (`GET/POST /api/v1/teams`, `GET/PUT/DELETE /api/v1/teams/:id`), backed by their respective repositories. User creation SHALL hash passwords with argon2id before storing. Every protected read/list SHALL filter rows to the caller's `org_id`; cross-org lookups by `:id` return `404 Not Found` rather than `403 Forbidden` to avoid existence enumeration.

#### Scenario: Password is hashed at rest
- **WHEN** an Owner creates a new user with password `"hunter2"`
- **THEN** the stored `password_hash` is an argon2 string starting with `$argon2id$` and is never equal to the plain password

#### Scenario: First user becomes Owner of default org
- **WHEN** the user table is empty and a user is created via `POST /api/v1/users`
- **THEN** the system also creates a default organization and inserts a `Membership { role: Owner }` for that user in the same transaction; subsequent `POST /api/v1/users` calls require an existing Owner's JWT and do NOT auto-create orgs

#### Scenario: Cross-org user fetch returns 404
- **WHEN** a member of `orgA` requests `GET /api/v1/users/<id_from_orgB>`
- **THEN** the response is `404 Not Found` with `{ "error": "user not found" }`

#### Scenario: Team within org isolated
- **WHEN** a member of `orgA` lists `/api/v1/teams`
- **THEN** only teams whose `org_id = orgA` are returned, regardless of how many teams exist in other orgs

#### Scenario: Membership remove requires Owner role
- **WHEN** a Viewer-role caller issues `DELETE /api/v1/orgs/:id/members/:user_id`
- **THEN** the response is `403 Forbidden` and the membership is unchanged
