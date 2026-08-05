## MODIFIED Requirements

### Requirement: Capability-Based IAM Enforcement

Every protected handler SHALL declare a canonical permission key, directly or through a typed mapping. After credential verification, IAM middleware SHALL resolve the authenticated principal's current versioned capabilities for the signed organization context and inject them into `IamContext`; handlers SHALL check that resolved set and reject with `403 Forbidden` when the permission is absent. JWTs SHALL NOT carry a role. Memberships SHALL NOT contain a role. Organization role assignment SHALL come from `iam_role_bindings`; `_sys` role assignment SHALL come from the persisted platform-administrator assignment plus its database purpose mapping. In both scopes, role names, display roles, and permissions SHALL be read from `iam_roles` and `iam_role_permissions`; no fixed application role enum, role-string conversion, or hard-coded system display role is permitted.

#### Scenario: Viewer cannot write
- **WHEN** a Viewer-role caller without `alerts.manage` posts to `/api/v1/alerts/rules`
- **THEN** the response is `403 Forbidden`

#### Scenario: Custom role grants access
- **WHEN** a user has an active custom-role binding that grants the handler's permission
- **THEN** the handler authorizes the request even when the token's display role is Viewer

#### Scenario: Removed binding takes effect immediately
- **WHEN** an administrator removes the binding that granted a permission
- **THEN** the user's next protected request is denied using the incremented policy version

## REMOVED Requirements

### Requirement: IAM Policy Storage Foundation

**Reason**: The low-level action/resource policy row model is replaced by the database-backed IAM permission catalog, role bindings, relationships, and explicit cross-organization grants.

**Migration**: Development databases run the unified IAM migration, which drops `rbac_policies` and uses role-free memberships plus IAM role/binding/grant APIs. No compatibility backfill from a membership role column is retained.

### Requirement: PolicyEvaluator trait

**Reason**: The old evaluator accepted unregistered strings and was unused by most handlers.

**Migration**: Callers use `IamAccessService` and typed permission checks; resource-aware clients use `/api/v1/iam/evaluate-batch`.
