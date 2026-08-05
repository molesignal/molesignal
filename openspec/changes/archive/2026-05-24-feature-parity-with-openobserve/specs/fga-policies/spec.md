## ADDED Requirements

### Requirement: Resource-level policy storage

The system SHALL expose `/api/v1/fga/policies` to CRUD policies of the form `{ subject: user|team|role, subject_id, action, resource_kind, resource_id?, effect: allow|deny }`. Policies are org-scoped. Operations require `license.has_feature("fga")`.

#### Scenario: Allow policy created

- **WHEN** an Admin POSTs `{ "subject": "user", "subject_id": "u1", "action": "read", "resource_kind": "stream", "resource_id": "app_logs", "effect": "allow" }`
- **THEN** user `u1` can subsequently `GET /api/v1/streams/app_logs` and the audit log records the policy that matched

### Requirement: Decision evaluation order

For any access decision, the system SHALL evaluate (1) explicit DENY policies first (any match → deny), (2) explicit ALLOW policies (any match → allow), (3) fallback to the role-based default from `identity` capability. Wildcards `*` in `resource_id` SHALL match any id of the same kind.

#### Scenario: DENY beats ALLOW

- **WHEN** user `u1` has both `allow read on streams/*` and `deny read on streams/secrets`
- **THEN** access to `streams/secrets` is denied; access to other streams is allowed

### Requirement: Decision cache

Policy evaluation SHALL use an in-process cache keyed by `(org_id, subject_id, action, resource_kind, resource_id)` with TTL 60s. Policy CRUD SHALL invalidate the cache for the affected org.

#### Scenario: Cache invalidated on update

- **WHEN** a policy is added for org A
- **THEN** the next decision request for org A reflects the new policy within 1 second
