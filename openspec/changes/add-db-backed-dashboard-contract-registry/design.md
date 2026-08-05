## Context

Dashboard model v2, authoring v1, and visualization v1 are language-neutral canonical files compiled into the Rust binary and copied into Web generated assets. That prevents schema drift, but runtime drafts currently record only numeric versions and a compiler version; there is no shared database record proving the exact canonical documents and binding used by every process. MoleSignal is multi-node, PostgreSQL-backed, and still pre-first-release, so the schema can be folded into the initial migration.

The compiler and renderer remain code-coupled to supported contract families. The registry therefore stores reviewed published snapshots and controls which compatible snapshot is active; it is not a general-purpose schema editor and does not let a model invent compiler capabilities.

## Goals / Non-Goals

**Goals:**

- Persist immutable, canonically hashed Dashboard contract documents in PostgreSQL.
- Maintain one atomic global active binding for `dashboard.authoring.v1` across all nodes.
- Load and compile the active bundle at runtime, cache it by binding revision/hash, and fail closed on database, integrity, dialect, version, or compatibility failures.
- Use the resolved authoring schema, model schema, and visualization manifest for capability discovery, preparation, compilation, semantic validation, and native write validation.
- Pin the active revision and all document hashes on a draft and re-check them through atomic consumption.
- Make activation/rollback available through an application/repository operation while keeping it unavailable to AI tools and public HTTP APIs.

**Non-Goals:**

- Moving the Git canonical assets or Web generation source into PostgreSQL.
- Building an arbitrary schema editor or administrator UI in this change.
- Allowing organization-specific Dashboard model contracts.
- Supporting a visualization/compiler version that the running binary does not understand.
- Changing MCP schema persistence, which already has its own organization-scoped synchronization lifecycle.

## Decisions

### Immutable version rows plus an atomic binding row

`intelligence_contract_versions` stores `(contract_key, version, kind, dialect, document, schema_hash, status, published_at_micros)` with a unique key/version and key/version/hash identity. Published rows are inserted idempotently and never updated in place. `intelligence_capability_contract_bindings` stores the three exact references, compiler version, enabled flag, monotonically increasing revision, and update time for `dashboard.authoring.v1`.

This is preferred over one mutable JSONB settings row because immutable rows preserve auditability, allow deterministic rollback, and prevent a schema from changing beneath a hash-pinned draft.

### Git publishes; PostgreSQL activates

Bootstrap parses and validates the three embedded canonical assets, computes canonical SHA-256 hashes, and inserts missing published versions. It creates the default binding only when none exists; it does not overwrite an operator-selected compatible binding on every restart. A registry service exposes activation to trusted internal callers and validates a candidate bundle before the repository atomically increments the active revision.

This keeps compiler-affecting changes code-reviewed and deployment-coupled while making the runtime choice and rollback database-owned. No public mutation route is added.

### Runtime resolver with a single-revision compiled cache

Every contract-sensitive operation reads the active binding/bundle from the repository. A process-local cache retains only the most recently compiled revision. A cache hit requires matching revision and all three persisted hashes; a binding change replaces the cache entry. JSON Schema validators and the visualization manifest are compiled/parsed only after canonical hash, dialect, kind, version, manifest consistency, and current binary compatibility checks succeed.

The binding read is not hidden behind a long TTL because a reviewed rollback must take effect promptly and serving a stale validator is more dangerous than the small cost on low-volume Dashboard writes. Repository errors fail closed rather than falling back to embedded contracts in production. Unit tests may use the explicit built-in resolver.

### Explicit contract snapshot injection

The authoring compiler and semantic validator gain variants that accept a resolved manifest/model validator instead of consulting process-global values. `DashboardAuthoringService` resolves one snapshot for capability discovery/preparation. `DashboardService` resolves the active snapshot for native create/update/import normalization and draft creation. Existing convenience constructors use the built-in resolver for isolated tests; bootstrap always injects the database-backed resolver.

This avoids global mutable state and preserves dependency direction. The shared JSON Schema evaluator remains in `shared`, contract records/ports live in `domain::dashboard`, orchestration/cache lives in `app::dashboard`, PostgreSQL lives in `infra`, and assembly lives in `bootstrap`.

### Draft pins and atomic re-check

Dashboard drafts add `contract_binding_revision`, `authoring_schema_hash`, `model_schema_hash`, and `visualization_schema_hash`. Preview/reference/execution compares all four values with the active resolved snapshot and returns `DRAFT_STALE` on mismatch. The PostgreSQL `consume_and_create` transaction also reads the current binding under the same transaction and compares it with the locked draft before inserting the Dashboard.

The second check closes the race where a binding changes after application validation but before draft consumption.

## Risks / Trade-offs

- **[PostgreSQL outage blocks Dashboard writes]** → This is intentional fail-closed behavior; reads of already persisted Dashboards remain unaffected, and health/error telemetry identifies registry resolution failures.
- **[Rolling deployments encounter an unsupported active compiler revision]** → Activation validates against the running binary; operators publish compatible assets first and activate only within the documented deployment window. A binding can be rolled back atomically.
- **[Per-write binding reads add database traffic]** → Dashboard writes and authoring calls are low volume; only compilation is cached. A future event/notification cache can optimize reads without changing the contract.
- **[DB rows diverge from Git files]** → Bootstrap only inserts immutable canonical snapshots and rejects an existing key/version whose hash differs. Contract drift tests continue to protect Rust/Web generated consumers.
- **[A direct SQL operator selects inconsistent references]** → Runtime validates all hashes, kinds, versions, manifest cross-links, and compiler support before use and fails closed.

## Migration Plan

1. Extend the initial migration with immutable contract and binding tables plus non-null draft pin columns.
2. Deploy code that publishes the embedded v2/v1/v1 bundle and creates revision 1 of the binding when absent.
3. Resolve the binding once during bootstrap before exposing services; abort startup on publication or initial resolution failure.
4. For a future contract release, deploy a binary containing and publishing the new compatible snapshot, then activate it through the trusted registry service/operational path.
5. Roll back by atomically activating the previous three references; new authoring uses the old revision and outstanding drafts from another revision become stale and must be prepared again.
6. Code rollback remains safe while the active binding references a bundle supported by the old binary; otherwise restore that binding before restarting the old binary.

## Open Questions

- A future change may add an administrator API/UI and activity audit event for activation. This change deliberately exposes only the internal service/repository seam so contract mutation authority is not broadened implicitly.
