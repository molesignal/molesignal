# Dashboard contracts

This directory is the language-neutral source of truth for Dashboard authoring and persisted Dashboard models.

## Ownership

- `model/v2.schema.json` describes the native model persisted by MoleSignal and rendered by Dashboard Engine.
- `authoring/v1.schema.json` describes the smaller intent-oriented payload accepted from AI authoring tools.
- `visualizations/v1.json` is the compiler capability catalog and owns visualization defaults and compatibility.
- `fixtures/valid/` and `fixtures/invalid/` are shared by Rust and Web contract tests.

The Dashboard domain owns persisted-model and authoring contracts. Dashboard Engine owns renderer option evolution, but changes that affect the authoring compiler must update the visualization catalog and shared fixtures in the same change.

## Generated consumers

Run `pnpm -C web dashboard-contracts:sync` after changing a canonical asset. Generated files under `web/src/dashboard-engine/contracts/generated/` must not be edited by hand. CI uses `dashboard-contracts:check` to fail on drift.

JSON Schema validates structure. Cross-field rules, budgets, recursive identifier uniqueness, grid bounds, and query/visualization compatibility are enforced by the server-side Dashboard semantic validator.

Native writes reject unknown fields except at explicit extension points (`extensions`, query payloads, visualization options, transformation options, field config, and link variables). Grafana import uses a separate compatibility validation mode so unknown vendor fields remain round-trippable.

## Runtime registry operations

Git remains the publication authority. PostgreSQL is the runtime registry: `intelligence_contract_versions` stores immutable, canonically hashed snapshots, while `intelligence_capability_contract_bindings` atomically selects the model, authoring, and visualization versions used by `dashboard.authoring.v1`. Startup publishes the embedded snapshots idempotently, creates the default binding only when absent, resolves it through the same validation path used at runtime, and aborts startup if the database record is missing, corrupted, disabled, or incompatible with the running compiler.

Dashboard authoring capabilities, preparation, preview/reference validation, native create/update/import validation, and draft execution all resolve the active database binding. A database outage therefore fails these operations closed; the process never silently falls back to a different embedded schema. Existing persisted Dashboard reads do not depend on registry resolution.

Contract releases and rollbacks use this order:

1. Merge and deploy the reviewed canonical assets together with compiler/renderer support and generated Web consumers.
2. Let startup publish the new immutable versions and verify every participating binary can resolve the current binding.
3. Activate all three reviewed references together through the trusted internal registry operation; activation increments one global binding revision atomically.
4. Monitor authoring and native-write validation. Drafts pinned to a previous revision intentionally return `DRAFT_STALE` and must be prepared again.
5. To roll back, atomically reactivate the previous three references before rolling code back when the older binary cannot understand the current bundle.

Do not update published version rows in place or activate references with ad-hoc SQL. AI/MCP tools, model output, tenant settings, and public HTTP APIs are not contract-management authorities and cannot publish or select Dashboard contracts. The binding is global because persisted Dashboard models and the running compiler/renderer must share one compatible contract family.
