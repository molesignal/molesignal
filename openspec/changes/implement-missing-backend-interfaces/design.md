## Context

The backend already contains repositories for alerts, schedules, scheduled reports, functions, and enrichment key/value rows. Some HTTP handlers still reject writes with a generic "not implemented" error even though the domain and persistence models exist. Other pages need lightweight API surfaces: invitations need a small durable table; correlation providers and report templates can be deterministic backend-provided catalogs.

## Decisions

- Keep DTOs explicit at route boundaries. HTTP create/update requests will be converted into domain structs and assigned server-side `id`, `org_id`, and timestamps.
- For alert rules, preserve evaluation fields on update: `last_eval_at` and `last_state` come from the existing row, not from user input.
- For schedules, implement override add/remove by updating the existing schedule row because overrides are stored as JSON on `schedules`.
- For enrichment tables, add list/upsert/delete routes over the existing `enrichment_kv` repository and refresh the in-memory `EnrichmentTable` after mutations.
- For invitations, add a simple PostgreSQL repository and table with pending/accepted/revoked status. The first implementation lists, creates, resends, and revokes pending invitations; accepting invitations can be a later auth-flow concern.
- For report templates and correlation providers, return stable built-in catalogs from backend routes. They are deterministic API surfaces without new persistence requirements.
- For function dry-run, execute VRL using the existing runtime. JavaScript dry-run returns the same runtime-disabled validation path unless the configured javascript runtime is available in a later change.

## Risks

- Alert DTO shape may not match every current frontend draft. Mitigation: accept permissive JSON defaults and map frontend fields conservatively.
- Invitation delivery is not implemented. Mitigation: mark invite rows as pending and expose resend/revoke; actual email delivery can integrate later.
- Function dry-run could be expensive for large samples. Mitigation: enforce the same source limit as function compile and execute a single JSON payload.
