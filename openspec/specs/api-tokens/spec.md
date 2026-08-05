# API Tokens Capability

## Purpose

提供三类长期 API 凭据：一次性回显的个人 `ms_` token、可重复安全回显的默认接入
`ms_` token，以及应用绑定的公开 RUM `msrum_` token。所有 token 均按全局唯一前缀
O(1) 查询，执行类型、密钥、撤销、过期、组织状态和 IAM role 校验；完整明文永不出现在
列表接口、日志或审计 payload 中。

## Requirements

### Requirement: Personal token issuance

The system SHALL expose `POST /api/v1/auth/tokens` accepting
`{ name, role_id?, expires_in_days? }`. It returns the plaintext
`ms_<16-prefix>_<32-secret>` only in that response, stores an Argon2id hash, and rejects a role
whose permissions exceed the caller. The built-in `rum_client` role cannot be selected here.

#### Scenario: Personal token is shown once

- **WHEN** a caller with `api_tokens.manage` creates a personal token
- **THEN** the response contains `{ id, prefix, token, role_id, role_key, token_kind: "personal" }`
- **AND** the response carries `Cache-Control: private, no-store` and `Pragma: no-cache`
- **AND** subsequent list responses contain neither plaintext nor `secret_hash`

#### Scenario: RUM role cannot be issued as a generic token

- **WHEN** the selected role has key `rum_client`
- **THEN** the request is rejected and directs the caller to `GET /api/v1/auth/tokens/rum`

### Requirement: Managed default ingestion token

The system SHALL expose `GET /api/v1/auth/tokens/default` to callers with
`api_tokens.manage`. It creates or re-displays one active token per organization and user with
token kind `default_ingestion` and the built-in write-only `ingest` role. The plaintext is encrypted
with the configured cipher root key; its secret is additionally stored as an Argon2id hash for
authentication.

#### Scenario: Datasource page can safely re-display the token

- **WHEN** an authorized user opens a non-RUM datasource guide more than once
- **THEN** each request returns the same active `ms_` credential
- **AND** every plaintext response is marked `private, no-store`
- **AND** the token grants `streams.write` but no read, query, configuration, or IAM permission

#### Scenario: Concurrent first reads converge

- **WHEN** two requests first load the same user's default token concurrently
- **THEN** an advisory transaction lock ensures they resolve to one active managed token

### Requirement: Application-bound RUM client token

The system SHALL expose
`GET /api/v1/auth/tokens/rum?application_id=<id>` to callers with `api_tokens.manage`.
It creates or re-displays one active `msrum_<16-prefix>_<32-secret>` credential per organization and
application. The token uses the built-in `rum_client` role, grants only `rum.write`, stores encrypted
plaintext for authorized re-display, and stores a SHA-256 verifier over the high-entropy 32-character
secret for low-cost public-ingest authentication.

#### Scenario: RUM token is least privilege and non-cacheable

- **WHEN** an authorized user requests the token for `application_id=checkout-mobile`
- **THEN** the response has `token_kind: "rum_client"`, the same `application_id`, and a full
  `msrum_` credential
- **AND** the response carries `Cache-Control: private, no-store` and `Pragma: no-cache`
- **AND** the token has `rum.write` but not `streams.write`, read, query, configuration, or IAM access

#### Scenario: RUM token cannot switch applications

- **WHEN** that credential sends a RUM event or replay segment naming another application
- **THEN** ingestion returns `403 Forbidden`
- **AND** an omitted application is filled from the credential binding

#### Scenario: Issuer suspension does not disable the application

- **WHEN** the user who originally requested an application token is later disabled
- **THEN** the application credential remains valid
- **AND** token revocation or organization disablement still takes effect

### Requirement: Token listing and revocation

The system SHALL expose `GET /api/v1/auth/tokens` for organization-scoped metadata and
`DELETE /api/v1/auth/tokens/{id}` for revocation. List items include token kind and optional
application binding but omit plaintext, sealed plaintext, nonce, and verifier. Revocation invalidates
the prefix cache immediately.

#### Scenario: List distinguishes credential types

- **WHEN** a caller lists organization tokens
- **THEN** each item includes `token_kind` in
  `personal | default_ingestion | rum_client`
- **AND** only `rum_client` items carry `application_id`
- **AND** revoked items remain visible with `revoked: true`

### Requirement: Bearer authentication

The auth middleware SHALL distinguish `ms_` and `msrum_` prefixes, reject a prefix/type mismatch,
verify the token-kind-specific secret hash, enforce revoke/expiry and current organization state,
resolve permissions from the stored role, and update `last_used_at_micros` at most once per five
minutes per prefix. Personal/default tokens additionally require the owning user to remain active.

#### Scenario: Wrong marker cannot bypass token kind

- **WHEN** a `rum_client` row is presented with an `ms_` marker, or another token kind is presented
  with `msrum_`
- **THEN** authentication returns `401 Unauthorized`

#### Scenario: Revocation invalidates a cached token

- **WHEN** an authorized caller revokes an active token
- **THEN** its in-process prefix cache is invalidated before the revoke request completes
- **AND** the next authenticated request is rejected
