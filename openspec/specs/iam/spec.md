# IAM Capability

## Purpose

统一管理身份主体、组织上下文、成员关系、角色、权限、密码登录、JWT 校验，以及 API token（`ms_*` 长期凭据）的发放与撤销。

## Requirements

### Requirement: Password Login & JWT Issuance

`POST /api/v1/auth/login` SHALL accept `{ "email", "password" }`, look up the user via `UserRepository::get_by_email`, verify the password against the stored argon2 hash, and return `{ "token": "<JWT>" }` on success.

#### Scenario: Successful login
- **WHEN** the email matches and the password verifies
- **THEN** the response is `200 OK` with a JWT signed by the server's HS256 secret containing claims `{ sub: user_id, org_id, scope, iat, exp, iss }` and no role claim

#### Scenario: Bad credentials
- **WHEN** the email is not found OR the password does not verify
- **THEN** the response is `401 Unauthorized` with body `{ "error": "invalid credentials" }` regardless of which condition failed (no user-enumeration)

#### Scenario: Disabled user
- **WHEN** the user exists but `disabled = true`
- **THEN** the response is `403 Forbidden` with body `{ "error": "user disabled" }`

### Requirement: JWT Auth Middleware

Every request to `/api/v1/*` except `/api/v1/auth/login` and `/api/v1/healthz` SHALL pass through middleware that validates the `Authorization: Bearer <token>` header, resolves current server-side capabilities, and injects `IamContext { user_id, org_id, display_role, roles, scope, permissions, policy_version }`; missing/invalid tokens yield `401 Unauthorized`.

#### Scenario: Expired token
- **WHEN** a request carries a JWT whose `exp` is in the past
- **THEN** the response is `401 Unauthorized` with `{ "error": "token expired" }`

#### Scenario: Health endpoint stays public
- **WHEN** an unauthenticated request hits `/api/v1/healthz`
- **THEN** the response is `200 OK`

### Requirement: Capability-Based IAM Enforcement

Every protected handler SHALL declare a canonical IAM permission directly or through a typed compatibility mapping. The handler SHALL check the server-resolved capability set and MUST NOT treat a JWT/API-token display role as authorization. Organization role assignment SHALL come from IAM role bindings, while `_sys` role assignment SHALL come from the persisted platform-administrator assignment and its database purpose mapping. Role display names and effective permissions in both scopes SHALL be read from `iam_roles` and `iam_role_permissions`. Resource handlers SHALL additionally evaluate resource bindings, relationships, and cross-organization grants.

#### Scenario: Viewer cannot write
- **WHEN** a Viewer-role caller posts to `/api/v1/alerts/rules`
- **THEN** the response is `403 Forbidden`

### Requirement: API Token Issuance and Authentication

The system SHALL expose `GET/POST /api/v1/auth/tokens` and `DELETE /api/v1/auth/tokens/:id`. A successful POST returns a one-time plaintext token of the form `ms_<16-char-id>_<32-char-secret>`; only the argon2id hash of the secret is persisted (column `api_tokens { id, org_id, user_id, prefix, secret_hash, role_id, expires_at?, last_used_at?, revoked, name, created_at }`). The auth middleware SHALL accept either a JWT or an `ms_*` token in `Authorization: Bearer <…>`; for API tokens it resolves the referenced IAM role and its current database permissions, updates `last_used_at` lazily (best-effort, no blocking write), and rejects revoked/expired tokens with `401 Unauthorized`.

#### Scenario: Token issued once
- **WHEN** a user POSTs `/api/v1/auth/tokens { name: "ci-ingest", role_id: "<database role id>", expires_at?: ... }`
- **THEN** the response body is `{ id, prefix, name, role_id, role_key, role_name, expires_at, token: "ms_aBcD..._XyZ..." }`; subsequent GET of the same id returns the row WITHOUT the `token` field

#### Scenario: Token accepted by middleware
- **WHEN** a request carries `Authorization: Bearer ms_aBcD..._XyZ...`
- **THEN** the middleware looks up by prefix, verifies the secret against `secret_hash` (argon2id), injects `IamContext`, and lets the request through

#### Scenario: Revoked token rejected
- **WHEN** an Owner DELETEs a token's id and then a request uses the same token
- **THEN** the response is `401 Unauthorized` with `{ "error": "token revoked" }`

### Requirement: User & Org Management

The system SHALL expose CRUD for users (`GET/POST /api/v1/users`, `GET/PUT/DELETE /api/v1/users/:id`), organizations (`GET/POST /api/v1/orgs`, `GET/PUT/DELETE /api/v1/orgs/:id`), memberships (`GET/POST /api/v1/orgs/:id/members`, `DELETE /api/v1/orgs/:id/members/:user_id`), and teams (`GET/POST /api/v1/teams`, `GET/PUT/DELETE /api/v1/teams/:id`), backed by their respective repositories. User creation SHALL hash passwords with argon2id before storing. Every protected read/list SHALL filter rows to the caller's `org_id`; cross-org lookups by `:id` return `404 Not Found` rather than `403 Forbidden` to avoid existence enumeration.

#### Scenario: Password is hashed at rest
- **WHEN** an Owner creates a new user with password `"hunter2"`
- **THEN** the stored `password_hash` is an argon2 string starting with `$argon2id$` and is never equal to the plain password

#### Scenario: First user becomes Owner of default org
- **WHEN** the user table is empty and a user is created via `POST /api/v1/users`
- **THEN** the system also creates a default organization, inserts a role-free membership, resolves the `organization_bootstrap` role id from the IAM database, and creates that user's role binding in the same transaction; subsequent `POST /api/v1/users` calls require `org.members.manage` and do NOT auto-create orgs

#### Scenario: Cross-org user fetch returns 404
- **WHEN** a member of `orgA` requests `GET /api/v1/users/<id_from_orgB>`
- **THEN** the response is `404 Not Found` with `{ "error": "user not found" }`

#### Scenario: Team within org isolated
- **WHEN** a member of `orgA` lists `/api/v1/teams`
- **THEN** only teams whose `org_id = orgA` are returned, regardless of how many teams exist in other orgs

#### Scenario: Membership remove requires Owner role
- **WHEN** a caller without `org.members.manage` issues `DELETE /api/v1/orgs/:id/members/:user_id`
- **THEN** the response is `403 Forbidden` and the membership is unchanged

### Requirement: JWT secret bootstrap delegation

`IamService` SHALL delegate JWT signing secret loading to `SigningSecretRepository` at construction time (no longer reading `[auth].jwt_secret`). Active secrets are loaded into memory at startup; rotate API triggers reload.

#### Scenario: Construction loads active secrets from DB

- **WHEN** `IamService::new` is called with a `signing_secrets` repo
- **THEN** all `kind='jwt'` rows with `retired_at_micros IS NULL OR > now - 24h` are loaded into `active_secrets`
- **AND** primary secret is identified

#### Scenario: issue_token uses primary

- **WHEN** `IamService::issue_token(user, org)` is called
- **THEN** the resulting JWT is signed with the current primary secret

#### Scenario: verify_token tries all active secrets

- **WHEN** `IamService::verify_token(jwt)` is called and the JWT was signed with a not-yet-fully-retired secret
- **THEN** verification succeeds

### Requirement: Middleware accepts both JWT and `ms_*` tokens

The auth middleware SHALL dispatch `Authorization: Bearer <X>` based on prefix:
- `ms_` → `api-tokens` capability path (see `api-tokens` spec)
- Else → JWT path via `IamService::verify_token` (multi-secret)

Whitelisted paths (`/api/v1/auth/login`, `/api/v1/healthz`, `/metrics`, `/s/*`, `/api/v1/files/stream/*`) SHALL continue to bypass auth.

#### Scenario: JWT request still works

- **WHEN** a request carries `Authorization: Bearer eyJhbG...` (JWT format)
- **THEN** middleware uses `IamService::verify_token`, resolves capabilities, and injects `IamContext` on success

#### Scenario: API token request takes the api-tokens path

- **WHEN** a request carries `Authorization: Bearer ms_aB3kZ1xT9pQrU7nM_...`
- **THEN** middleware skips JWT decode and routes to `api_tokens.find_by_prefix` + argon2 verify
