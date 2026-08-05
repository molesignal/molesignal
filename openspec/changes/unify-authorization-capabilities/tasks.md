## 1. IAM Permission Catalog And Contracts

- [x] 1.1 Add normalized database tables and migration seeds for platform/organization permissions, domains, translations, built-in roles, relation mappings, and bundles.
- [x] 1.2 Add Rust IAM catalog repository contracts, database loading, scope validation, and typed compatibility mappings.
- [x] 1.3 Add TypeScript catalog types/helpers driven exclusively by the IAM API.

## 2. Persistence And Versioning

- [x] 2.1 Add IAM migrations for role bindings, relationships, cross-org grants, policy versions, membership backfill, catalog storage, and `rbac_policies` removal.
- [x] 2.2 Implement the IAM repository for snapshot resolution, binding CRUD, and atomic policy-version increments.
- [x] 2.3 Implement bounded relationship and cross-org grant persistence with active-window filtering.

## 3. Backend IAM

- [x] 3.1 Implement `IamAccessService`, versioned capability snapshots, deterministic decisions, and batch evaluation.
- [x] 3.2 Enrich `IamContext` in middleware and make existing typed permission guards read resolved capabilities.
- [x] 3.3 Expose permission catalog, capabilities, role-binding, relationship, grant, share-target, and evaluate-batch IAM endpoints.
- [x] 3.4 Validate role permissions through the database catalog and invalidate policy versions on role/membership mutations.
- [x] 3.5 Remove the old policy module, adapter, low-level route/repository wiring, license gate references, and stale terminology.
- [x] 3.6 Add backend unit and HTTP integration coverage for viewer/custom/system/revocation/default-deny/cross-org cases.
- [x] 3.7 Remove the fixed application role enum and membership role column; resolve multi-role display metadata and permissions from database role bindings.
- [x] 3.8 Materialize the `_sys` platform role and resolve its display name and permissions from `iam_roles` / `iam_role_permissions` instead of a hard-coded Owner grant.

## 4. Capability-Driven Web Shell

- [x] 4.1 Add the capabilities API client and organization-keyed React Query bootstrap.
- [x] 4.2 Replace route role arrays with permission/feature/scope metadata in the static product route registry.
- [x] 4.3 Make sidebar, management navigation, command palette, shortcuts, and direct-route guarding consume the same capability access function without denied-content flash.
- [x] 4.4 Clear capability state on organization switch and cover stale-organization regressions.

## 5. IAM Experience

- [x] 5.1 Render role permissions grouped by catalog domain with permission bundles and automatic editable-before-create role keys.
- [x] 5.2 Replace the raw policy form with semantic principal/resource/permission/constraint selectors backed by binding APIs.
- [x] 5.3 Add the separate cross-organization sharing workflow with expiry, acceptance, revocation, and non-transitive messaging.
- [x] 5.4 Update English/Chinese IAM copy and deterministic mock-backend fixtures.
- [x] 5.5 Remove legacy frontend JWT `role`/`platform_permissions` state and make role selectors consume database role ids.

## 6. Validation And Documentation

- [x] 6.1 Add frontend unit/Playwright coverage for dynamic visibility, direct denial, role editing, and organization switching.
- [x] 6.2 Update architecture/API documentation and remove the obsolete policy specification.
- [x] 6.3 Run Rust tests, clippy, frontend tests/lint/typecheck/Playwright, migration checks, and `openspec validate --all --strict`.
