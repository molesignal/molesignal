## ADDED Requirements

### Requirement: Password Login & JWT Issuance

`POST /api/v1/auth/login` SHALL accept `{ "email", "password" }`, look up the user via `UserRepository::get_by_email`, verify the password against the stored argon2 hash, and return `{ "token": "<JWT>" }` on success.

#### Scenario: Successful login
- **WHEN** the email matches and the password verifies
- **THEN** the response is `200 OK` with a JWT signed by the server's HS256 secret containing claims `{ sub: user_id, org_id, role, exp: now + auth.token_ttl_secs }`

#### Scenario: Bad credentials
- **WHEN** the email is not found OR the password does not verify
- **THEN** the response is `401 Unauthorized` with body `{ "error": "invalid credentials" }` regardless of which condition failed (no user-enumeration)

#### Scenario: Disabled user
- **WHEN** the user exists but `disabled = true`
- **THEN** the response is `403 Forbidden` with body `{ "error": "user disabled" }`

### Requirement: JWT Auth Middleware

Every request to `/api/v1/*` except `/api/v1/auth/login` and `/api/v1/healthz` SHALL pass through a middleware that validates the `Authorization: Bearer <token>` header and injects `AuthContext { user_id, org_id, role }` into the request extensions; missing/invalid tokens yield `401 Unauthorized`.

#### Scenario: Expired token
- **WHEN** a request carries a JWT whose `exp` is in the past
- **THEN** the response is `401 Unauthorized` with `{ "error": "token expired" }`

#### Scenario: Health endpoint stays public
- **WHEN** an unauthenticated request hits `/api/v1/healthz`
- **THEN** the response is `200 OK`

### Requirement: Role-Based Authorization

Every protected handler SHALL declare the required `Permission` and the middleware/extractor SHALL call `Role::allows(perm)` on the caller's role, rejecting with `403 Forbidden` when it returns false.

#### Scenario: Viewer cannot write
- **WHEN** a Viewer-role caller posts to `/api/v1/alerts/rules`
- **THEN** the response is `403 Forbidden`

### Requirement: User & Org Management

The system SHALL expose CRUD for users (`/api/v1/users`), organizations (`/api/v1/orgs`), memberships (`/api/v1/orgs/:id/members`), and teams (`/api/v1/teams`), backed by their respective repositories; user creation hashes passwords with argon2 before storing.

#### Scenario: Password is hashed at rest
- **WHEN** an Owner creates a new user with password `"hunter2"`
- **THEN** the stored `password_hash` is an argon2 string starting with `$argon2id$` and is never equal to the plain password

#### Scenario: First user becomes Owner of default org
- **WHEN** the user table is empty and a user is created via `POST /api/v1/users`
- **THEN** the system also creates a default organization and inserts a `Membership { role: Owner }` for that user
