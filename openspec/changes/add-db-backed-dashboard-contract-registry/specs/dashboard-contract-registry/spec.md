## ADDED Requirements

### Requirement: Immutable Published Dashboard Contracts

The system SHALL persist each reviewed Dashboard model schema, Dashboard authoring schema, and visualization manifest as an immutable published contract version containing a stable contract key, numeric version, kind, dialect, canonical document, canonical SHA-256 hash, status, and publication timestamp. Re-publishing an identical key/version/hash SHALL be idempotent, while publishing different content under an existing key/version SHALL fail without modifying the stored row.

#### Scenario: Built-in bundle is published at bootstrap
- **WHEN** a process starts against a database that does not contain the deployed Dashboard contracts
- **THEN** it validates and inserts the embedded model, authoring, and visualization documents as published immutable versions before serving contract-sensitive operations

#### Scenario: Existing version has different content
- **WHEN** bootstrap finds the same contract key and version with a different canonical hash
- **THEN** startup fails closed and does not overwrite the existing contract document

### Requirement: Atomic Dashboard Capability Binding

The system SHALL maintain one global active `dashboard.authoring.v1` binding that references exact published model, authoring, and visualization contract versions and hashes, a supported compiler version, an enabled flag, and a monotonically increasing revision. Changing or rolling back the binding SHALL validate the complete candidate bundle and update all references atomically.

#### Scenario: First bootstrap creates the default binding
- **WHEN** the three deployed contract versions are published and no Dashboard authoring binding exists
- **THEN** the system creates an enabled revision-one binding to the exact deployed versions and hashes

#### Scenario: Restart preserves a selected compatible binding
- **WHEN** a compatible active binding already exists and the process restarts
- **THEN** bootstrap publishes any missing built-in snapshots without replacing the existing binding

#### Scenario: Rollback activates an earlier published bundle
- **WHEN** a trusted internal caller activates a previously published compatible bundle
- **THEN** all three references change in one transaction and the binding revision increases exactly once

### Requirement: Fail-Closed Runtime Contract Resolution

The system SHALL resolve Dashboard contract-sensitive operations from the active PostgreSQL binding and compile/cache the bundle by binding revision and exact hashes. It SHALL reject missing, disabled, malformed, hash-mismatched, unsupported-dialect, cross-version-inconsistent, compiler-incompatible, or unavailable bundles without falling back to embedded contracts in production.

#### Scenario: Active bundle resolves successfully
- **WHEN** capability discovery, Dashboard preparation, or a native Dashboard write loads a valid active bundle
- **THEN** the operation uses the resolved authoring validator, model validator, and visualization manifest from that exact bundle

#### Scenario: Database registry is unavailable
- **WHEN** the active binding or referenced documents cannot be loaded from PostgreSQL
- **THEN** the contract-sensitive operation fails with an unavailable/internal error and does not validate, compile, persist, or execute a Dashboard using a stale or embedded fallback

#### Scenario: Binding revision changes
- **WHEN** the repository returns a different binding revision or any different referenced hash
- **THEN** the process replaces its compiled cache entry before serving the operation

### Requirement: Binary Compatibility Boundary

The runtime SHALL expose and execute only a database bundle whose authoring versions, Dashboard model version, visualization manifest version, compiler version, query kinds, and visualization capability structure are supported by the running binary. Database content SHALL NOT enable an implementation path absent from the compiler or renderer.

#### Scenario: Database declares an unsupported compiler version
- **WHEN** an active binding references a well-formed manifest whose compiler version is not supported by the running binary
- **THEN** runtime resolution fails closed and no unsupported capability is advertised to the model

#### Scenario: Capability discovery uses the active manifest
- **WHEN** the Agent calls `get_dashboard_capabilities`
- **THEN** the response is derived from the active resolved visualization manifest rather than a duplicate prompt or process-global catalog

### Requirement: Dashboard Draft Contract Pinning

Every prepared Dashboard draft SHALL persist the active binding revision plus authoring, model, and visualization contract hashes in addition to its existing versions, compiler version, and model hash. Preview, proposal, and execution SHALL compare those pins with the active bundle, and atomic consumption SHALL re-check the current database binding before inserting a Dashboard.

#### Scenario: Binding changes after preview
- **WHEN** a draft was reviewed under binding revision N and the active binding changes before proposal or execution
- **THEN** the operation returns `DRAFT_STALE`, creates no Dashboard, and requires the user to prepare and review a new draft

#### Scenario: Binding changes during execution
- **WHEN** the active binding changes after application validation but before the draft consumption transaction inserts the Dashboard
- **THEN** the transactional re-check rejects consumption and exactly zero Dashboards are created from that attempt

#### Scenario: Matching pinned draft is consumed once
- **WHEN** the draft pins match the active binding and all existing integrity, permission, expiry, and idempotency checks pass
- **THEN** the system atomically creates exactly one native Dashboard and marks the draft consumed

### Requirement: Contract Publication Authority

Dashboard contract publication and binding activation SHALL be available only to bootstrap and trusted internal administration seams. AI tool input, MCP tool input, chat prompts, tenant-scoped API requests, and Dashboard authoring specifications SHALL NOT be able to publish, replace, select, or disable contract versions.

#### Scenario: Model supplies contract selection fields
- **WHEN** a model attempts to include a contract hash, binding revision, contract key, or compiler override in a Dashboard authoring tool call
- **THEN** strict tool/authoring validation rejects the unknown fields and the server continues to derive the active bundle from PostgreSQL
