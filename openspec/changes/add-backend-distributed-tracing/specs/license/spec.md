## MODIFIED Requirements

### Requirement: License File Parsing

On startup, the system SHALL ensure `_sys` and load the single active License version persisted for that system organization. The stored record SHALL contain an immutable original signed License package. The server SHALL re-verify its Ed25519 signature and validity before constructing the active `LicenseGate`.

When no persisted version exists, an explicitly configured file/environment License MAY be used for first bootstrap and persisted after successful verification. A disaster fallback to file/environment SHALL occur only when explicitly enabled. Missing License SHALL run in Community mode. A persisted or fallback License that is malformed, damaged, or fails signature verification SHALL NOT be activated; the server SHALL start in Community mode and emit a high-priority alert rather than stop the core service.

#### Scenario: Valid persisted License is activated
- **WHEN** `_sys` has an active License version whose signature verifies
- **THEN** `main()` starts with a LicenseGate built from that version
- **AND** an authorized `GET /api/v1/system/license` returns its verified snapshot

#### Scenario: Tampered persisted License degrades safely
- **WHEN** the active stored payload is modified but its signature is unchanged
- **THEN** the License is not activated
- **AND** the server starts in Community mode
- **AND** detailed system health and alerting report the verification failure

#### Scenario: Missing License falls back to Community
- **WHEN** no persisted License exists and no valid bootstrap source is configured
- **THEN** the server starts in Community mode
- **AND** `LicenseGate::has_feature(_)` returns false for every licensed feature

#### Scenario: Explicit bootstrap succeeds once
- **WHEN** no persisted version exists and a valid bootstrap License source is configured
- **THEN** the server verifies and stores it as an immutable `_sys` version
- **AND** subsequent restarts load the persisted active version

## ADDED Requirements

### Requirement: System License Version Management

The system SHALL maintain one instance-wide License under `_sys` using immutable version records and one transactional active-version pointer. An authorized platform administrator MAY upload a new signed version, list version metadata, inspect the active snapshot, and activate a still-valid historical version. Version payloads SHALL NOT be edited or deleted. Activation SHALL re-verify the signature and validity, update the pointer transactionally, and hot-replace the process `LicenseHolder`.

#### Scenario: New version is uploaded and activated
- **WHEN** a caller with `LicenseWrite` uploads a valid signed License package
- **THEN** an immutable version record is created under `_sys`
- **AND** activation atomically updates the active pointer and runtime LicenseGate

#### Scenario: Historical version can be reactivated
- **WHEN** an authorized platform administrator selects a historical version that still verifies and is not expired
- **THEN** it becomes the active instance License
- **AND** the prior active version remains in immutable history

#### Scenario: Version deletion is rejected
- **WHEN** any identity or application database role attempts to delete or edit a License version
- **THEN** the operation is rejected
- **AND** the version history remains intact

### Requirement: License APIs Are System-Only

All License read and write APIs SHALL reside under `/api/v1/system/license*` and SHALL require valid `system_scope` plus `LicenseRead` or `LicenseWrite`. Ordinary authenticated users SHALL see no License edition, features, owner, limits, status, signature data, or endpoint existence.

#### Scenario: Platform administrator reads License
- **WHEN** a system-scoped caller with `LicenseRead` requests `/api/v1/system/license`
- **THEN** the response contains the active License snapshot and permitted version metadata
- **AND** it never returns private key material or unredacted secret references

#### Scenario: Ordinary authenticated user sees nothing
- **WHEN** a tenant-scoped user requests any License endpoint
- **THEN** the response is `404 Not Found`
- **AND** no License metadata is returned

## REMOVED Requirements

### Requirement: License Read Endpoint

**Reason**: Instance License information is a platform-level system resource. Exposing edition, features, owner, limits, and expiry to every authenticated tenant conflicts with the `_sys` security boundary.

**Migration**: Remove `/api/v1/license`. Platform administrators SHALL switch to `_sys` and use `/api/v1/system/license`; non-platform clients receive no replacement endpoint.

