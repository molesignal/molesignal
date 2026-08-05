## ADDED Requirements

### Requirement: RBAC Policy Storage Foundation

The system SHALL persist OSS-level RBAC policies in a `rbac_policies` table `{ id, org_id, subject_kind: user|team|role, subject_id, action, resource_kind, resource_id?, effect: allow|deny, created_by, created_at }`. The schema is the subset of `fga-policies` (enterprise capability) such that an enterprise upgrade adds columns without altering existing ones.

#### Scenario: OSS policy persisted

- **WHEN** an Admin POSTs `/api/v1/auth/policies { subject_kind: "role", subject_id: "viewer", action: "read", resource_kind: "stream" }`
- **THEN** the row persists in `rbac_policies` and subsequent access decisions for viewers reading streams check this row

### Requirement: PolicyEvaluator trait

The system SHALL define a `PolicyEvaluator` trait with method `decide(org_id, subject_id, action, resource_kind, resource_id) -> Decision::{Allow, Deny}`. The OSS impl `BasicPolicyEvaluator` SHALL evaluate `rbac_policies` rows and fall back to `Role::allows`. Enterprise builds SHALL replace this with the `fga-policies` engine via cfg gate.

#### Scenario: OSS decides via basic evaluator

- **WHEN** the binary is OSS and a request reaches the permission middleware
- **THEN** `BasicPolicyEvaluator` is used; no FGA-specific features (attribute conditions, relationship traversal) are available

#### Scenario: Decision logged to audit

- **WHEN** any `decide` call returns `Deny`
- **THEN** an `audit_events` row is written with `{ kind: "policy_denied", subject_id, action, resource_kind, resource_id }`
