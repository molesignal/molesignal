# Signing Secrets Capability

## Purpose

JWT 签名密钥的 DB 持久化、首次启动自举、并发安全、多密钥校验窗口与轮换 HTTP 端点。消除静态配置型 `jwt_secret` 字段带来的部署陷阱，并保证多副本下密钥的一致性。

## Requirements

### Requirement: First-run JWT Signing Secret Bootstrap

The system SHALL on every process start ensure exactly one `signing_secrets` row exists with `kind = 'jwt'` and `is_primary = TRUE`. Resolution order:
1. If `MS_AUTH_JWT_SECRET_OVERRIDE` env is set, write/upsert it as primary (CI / deterministic deploy path).
2. Else if a primary row already exists, use it.
3. Else generate a 32-byte CSPRNG (`rand::OsRng`) value, INSERT as primary, emit `tracing::info!("first-run: generated new JWT signing secret, kid={}", id)`.

#### Scenario: Empty DB on first start

- **WHEN** the process starts against a fresh database with no signing_secrets rows
- **THEN** exactly one row with `kind='jwt'`, `is_primary=TRUE`, `secret` = 32 random bytes is inserted
- **AND** the info log line appears
- **AND** subsequent process starts against the same DB reuse this row (no new row inserted)

#### Scenario: Override via env var

- **WHEN** `MS_AUTH_JWT_SECRET_OVERRIDE=<base64-32B>` is set
- **THEN** the configured value SHALL be written as primary even if a different primary already exists; the previously-primary row SHALL be retired (see rotate)

### Requirement: Concurrent bootstrap safety

When two processes start simultaneously against the same fresh DB, only one row SHALL end up as primary. The losing process SHALL detect the unique-constraint violation and re-read the winner's row.

#### Scenario: Two ingesters start at exactly the same time

- **WHEN** ingester A and ingester B both reach bootstrap_or_load step with no existing primary
- **AND** both generate their own 32B secret and attempt INSERT
- **THEN** the second INSERT fails with unique-violation on `kind WHERE is_primary` partial index
- **AND** the loser re-reads the primary row, discards its locally-generated secret, and uses the winner's

### Requirement: Multi-secret verify during rotation window

The system SHALL hold all `signing_secrets` rows where `kind='jwt'` AND `retired_at_micros IS NULL OR retired_at_micros > now - 24h` in memory as the active secret set. `IamService::verify_token` SHALL attempt each active secret in order (primary first); the first successful decode wins.

#### Scenario: Token issued before rotate still verifies for 24h

- **GIVEN** primary secret S1 issued JWT T1 at t0
- **WHEN** rotate happens at t0 + 1h (S1 retired, S2 becomes primary)
- **THEN** T1 SHALL still verify successfully (S1 in active set since retired < 24h ago)
- **AND** new tokens are signed with S2

#### Scenario: Token issued > 24h before rotate fails

- **GIVEN** S1 retired at t0 + 25h
- **WHEN** verify_token(T1 signed by S1) at t0 + 26h
- **THEN** the call returns Unauthorized (S1 no longer in active set)

### Requirement: Rotate HTTP endpoint

The system SHALL expose `POST /api/v1/auth/jwt/rotate` (Owner-only). It SHALL:
1. Retire current primary (set `is_primary=FALSE`, `retired_at_micros=now`).
2. INSERT new primary with random 32B.
3. Trigger in-process active-secrets reload across all running instances (cluster broadcast via `cluster_nodes` repo OR best-effort: rely on per-instance 60s reload).
4. Write audit_events row.

#### Scenario: Owner rotates JWT secret

- **WHEN** an Owner POSTs `/api/v1/auth/jwt/rotate`
- **THEN** response is `200 { new_kid: "<ksuid>", retired_kid: "<old-ksuid>" }`
- **AND** new JWT issuance immediately uses new secret
- **AND** an audit_events row is written with `action="jwt.rotate"`

#### Scenario: Non-Owner rotate denied

- **WHEN** a user with role `Admin` (not Owner) POSTs `/api/v1/auth/jwt/rotate`
- **THEN** response is `403 Forbidden`

### Requirement: List endpoint excludes secret material

The system SHALL expose `GET /api/v1/auth/jwt/secrets` (Owner-only). Response includes `[{ id, created_at_micros, retired_at_micros, is_primary }]` — **the `secret` bytes column MUST NOT appear in any response or log**.

#### Scenario: list returns metadata only

- **WHEN** an Owner GETs `/api/v1/auth/jwt/secrets`
- **THEN** every item has fields `{id, created_at_micros, retired_at_micros, is_primary}` and no `secret` field
