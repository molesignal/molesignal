## ADDED Requirements

### Requirement: Platform Administrator Persistence

The identity domain SHALL persist platform-administrator assignments independently from organization Membership and Role. During first bootstrap, the configured root user SHALL receive the initial assignment when no platform administrator exists. The system SHALL prevent revoking, disabling, or deleting the last active platform administrator. Grant and revoke operations SHALL require `PlatformAdminManage` in `system_scope`.

#### Scenario: Root user receives initial platform role
- **WHEN** the root user is resolved and no platform administrator assignment exists
- **THEN** the system creates one persistent platform-administrator assignment for that user
- **AND** subsequent authorization uses the assignment rather than matching the user's email

#### Scenario: Multiple platform administrators are supported
- **WHEN** an authorized system-scoped administrator grants the platform role to another active user
- **THEN** both users can independently obtain system scope
- **AND** no organization Membership is created as a side effect

#### Scenario: Last platform administrator cannot be removed
- **WHEN** an operation would leave zero active platform administrators
- **THEN** the operation is rejected
- **AND** the protected user and assignment remain usable

### Requirement: System-Scope Auth Context and JWT

JWT claims and `AuthContext` SHALL distinguish ordinary organization scope from `system_scope`. A system-scoped token SHALL identify `_sys`, issuer user, issued time, and expiry no more than one hour after issue, but SHALL NOT contain a role or permission grants. An active platform-administrator assignment SHALL make the user eligible for `_sys`; the concrete platform role SHALL be selected by the database purpose mapping and loaded from `_sys` `iam_roles`, with effective permissions loaded from `iam_role_permissions`. Organization Roles and API tokens SHALL NOT implicitly grant system scope. Platform endpoints SHALL return `404 Not Found` when called without valid system scope.

#### Scenario: System token injects platform context
- **WHEN** a platform administrator presents a valid system-scoped JWT
- **THEN** authentication resolves the database platform role and injects an AuthContext marked as system scope with its current platform permissions
- **AND** ordinary organization Role evaluation is not used to authorize platform operations

#### Scenario: Root receives the database-seeded system role without tenant escalation
- **WHEN** the configured bootstrap root user selects `_sys`
- **THEN** the selection response and capability snapshot contain the role selected for the `platform_administrator` purpose
- **AND** the default database seed names that role `Owner`
- **AND** the system-scoped JWT contains no role or permission claim
- **AND** ordinary stream mutation, organization administration, API-token creation, and public ingest remain unavailable
- **AND** platform operations continue to require their fine-grained platform permissions

#### Scenario: API token cannot become system scoped
- **WHEN** an `ms_*` API token is used against `/api/v1/system/license`
- **THEN** the request returns `404 Not Found`
- **AND** the token's organization Role is not considered a platform permission

#### Scenario: Expired system token is rejected
- **WHEN** a system-scoped JWT is older than its maximum one-hour lifetime
- **THEN** authentication rejects it and requires the user to switch into `_sys` again

### Requirement: System Organization Discovery and Selection

Ordinary organization list/search/get APIs SHALL hide `_sys` from non-platform users. For a platform administrator, the existing organization-list response SHALL include `_sys` marked as a system scope option. Selecting it SHALL issue a system-scoped JWT without creating or requiring Membership. `_sys` SHALL NOT be selectable as an ordinary organization or used as a target for membership/team APIs.

#### Scenario: Platform administrator sees a switch target
- **WHEN** a platform administrator lists selectable organizations with a tenant-scoped session
- **THEN** `_sys` appears once with an explicit system marker
- **AND** selecting it yields a system-scoped token

#### Scenario: Ordinary user cannot enumerate `_sys`
- **WHEN** a non-platform user lists organizations or requests `_sys` by ID or slug
- **THEN** `_sys` is omitted or returned as `404 Not Found`

#### Scenario: Membership creation is forbidden
- **WHEN** any caller attempts to add a user or team membership to `_sys`
- **THEN** the operation is rejected at both domain and Repository boundaries

### Requirement: Platform Administrator Management API

The system SHALL expose list, grant, and revoke operations under `/api/v1/system/platform-admins`. All mutations SHALL require `PlatformAdminManage`, enforce the last-administrator invariant transactionally, and return no authentication secret.

#### Scenario: Authorized grant succeeds
- **WHEN** a system-scoped administrator grants the platform role to an active user
- **THEN** the API returns the assignment metadata
- **AND** the action is audited

#### Scenario: Tenant-scoped grant is hidden
- **WHEN** the same user calls the endpoint with a tenant-scoped token
- **THEN** the endpoint returns `404 Not Found`
