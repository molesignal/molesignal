# Dashboard authoring rollout and rollback

## Enablement prerequisites

Dashboard authoring is server-owned and has no runtime code-generation path. Enable it only after:

1. the canonical `contracts/dashboard/` assets and generated Web copies pass their drift check;
2. the `intelligence_dashboard_drafts` and execution uniqueness indexes exist;
3. the organization is licensed for `intelligence`;
4. intended users have `intelligence.use`, `dashboards.create`, and the query permissions needed by preflight;
5. the active Agent Profile and Toolset include `get_dashboard_capabilities` and `prepare_dashboard`;
6. `propose_dashboard_creation` has an explicit organization policy. Its effective floor is Confirmation, even if an L1 policy is configured as Automatic.

Start with proposal disabled or omitted from the active Profile. This enables prepare/preview-only operation and exercises compilation, dry-run, TTL, rendering, and contract telemetry without allowing Dashboard creation. Enable proposal after preview failures and preflight warning rates are acceptable.

## Runtime controls

- Disable all authoring: remove the three Dashboard tools from the active Agent Profile/Toolset, or disable their tool policies.
- Keep previews but stop creation: disable only `propose_dashboard_creation`. Chat automatically degrades to preview-only.
- Tighten creation: set proposal execution to `single_approval` or `dual_approval`. Policy may tighten the Confirmation floor but cannot bypass it.
- Stop new Intelligence traffic globally: remove the `intelligence` license feature. Existing native Dashboards remain readable.

Changes take effect on the next request because Profile, Toolset, policy, IAM, and license state are resolved server-side. Model-supplied organization, user, folder, approval, schema version, layout, or compiled model fields do not override those controls.

## Operational checks

Monitor Dashboard authoring tool-call outcomes, structured validation codes, preflight timeouts, draft expiry, approval state, execution state, and `dashboard.created_from_ai_draft` activity. A successful execution must contain a Dashboard ID/route, `draft_consumed = true`, and exactly one execution row for its approval. Activity-audit summaries may include IDs, routes, hashes, and summaries, but never the compiled model or credentials. Federation CUD uses the normal native Dashboard resource envelope required by cross-cluster synchronization and is emitted only for the first successful draft consumption.

Before widening access, verify these paths in both Chinese and English:

- missing topic/data/time asks a clarification and does not force preparation;
- valid intent creates a persisted preview but no Dashboard;
- expired, stale, hash-mismatched, or cross-organization drafts fail closed;
- confirmation/single/dual policies show the corresponding UI state;
- concurrent execute requests return the same execution and create one Dashboard;
- the resulting `/dashboards/{id}` route renders through the normal Dashboard engine.

## Rollback

1. Disable `propose_dashboard_creation` first. This immediately prevents new approval requests while preserving preview access.
2. If needed, disable `prepare_dashboard` and `get_dashboard_capabilities` or remove the capability from active Profiles/Toolsets.
3. Allow ready drafts and approvals to expire; do not delete or mutate them during rollback. A draft TTL also caps its approval expiry.
4. Preserve consumed drafts, executions, activity records, and federation outbox entries for idempotency and audit evidence.
5. Roll back application code only after proposal is disabled. Do not downgrade the canonical Dashboard v2 write validator or remove persisted schema/hash columns while old rows exist.

Rollback does not delete Dashboards already created. They are ordinary native Dashboard records and remain manageable through existing Dashboard APIs. Re-enablement can reuse no ready draft whose contract/compiler revision is stale; users must prepare a fresh preview.
