## ADDED Requirements

### Requirement: Immutable System Organization

The system SHALL always idempotently ensure exactly one system organization whose `name` and `slug` are both `_sys`. The organization SHALL be marked as system-owned. No user, platform administrator, API, background worker, Repository implementation, or application database role SHALL rename or delete it. Domain validation, Repository guards, and database triggers/constraints SHALL independently enforce this invariant.

#### Scenario: Fresh startup creates the organization
- **WHEN** the database has no organization with system identity
- **THEN** startup creates one organization with `name = "_sys"`, `slug = "_sys"`, and the system marker
- **AND** the operation is idempotent across concurrent/repeated startup

#### Scenario: Delete is rejected at the database boundary
- **WHEN** application code or direct SQL using the application database role attempts to delete `_sys`
- **THEN** the database rejects the operation
- **AND** `_sys` and its system-owned resources remain unchanged

#### Scenario: Runtime preparation failure degrades safely
- **WHEN** `_sys` cannot be prepared because the metadata store is temporarily unavailable
- **THEN** core data-plane roles may continue starting
- **AND** system telemetry and License management report degraded
- **AND** a structurally conflicting or tampered system organization fails startup

### Requirement: Immutable MoleSignal System Streams

For each enabled self-telemetry signal, the system SHALL create or reuse a typed stream named `_molesignal` in `_sys`. The stream name, organization ownership, system marker, and existence SHALL be immutable for every identity; deletion and rename SHALL always be rejected. Platform configuration MAY update retention and capacity-related non-identity properties.

#### Scenario: Typed streams cannot be deleted
- **WHEN** any caller attempts to delete `_sys/_molesignal` for logs, metrics, traces, or profiles
- **THEN** the request is rejected
- **AND** the exact typed stream remains queryable

#### Scenario: Retention remains configurable
- **WHEN** a platform administrator updates the Trace retention through the system telemetry API
- **THEN** only the retention policy changes
- **AND** stream name, type, organization, and system marker remain unchanged

### Requirement: Persistent Platform Administrators

The system SHALL persist platform-level administrator assignments independently of organization Membership and Role. The configured `root_email` SHALL receive the first assignment during bootstrap only. A platform administrator MAY grant or revoke other platform administrators, but the last remaining platform administrator SHALL NOT be revoked, deleted, or disabled in a way that leaves zero active administrators.

#### Scenario: Root bootstraps the first platform administrator
- **WHEN** no platform administrator exists and the configured root user is created or resolved
- **THEN** that user receives a persistent platform administrator assignment

#### Scenario: Last administrator is protected
- **WHEN** the only active platform administrator attempts to revoke their own assignment or is targeted for deletion
- **THEN** the operation returns a conflict/forbidden result
- **AND** the assignment and user remain active

### Requirement: System Scope Without Membership

`_sys` SHALL NOT accept ordinary Memberships and SHALL be hidden from ordinary organization lists, searches, and cross-organization queries. A platform administrator MAY select `_sys` through the existing organization-selection flow without Membership. The server SHALL issue a `system_scope` JWT with a maximum one-hour lifetime; ordinary users SHALL never receive `_sys` in their selectable organization list.

#### Scenario: Platform administrator selects the system organization
- **WHEN** a platform administrator selects `_sys` from an ordinary organization session
- **THEN** the server issues a short-lived token marked `system_scope`
- **AND** the token identifies `_sys` without creating a Membership row

#### Scenario: Ordinary user cannot discover the system organization
- **WHEN** a non-platform user lists, searches, fetches, or selects organizations
- **THEN** `_sys` is omitted or returned as `404 Not Found`

### Requirement: Fine-Grained Platform Permissions

The platform authorization model SHALL expose at least `SystemTelemetryRead`, `SystemTelemetryManage`, `LicenseRead`, `LicenseWrite`, `PlatformAdminManage`, and `TraceDebug`. A normal tenant-scoped token SHALL NOT exercise these permissions even when its user is a platform administrator; the caller MUST first obtain `system_scope`. `system_scope` SHALL NOT grant generic Organization or Stream mutation permissions.

#### Scenario: Tenant token cannot manage License
- **WHEN** a platform administrator calls a License management endpoint with a normal tenant-scoped JWT
- **THEN** the endpoint does not reveal the resource and returns `404 Not Found`

#### Scenario: System token can query telemetry but not mutate streams
- **WHEN** a platform administrator uses `system_scope` with `SystemTelemetryRead`
- **THEN** existing stream-list and Trace-query APIs can read `_sys/_molesignal`
- **AND** rename, delete, schema mutation, and public ingest operations remain forbidden

### Requirement: Platform API Namespace

All platform-level management endpoints SHALL be located under `/api/v1/system/*`, including License, platform-administrator, telemetry-policy, and Trace-debug resources. No compatibility alias SHALL expose these operations in ordinary organization namespaces during the development-stage migration.

#### Scenario: Legacy License route is absent
- **WHEN** any caller requests the former `/api/v1/license` route
- **THEN** the response is `404 Not Found`
- **AND** only an authorized `system_scope` caller can use `/api/v1/system/license`

