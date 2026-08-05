## ADDED Requirements

### Requirement: System Management Audit Events

The audit system SHALL record successful and failed platform-administrator grants/revocations, `_sys` scope issuance, License upload/activation, Trace runtime enable/disable, sampling/threshold/capacity policy changes, and Trace debug-token issuance/use/revocation. System events SHALL be attributed to `_sys` and the initiating platform user. Audit payloads SHALL contain only action scope and a redacted change summary; they MUST NOT contain JWTs, debug tokens, License signed payloads/signatures, exporter credentials, or secret references.

#### Scenario: Trace policy update is audited
- **WHEN** a system-scoped administrator changes a sampling rule
- **THEN** an audit row records actor, policy version, affected rule identity, redacted before/after summary, result, and timestamp
- **AND** it does not store arbitrary matcher values that were rejected as sensitive/high-cardinality

#### Scenario: License upload excludes signed package
- **WHEN** an authorized License upload succeeds or fails verification
- **THEN** the audit event records version ID or verification outcome
- **AND** payload and signature bytes are absent

#### Scenario: Debug token use is audited without the token
- **WHEN** a scoped force-sampling token is issued, used, or revoked
- **THEN** each action creates an audit event containing token ID, scope, actor, and expiry
- **AND** no plaintext token is persisted

### Requirement: Platform Audit Query Isolation

System management audit events SHALL be queryable only through a system-scoped audit endpoint by callers with the appropriate platform audit permission. Ordinary organization audit queries SHALL NOT return `_sys` events.

#### Scenario: Tenant audit excludes platform operations
- **WHEN** an organization Owner queries the ordinary audit endpoint
- **THEN** no `_sys` License, platform-role, Trace-policy, or debug-token event is returned

#### Scenario: System auditor sees platform operations
- **WHEN** an authorized system-scoped caller queries the platform audit endpoint
- **THEN** matching `_sys` audit events are returned in timestamp order with redacted payloads

