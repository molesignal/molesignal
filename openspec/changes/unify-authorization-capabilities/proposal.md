## Why

MoleSignal currently computes backend IAM access from a fixed role enum while the web shell independently hides routes from a second role table. This split already causes routes to remain visible after an organization switch and will not safely scale to custom roles, resource grants, or cross-organization sharing.

## What Changes

- Introduce one canonical database-backed `resource.action` IAM permission catalog shared by role definitions, backend enforcement, capability snapshots, route metadata, menus, buttons, and tests.
- Resolve every authenticated request through the active organization context into a versioned capability snapshot; backend handlers remain the security boundary.
- Add multi-role bindings, resource relationships, constrained conditions, and explicit cross-organization grants with expiry, revocation, non-transitivity, and audit metadata.
- Add `GET /api/v1/iam/capabilities` and batch evaluation contracts so the web client can build navigation without issuing one IAM decision request per control.
- Replace role-based frontend route checks with a static route registry filtered by permissions, features, scope, and the active organization. Direct URL access uses the same registry and never mounts a denied page.
- Upgrade IAM role and access UIs from raw action/resource/subject identifiers to grouped permissions, permission bundles, and semantic principal/resource selectors.
- **BREAKING**: remove the old low-level policy module, `/auth/policies` API, and `rbac_policies` storage. There is no compatibility layer because the product is still in development.
- Keep `_sys` as an immutable platform scope. Platform permissions cannot be granted by organization roles, and the bootstrap root receives the database-seeded platform Owner role in `_sys`.

## Capabilities

### New Capabilities

- `iam-capabilities`: Database-backed permission catalog, role bindings, resource relationships, conditional evaluation, versioned capability snapshots, batch decisions, and cross-organization grants.

### Modified Capabilities

- `iam`: IAM middleware enriches the principal and organization context with resolved capabilities instead of authorizing from a fixed role enum alone.
- `web-shell`: Static route metadata declares required permissions/features and is the sole source for navigation visibility and direct-route guarding.
- `web-iam`: Roles and grants use grouped atomic permissions and semantic selectors instead of a low-level policy form.

## Impact

- Backend: `src/domain/iam`, `src/app/iam`, HTTP IAM middleware/routes, `AppState` wiring, IAM repositories, audit integration, and PostgreSQL migrations.
- Frontend: auth/capability API and state, `web/src/product/ia.ts`, product access calculation, route guard, sidebar/settings/IAM navigation, role/grant editors, and mock backend fixtures.
- Contracts: new capability/evaluation/grant endpoints; removed `/api/v1/auth/policies`; role permission keys change from broad bundles such as `data.read` to canonical atomic keys.
- Operations: permission mutations bump an organization policy version so cached snapshots and browser queries become invalid immediately.
