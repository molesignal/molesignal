## ADDED Requirements

### Requirement: JWT secret bootstrap delegation

`IdentityService` SHALL delegate JWT signing secret loading to `SigningSecretRepository` at construction time (no longer reading `[auth].jwt_secret`). Active secrets are loaded into memory at startup; rotate API triggers reload.

#### Scenario: Construction loads active secrets from DB

- **WHEN** `IdentityService::new` is called with a `signing_secrets` repo
- **THEN** all `kind='jwt'` rows with `retired_at_micros IS NULL OR > now - 24h` are loaded into `active_secrets`
- **AND** primary secret is identified

#### Scenario: issue_token uses primary

- **WHEN** `IdentityService::issue_token(user, org, role)` is called
- **THEN** the resulting JWT is signed with the current primary secret

#### Scenario: verify_token tries all active secrets

- **WHEN** `IdentityService::verify_token(jwt)` is called and the JWT was signed with a not-yet-fully-retired secret
- **THEN** verification succeeds

### Requirement: Middleware accepts both JWT and `ms_*` tokens

The auth middleware SHALL dispatch `Authorization: Bearer <X>` based on prefix:
- `ms_` → `api-tokens` capability path (see `api-tokens` spec)
- Else → JWT path via `IdentityService::verify_token` (multi-secret)

Whitelisted paths (`/api/v1/auth/login`, `/api/v1/healthz`, `/metrics`, `/s/*`, `/api/v1/files/stream/*`) SHALL continue to bypass auth.

#### Scenario: JWT request still works

- **WHEN** a request carries `Authorization: Bearer eyJhbG...` (JWT format)
- **THEN** middleware uses `IdentityService::verify_token` and injects `AuthContext` on success

#### Scenario: API token request takes the api-tokens path

- **WHEN** a request carries `Authorization: Bearer ms_aB3kZ1xT9pQrU7nM_...`
- **THEN** middleware skips JWT decode and routes to `api_tokens.find_by_prefix` + argon2 verify

## REMOVED Requirements

### Requirement: `[auth].jwt_secret` configuration field
**Reason**: JWT secrets are now DB-persisted with auto-bootstrap; static config field invites footguns ("why isn't my override taking effect?"). Override path moves to `MS_AUTH_JWT_SECRET_OVERRIDE` env var with explicit upsert-as-primary semantics.

**Migration**: Operators who previously set `[auth] jwt_secret = "..."` in `conf/config.toml` SHALL either:
1. Delete the line (auto-generation takes over; new secret is generated on first start and persisted)
2. Move the value to `MS_AUTH_JWT_SECRET_OVERRIDE` env var if deterministic / shared-across-deployment behavior is required

The OSS `conf/config.toml` template and `crates/config/src/settings.rs::AuthSettings` SHALL no longer contain a `jwt_secret` field.
