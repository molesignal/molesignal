## Why

Dashboard authoring contracts are currently embedded directly into each process, so the runtime cannot prove which published contract revision and capability binding produced a draft. Persisting immutable published snapshots and active bindings in PostgreSQL gives every node a shared, auditable runtime revision while keeping Git-owned canonical assets reviewable with the compiler and renderer.

## What Changes

- Add an immutable PostgreSQL registry for versioned Dashboard model schemas, authoring schemas, and visualization manifests, identified by canonical SHA-256 hashes.
- Publish the deployed built-in contract bundle during bootstrap and atomically maintain the active `dashboard.authoring.v1` binding.
- Resolve and cache the active bundle from PostgreSQL for capability discovery, authoring validation, compilation, and native Dashboard validation; missing, disabled, inconsistent, or unsupported bindings fail closed.
- Pin authoring/model/visualization schema hashes and the binding revision on each Dashboard draft, and reject execution when the active runtime binding no longer matches the reviewed draft.
- Add repository, runtime, bootstrap, migration, regression, and rollback documentation coverage without allowing AI tools to publish or mutate contracts.

## Capabilities

### New Capabilities

- `dashboard-contract-registry`: Immutable contract publication, active capability bindings, runtime resolution/cache behavior, draft revision pinning, fail-closed safety, and rollback semantics for Dashboard authoring.

### Modified Capabilities

None.

## Impact

- Affects Dashboard domain/application services, authoring compiler and validation boundaries, PostgreSQL persistence, bootstrap composition, draft persistence, and integration tests.
- Extends the initial development migration with registry tables and draft revision columns; no new public HTTP mutation API is introduced.
- Retains `contracts/dashboard/` as the canonical reviewed source and Web generation input; PostgreSQL stores published runtime snapshots rather than becoming an unreviewed compiler capability source.
