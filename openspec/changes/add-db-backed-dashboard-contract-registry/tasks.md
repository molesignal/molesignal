## 1. Registry Domain and Contract Runtime

- [x] 1.1 Add Dashboard contract kind/reference/version/binding/bundle domain types and repository port with immutable publication, active load, and trusted activation operations.
- [x] 1.2 Expose validated visualization-manifest parsing and add resolved contract snapshot/resolver abstractions with explicit built-in test support.
- [x] 1.3 Implement the application registry service to build the canonical bundle, verify dialect/kind/version/hash/compiler compatibility, and cache one active compiled revision.

## 2. PostgreSQL Persistence

- [x] 2.1 Extend the initial migration with immutable contract-version and active-binding tables, integrity constraints/indexes, and non-null contract pin columns on Dashboard drafts.
- [x] 2.2 Implement the PostgreSQL contract repository with idempotent immutable publication, joined active-bundle loading, and transactional revision-incrementing activation.
- [x] 2.3 Extend Dashboard draft row mapping/create/consume SQL for binding/hash pins and transactionally compare the locked draft with the current active binding before Dashboard insertion.

## 3. Runtime Wiring

- [x] 3.1 Refactor Dashboard JSON Schema/semantic validation and the authoring compiler to accept an explicit resolved model validator and visualization manifest while retaining built-in test entry points.
- [x] 3.2 Resolve the active DB bundle for Dashboard authoring capability discovery, preparation, draft preview/reference validation, and draft persistence.
- [x] 3.3 Resolve the active DB bundle for native create/update, Grafana normalization, and create-from-draft; return `DRAFT_STALE` for any pin mismatch.
- [x] 3.4 Publish built-in contracts during bootstrap, validate the initial active bundle, inject one database-backed resolver into Dashboard services, and fail startup closed on registry inconsistency.

## 4. Tests and Operations

- [x] 4.1 Add registry unit tests for canonical publication inputs, compiled cache replacement, malformed/hash-mismatched/disabled/unsupported bundles, and built-in compatibility.
- [x] 4.2 Add PostgreSQL integration coverage for idempotent publication, conflicting immutable versions, atomic activation/rollback, joined resolution, and concurrent revision behavior.
- [x] 4.3 Extend Dashboard authoring/service tests for persisted pins, stale binding rejection before proposal/execution, atomic race re-check, and matching exactly-once consumption.
- [x] 4.4 Document ownership, startup publication, activation/rollback order, database outage behavior, and the prohibition on model/tenant contract selection.

## 5. Verification

- [x] 5.1 Verify all new production files remain below 500 lines and registry responsibilities stay within domain/application/infra/bootstrap boundaries.
- [x] 5.2 Run one final Rust format/license/lint/test round, rerun only failed items after fixes, and validate the OpenSpec change strictly.
