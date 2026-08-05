## ADDED Requirements

### Requirement: Database-Backed IAM Permission Catalog

The system SHALL load one canonical database catalog of `resource.action` permission keys shared by backend validation and frontend route metadata. Each permission SHALL declare its scope, domain, translation keys, built-in role assignments, and optional product feature. Unknown keys and organization attempts to grant `sys.*` keys MUST be rejected. No runtime file registry is permitted.

#### Scenario: Unknown permission rejected
- **WHEN** an administrator creates or updates a role with `permissions: ["streams.reed"]`
- **THEN** the API returns `400 Bad Request`
- **AND** no role permission or policy version is changed

#### Scenario: Organization role cannot grant platform permission
- **WHEN** an organization owner assigns `sys.licenses.manage` to an organization role
- **THEN** the API returns `400 Bad Request`
- **AND** the platform capability remains unavailable

### Requirement: Versioned Capability Snapshot

`GET /api/v1/iam/capabilities` SHALL return the authenticated principal's capabilities for the signed active organization as `{ organization_id, scope, permissions, features, version }`. Permissions SHALL be sorted, deduplicated, and computed from current server-side bindings rather than trusted from browser state or the display role in a token.

#### Scenario: Viewer receives read capabilities
- **WHEN** a Viewer member requests `/api/v1/iam/capabilities`
- **THEN** the response includes the catalog permissions assigned to the built-in Viewer role
- **AND** excludes write, role-management, and all `sys.*` permissions

#### Scenario: Revoked permission disappears on next request
- **WHEN** an administrator removes a role binding and the mutation succeeds
- **THEN** the organization policy version increases
- **AND** the principal's next capability request omits the removed permissions without waiting for a TTL

### Requirement: Multi-Role Principal Bindings

The system SHALL support multiple active role bindings per user, team, group, service account, or organization principal. A binding SHALL be scoped to one organization and MAY constrain resource type/id, bounded conditions, start time, and expiry. Expired or not-yet-active bindings SHALL not contribute permissions.

#### Scenario: User inherits permissions from two roles
- **WHEN** a user has active `data_analyst` and `pipeline_operator` bindings in the same organization
- **THEN** the capability snapshot is the union of both roles' allowed permissions

#### Scenario: Team binding applies to members
- **WHEN** team `sre` is bound to a role granting `alerts.manage`
- **AND** user `u1` is an active member of team `sre`
- **THEN** `u1` is allowed to manage alerts in the binding scope

### Requirement: Resource Relationship IAM

The system SHALL support direct `owner`, `editor`, `operator`, `viewer`, and `member` relationships between a resource and a principal. Relationship-to-permission mappings SHALL be catalog-controlled and evaluation SHALL be bounded to a direct relation plus at most one declared container relation.

#### Scenario: Dashboard viewer relation grants read only
- **WHEN** user `u1` has relation `viewer` on dashboard `d1`
- **THEN** evaluation of `dashboards.read` for `d1` is allowed
- **AND** evaluation of `dashboards.edit` for `d1` is denied unless another binding grants it

### Requirement: Explicit Cross-Organization Grants

Cross-organization access SHALL require an active explicit grant containing source organization, target organization, grantee, bounded resource selector, permissions, validity window, status, approver, and creator. Grants SHALL be non-transitive and MUST NOT permit permissions the creator lacks on the shared resource.

#### Scenario: Active grant permits shared dashboard read
- **WHEN** Org A grants Org B team `sre` `dashboards.read` for dashboard `d1` until a future expiry
- **AND** the grant is accepted and active
- **THEN** an Org B member of team `sre` can read `d1`
- **AND** receives no navigation or access to unrelated Org A resources

#### Scenario: Re-sharing is denied
- **WHEN** Org B only possesses Org A dashboard `d1` through a cross-organization grant
- **THEN** Org B cannot create a grant that shares `d1` to Org C

### Requirement: Deterministic IAM Decisions

The IAM access service SHALL apply platform boundary, tenant isolation, hard system deny, active cross-organization grant, role permissions, resource relationships, bounded conditions, and default deny in that order. Decisions SHALL include `allowed`, `reason`, matched binding/grant identifiers, and `policy_version`.

#### Scenario: Default deny
- **WHEN** no active binding, relationship, platform assignment, or cross-organization grant matches a request
- **THEN** the decision is denied with reason `default_deny`

#### Scenario: Hard deny cannot be overridden
- **WHEN** an organization binding contains a permission that is forbidden in the current system scope
- **THEN** the decision is denied with reason `scope_boundary`

### Requirement: Batch IAM Evaluation

`POST /api/v1/iam/evaluate-batch` SHALL accept at most 100 permission/resource requests for the authenticated principal and signed organization context and return one ordered decision per input. The endpoint MUST reject attempts to evaluate another arbitrary principal.

#### Scenario: Page evaluates multiple controls
- **WHEN** the browser submits read, edit, and delete decisions for one dashboard
- **THEN** the response preserves input order and returns all three decisions in one request

### Requirement: IAM Audit And Version Invalidation

Every role, binding, relationship, or cross-organization grant mutation SHALL increment the affected policy version atomically and record an audit event identifying the actor, affected organization, target object, and changed permission keys.

#### Scenario: Role update invalidates snapshots
- **WHEN** an administrator removes `pipelines.run` from a custom role
- **THEN** the role update, version increment, and audit event commit together
- **AND** cached snapshots under the prior version are no longer used

### Requirement: Immutable System Scope

The `_sys` organization SHALL remain immutable. A persisted IAM
platform-administrator assignment SHALL make a principal eligible for `_sys`,
while `iam_builtin_role_purposes` SHALL select its materialized platform role.
The role's display name SHALL be read from `iam_roles`, and its capabilities
SHALL be read from `iam_role_permissions` and validated against platform-scoped
catalog entries. The bootstrap seed SHALL map platform administrators to the
database-defined Owner role; ordinary organization role bindings SHALL never
add system capabilities.

#### Scenario: Root system snapshot
- **WHEN** the bootstrap root switches to `_sys`
- **THEN** the snapshot scope is `system`
- **AND** it includes the registered platform-administrator permissions
- **AND** the display role is the name of the database role selected for the platform-administrator purpose

#### Scenario: System role changes are database-driven
- **WHEN** the selected `_sys` role is renamed or one of its role-permission rows is removed and the policy version advances
- **THEN** the next capability snapshot returns the new role name and no longer contains the removed permission
- **AND** no application role enum or hard-coded Owner fallback is involved
