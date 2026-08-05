## ADDED Requirements

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
