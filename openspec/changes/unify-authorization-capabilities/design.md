## Context

IAM enforcement is currently split across independent mechanisms:

1. JWT/API-token handling historically carried a fixed display `Role`.
2. A low-level policy table exists but almost no handler uses it.
3. The web shell separately filters routes by hard-coded role/scope arrays.

The product is in development, so this change uses a clean break instead of preserving the old policy API, storage, or terminology.

The current URL model uses a signed organization-scoped token rather than `/orgs/:orgId/*` routes. Rewriting every product URL and API is valuable, but it is deliberately separated from this change so the security and visibility source of truth can ship as one reviewable vertical slice first.

## Goals / Non-Goals

**Goals:**

- Make a canonical database-backed IAM permission catalog the only vocabulary used by built-in roles, custom roles, middleware, capability snapshots, route metadata, menus, and IAM editors.
- Resolve capabilities after authentication for the active signed organization context and attach them to `IamContext`.
- Keep existing synchronous handler guards while changing their input from fixed-role logic to the resolved permission set.
- Support multiple role bindings per principal, bounded resource selectors, expiry, resource relationships, and explicit non-transitive cross-organization grants.
- Make policy changes immediately observable through monotonically increasing organization versions.
- Remove the old low-level policy module, storage, API, UI copy, and OpenSpec requirements.

**Non-Goals:**

- Arbitrary user-authored ABAC expressions or a general-purpose policy programming language.
- Recursive relationship graph traversal in the first implementation; only direct relations and one bounded container inheritance hop are supported.
- Rewriting all browser and API URLs to include `/orgs/:orgId`; the signed token remains the trusted active-org source for this change.
- Treating frontend route hiding as a security boundary.

## Decisions

### 1. One database-backed IAM permission catalog

PostgreSQL SHALL store the canonical permission catalog in normalized IAM tables. Migration seed data bootstraps the catalog, but all runtime Rust and TypeScript consumers read it through the IAM repository/API. Each permission contains:

- canonical `resource.action` key;
- `platform` or `organization` scope;
- UI domain and translation keys;
- built-in role membership;
- optional feature requirement.

Database ownership avoids separate Rust/TypeScript registries drifting apart. TypeScript treats permission keys as validated strings returned by the IAM API, while backend input validation rejects unknown or wrong-scope keys.

Alternatives considered:

- A checked-in JSON registry: rejected because it becomes a second operational source of truth outside the database.
- Duplicated enums with parity tests: every permission edit requires coordinated source changes.
- Backend-returned arbitrary routes: rejected because React components and safe paths must remain statically compiled.

### 2. Resolve capabilities in IAM middleware

JWT claims identify only the principal, active organization, and IAM scope. API
tokens identify their database `role_id`. After credential verification, the
middleware asks `IamAccessService` for a versioned snapshot and attaches:

```text
IamContext {
  principal,
  organization,
  scope,
  display_role,
  roles,
  permissions,
  policy_version
}
```

Existing handlers can keep calling synchronous `Permission::require`; `Permission` maps to a catalog key and checks the attached set. Resource-aware handlers call the asynchronous IAM evaluator. This avoids converting unrelated handlers while ensuring every protected request uses current server-side data.

System scope bypasses organization bindings. A persisted platform-administrator
assignment makes the principal eligible for `_sys`, then
`iam_builtin_role_purposes` selects the concrete materialized platform role.
The snapshot's display name comes from that `iam_roles` row and its permissions
come from `iam_role_permissions`; runtime code never substitutes `Owner` or
grants every platform permission directly. Organization roles can never grant
`sys.*`.

### 3. Versioned storage and cache keys

New tables:

- `iam_role_bindings`
- `iam_relationships`
- `iam_cross_org_grants`
- `iam_policy_versions`
- `iam_permissions`
- `iam_builtin_roles`
- `iam_builtin_role_purposes`
- `iam_builtin_role_permissions`
- `iam_permission_bundles`
- `iam_permission_bundle_items`
- `iam_permission_catalog_versions`

`iam_roles` and `iam_role_permissions` are the runtime role catalog.
`iam_memberships` stores only the user/organization relationship and join time;
it has no role column. A principal can receive one or more roles exclusively
through `iam_role_bindings`. Membership creation/update accepts database role
ids and replaces the principal's organization-wide bindings transactionally.
Display roles and permission sets are both resolved from those bindings, so
there is no Rust role enum, role-string conversion, or `Role::allows` fallback.
Built-in role names and defaults exist only as database migration seed/catalog
data; runtime code queries them by role id or semantic purpose.
This applies to `_sys` as well: the seeded `platform_owner` is materialized
under the system organization, but its name and effective permissions are read
from `iam_roles` and `iam_role_permissions` on every versioned snapshot.

Every mutation increments `iam_policy_versions.version` in the same transaction. Snapshot cache keys include `(organization_id, principal_id, auth_scope, version)`; therefore revocation takes effect on the next request without waiting for a TTL. The web query key includes organization id and receives the version in the payload.

The legacy `rbac_policies` table is dropped.

### 4. Deterministic evaluation order

The service evaluates:

1. platform/scope boundary;
2. tenant isolation and membership;
3. hard system denies;
4. active cross-org grant when the requested resource belongs to another org;
5. role permissions;
6. direct resource relation and one container relation;
7. bounded conditions (`environment`, labels, start/expiry);
8. default deny.

User-authored explicit deny is not exposed in the first version. This keeps decisions explainable while retaining hard system denies.

`IamDecision` includes the matched binding/grant ids and policy version for audit and diagnostics. A batch endpoint accepts at most 100 decisions per call.

### 5. Static frontend route registry filtered by snapshots

`PRODUCT_ROUTES` remains the static registry for component-safe paths and navigation metadata. Role arrays are replaced by:

```ts
requiredPermissions?: PermissionKey[];
permissionMode?: 'all' | 'any';
requiredFeatures?: string[];
allowedScopes: AuthScope[];
organizationScoped: boolean;
```

`useProductAccess` fetches `/iam/capabilities` after authentication. Until the initial snapshot is ready, the route guard renders a neutral loading state and does not mount the destination. Sidebar, Settings navigation, IAM navigation, command palette, shortcuts, and direct-route guard call the same access function.

Organization switches clear the old query cache before installing the new session; the next render cannot reuse capabilities from the previous organization.

### 6. Semantic IAM surfaces

Role permissions are grouped by catalog domain and can be initialized from database-defined bundles. The role key is generated from the role name until edited and becomes immutable after creation.

The raw policy page is replaced by an access-grant editor whose steps are principal, organization, resource selector, permissions, and constraints. Selectors are server-provided users/teams/roles/resources; administrators never type subject ids or action strings. Cross-organization sharing is a separate workflow.

## Risks / Trade-offs

- **[Risk] A permission key is assigned to a route but not to its backend endpoint.** → Registry contract tests map every protected route family to the same key used by middleware, and integration tests verify both visible and direct-access behavior.
- **[Risk] Per-request snapshot resolution adds database load.** → Cache by policy version and use one bounded aggregate query on misses.
- **[Risk] Role mutation and version bump can diverge.** → Repositories execute both in one transaction; mutation tests assert version increments.
- **[Risk] A session outlives a role mutation.** → JWTs carry no role; middleware always resolves roles and permissions from the current server-side bindings and policy version.
- **[Risk] A cross-org grant leaks resource existence.** → Invalid or unauthorized foreign resource ids return `404`, and grants are non-transitive by schema and evaluator rules.
- **[Trade-off] Root URLs still rely on the active signed org token.** → This remains safe because the server ignores arbitrary client org headers, but multi-org tabs are not solved until the separate URL-context migration.

## Migration Plan

1. Add the database catalog and IAM access tables; development databases use a clean schema with role-free memberships and database role bindings.
2. Wire `IamAccessService`, enrich `IamContext`, and expose capabilities/evaluation endpoints.
3. Convert role validation and IAM mutation paths to catalog keys and version bumps.
4. Switch the web shell and management navigation to capability-driven checks.
5. Replace the raw policy UI with semantic grants.
6. Delete old policy modules/routes/repository references and drop `rbac_policies`.
7. Run backend unit/integration tests, frontend unit/Playwright tests, clippy, lint, typecheck, and OpenSpec validation.

Rollback during development is a database reset to the previous migration set plus restoring the prior binary/web bundle; no mixed-version compatibility is promised.

## Open Questions

- The later organization-URL change must choose stable id versus slug and define deep-link redirects.
- Resource repositories need a common owner/container lookup interface before every resource type can support inherited relations; this change starts with direct ids and folder selectors.
