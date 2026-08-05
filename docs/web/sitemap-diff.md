# Web sitemap diff: molesignal ↔ openobserve

> Reference: `/Users/gagral/code/openobserve/web/src/composables/shared/router.ts`
> + `useManagementRoutes.ts` + `useProRoutes.ts` + `useIngestionRoutes.ts`.
>
> Each row notes the backend endpoint(s) the route depends on and whether
> they exist in `src/api/http/routes/`. Priorities follow the
> `web-feature-parity` proposal: **P0** lands in this change, **P1** in
> `web-feature-parity-settings`, **P2** in `web-feature-parity-misc`.
> ✓ = implemented in this change, ☐ = still pending in its assigned bucket.

## Status legend

| Marker | Meaning |
| --- | --- |
| ✓ | molesignal route exists after this change |
| ✓¹ | already existed before this change |
| ☐ | not implemented yet (priority noted in next column) |
| 🔌 | backend endpoint exists in `src/api/http/routes/` |
| 🚧 | backend endpoint missing — page renders "Awaiting backend" empty state |
| — | n/a (route does not apply to molesignal) |

## Top-level: 1:1 already done

| openobserve path | molesignal path | Status | Backend |
| --- | --- | --- | --- |
| `/` (home) | `/home` | ✓¹ | — |
| `/logs` | `/logs` | ✓¹ | 🔌 `routes/query.rs` |
| `/metrics` | `/metrics` | ✓¹ | 🔌 `routes/metrics.rs` + `query.rs` |
| `/traces` | `/traces` | ✓¹ | 🔌 `routes/traces.rs` |
| `/streams` | `/streams` | ✓¹ | 🔌 `routes/web/streams` |
| `/dashboards` | `/dashboards` | ✓¹ | 🔌 `routes/dashboards.rs` |
| `/dashboards/view` (`/dashboards/:id`) | `/dashboards/:id` | ✓¹ | 🔌 |
| `/dashboards/add_panel` (`new/edit`) | `/dashboards/new/edit` + `/dashboards/:id/edit` | ✓¹ | 🔌 |
| `/alerts` | `/alerts` | ✓¹ | 🔌 `routes/alerting.rs` |
| `/reports` | `/reports` | ✓¹ | 🔌 `routes/scheduled_reports.rs` |
| `/pipeline/pipelines` | `/pipelines` | ✓¹ | 🔌 `routes/scheduled_pipelines.rs` |
| `/settings` (root) | `/settings` | ✓¹ | mixed |
| `/ingest/*` (legacy) | `/ingest/:category/:source` | ✓¹ | — (docs only) |

## P0 — APM and User Experience

Backend APM has dedicated aggregate APIs and canonical routes:

| molesignal path | Status | Backend |
| --- | --- | --- |
| `/apm/overview` | ✓ | 🔌 `GET /api/v1/apm/overview` |
| `/apm/services` | ✓ | 🔌 `GET /api/v1/apm/services` |
| `/apm/services/:service` | ✓ | 🔌 `GET /api/v1/apm/services/:service` |
| `/apm/transactions` | ✓ | 🔌 `GET /api/v1/apm/transactions` |
| `/apm/dependencies` | ✓ | 🔌 `GET /api/v1/apm/dependencies` |
| `/apm/errors` | ✓ | 🔌 `GET /api/v1/apm/errors` |
| `/apm/errors/:fingerprint` | ✓ | 🔌 `GET /api/v1/apm/errors/:fingerprint` |
| `/apm/deployments` | ✓ | 🔌 `GET /api/v1/apm/versions/compare` |

RUM remains an independent canonical product:

| openobserve | molesignal | Status | Backend |
| --- | --- | --- | --- |
| `/rum` | `/rum/overview` | ✓ | 🚧 (queries RUM streams) |
| `/rum/applications` | `/rum/applications` | ✓ | 🚧 (aggregates `rum_sessions`) |
| `/rum/sessions` | `/rum/sessions` | ✓ | 🚧 (`POST /rum/sessions` ingests; `GET /rum/sessions` lists `rum_sessions`) |
| `/rum/sessions/view/:id` | `/rum/sessions/view/:id` | ✓ | 🚧 (queries `rum_sessions` + `rum_actions` streams) |
| `/rum/pages` | `/rum/pages` | ✓ | 🚧 (aggregates `rum_actions`) |
| `/rum/errors` | `/rum/errors` | ✓ | 🚧 (queries `rum_errors` stream) |
| `/rum/errors/view/:id` | `/rum/errors/view/:id` | ✓ | 🚧 (queries `rum_errors` stream) |
| `/rum/performance/*` | `/rum/performance/*` | ✓ | 🚧 (aggregates RUM streams) |
| `/rum/session-replay` | `/rum/session-replay` | ✓ | 🔌 `GET /rum/replay/:session_id` |
| `/rum/source-maps` | `/rum/settings/source-maps` | ✓ | 🔌 `GET /debug-artifacts` |
| `/rum/upload-source-maps` | `/rum/settings/source-maps/upload` | ✓ | 🔌 `POST /debug-artifacts` (multipart) |

Note: backend RUM ingestion uses
`POST /rum/{sessions,actions,errors,replay}`. The browser pages query the
`rum_sessions` / `rum_actions` / `rum_errors` Logs streams and degrade gracefully
when those streams have no data. `/apm/user-experience/*` remains a compatibility entry and
preserves the full suffix, path parameters, query string, and fragment when
redirecting to canonical RUM. `/services*` similarly redirects to
`/apm/services*`, while `/apm/versions/compare` redirects to `/apm/deployments`.

## P0 — Functions + Enrichment (2 routes, this change)

| openobserve | molesignal | Status | Backend |
| --- | --- | --- | --- |
| `/pipeline/functions` | `/functions` | ✓ | 🔌 `routes/functions.rs` (`GET/POST /functions`, `GET/PUT/DELETE /functions/:id`) |
| (inline editor) | `/functions/:id` (drawer) | ✓ | 🔌 same |
| `/pipeline/enrichment-tables` | `/enrichment-tables` | ✓ | 🚧 (no `/enrichment_tables` endpoint yet — page lists nothing + shows note) |

## P0 — Pipeline editor (6 routes, this change)

| openobserve | molesignal | Status | Backend |
| --- | --- | --- | --- |
| `/pipeline/pipelines/new` | `/pipelines/new` | ✓ | 🔌 `POST /scheduled_pipelines` |
| `/pipeline/pipelines/detail` | `/pipelines/:id` | ✓ | 🔌 `GET /scheduled_pipelines/:id` + `/runs` |
| `/pipeline/pipelines/edit` | `/pipelines/:id/edit` | ✓ | 🔌 `GET/PUT /scheduled_pipelines/:id` |
| `/pipeline/pipelines/import` | `/pipelines/import` | ✓ | 🚧 (no dedicated `/import` endpoint; UI POSTs adapted body to `/scheduled_pipelines`) |
| `/pipeline/pipelines/history` | `/pipelines/:id/history` | ✓ | 🔌 `GET /scheduled_pipelines/:id/runs` |
| `/pipeline/pipelines/backfill` | `/pipelines/:id/backfill` | ✓ | 🔌 `POST /scheduled_pipelines/:id/backfill` (returns search-job for monitoring) |

## P0 — IAM (7 routes, this change)

| openobserve | molesignal | Status | Backend |
| --- | --- | --- | --- |
| `/iam/users` | `/iam/users` | ✓ | 🔌 `GET /users`, `POST /users`, `DELETE /users/:id` |
| `/iam/serviceAccounts` | `/iam/service-accounts` | ✓ | 🚧 (no service-account endpoint — page uses `/users` filtered + note) |
| `/iam/organizations` | `/iam/organizations` | ✓ | 🔌 `GET /orgs`, `POST /orgs`, `POST /orgs/:id/members` |
| `/iam/groups` (+ `edit/:group_name`) | `/iam/groups` | ✓ | 🔌 `routes/iam_access.rs` (`/iam/role-bindings`, `/iam/cross-org-grants`) |
| `/iam/roles` (+ `edit/:role_name`) | `/iam/roles` | ✓ | 🚧 (roles are an enum — page renders a read-only matrix) |
| `/iam/quota` | `/iam/quota` | ✓ | 🚧 (no `/quota` endpoint — page shows "Awaiting backend") |
| `/iam/invitations` | `/iam/invitations` | ✓ | 🚧 (no `/invitations` endpoint) |

## P1 — Settings sub-routes (16) — `web-feature-parity-settings`

| openobserve | molesignal | Status | Backend |
| --- | --- | --- | --- |
| `/settings/general` | `/settings/general` | ✓ | 🔌 `GET /users/:id` (preferences pending) |
| `/settings/organization` | `/settings/general` | ✓ | Current workspace settings are consolidated into General; the legacy URL redirects |
| Notify settings | `/settings/notify/{connectors,users,policies,templates,defaults,deliveries}` | ✓ | 🔌 `routes/notify/*` |
| `/settings/cipher_keys` | `/settings/cipher_keys` | ✓ | 🔌 `routes/cipher_keys.rs` |
| `/settings/regex_patterns` | `/settings/regex_patterns` | ✓ | 🔌 `routes/regex_patterns.rs` |
| `/settings/ai_toolsets` | `/settings/ai_toolsets` | ✓ | 🔌 `routes/ai_toolsets.rs` (OSS stub returns empty list; writes 403) |
| `/settings/model_pricing` | `/settings/model_pricing` | ✓ | 🔌 `routes/model_prices.rs` |
| `/settings/query_management` | `/settings/query_management` | ✓ | 🔌 `routes/query.rs` (`/query/running` + `/query/{id}/cancel`) |
| `/settings/storage_settings` | `/settings/storage_settings` | ✓ | 🔌 `routes/storage_providers.rs` |
| `/settings/nodes` | `/settings/nodes` | ✓ | 🔌 `routes/clusters.rs` (federated_search license) |
| `/settings/domain_management` | `/settings/domain_management` | ✓ | 🔌 `routes/domains.rs` |
| `/settings/correlation` | `/settings/correlation` | ✓ | 🔌 `routes/web/correlation` (read-only) |
| `/settings/organization_management` | `/settings/organization_management` | ✓ | 🔌 `routes/iam_directory.rs` (`/orgs`) |
| `/settings/pipeline_destinations` | `/settings/pipeline_destinations` | ✓ | 🔌 `routes/connectors.rs` |
| `/settings/license` | `/settings/license` | ✓ | 🔌 `routes/license.rs` |

## P2 — Sub-routes + misc (`web-feature-parity-misc`)

| openobserve | Planned molesignal | Status | Backend |
| --- | --- | --- | --- |
| `/logs/inspector` | `/logs/inspector` | ✓ | 🔌 `routes/search_jobs.rs` |
| `/promql-builder` | `/metrics` (inline Builder mode) | ✓ | 🔌 `routes/query.rs` (`language=promql`) |
| `/traces/trace-details` | `/traces/:id` | ✓ | 🔌 `routes/web/trace.rs` |
| `/traces/session-details` | `/traces/session/:id` | ✓ | 🔌 `routes/query.rs` (SQL on `traces` filtered by `attributes['session.id']`) |
| `/streams/stream-explore` | `/streams/:id` | ✓ | 🔌 derived from `routes/web/search.rs` (no per-stream endpoint) |
| `/service-graph` | `/service-graph` | ✓ | 🔌 `routes/web/topology.rs` |
| `/dashboards/import` | `/dashboards/import` | ✓ | 🔌 `routes/dashboards.rs` (`POST /dashboards`) |
| `/dashboards/add_panel` (dedicated) | `/dashboards/:id/panels/new` | ✓ | 🔌 reuses DashboardEditor |
| `/alerts/history` | `/alerts/history` | ✓ | 🔌 derived from `routes/alerting.rs` (`/alerts/incidents` filtered to resolved/closed) |
| `/alerts/insights` | `/alerts/insights` | ✓ | 🔌 derived from `/alerts/incidents` (client-side aggregate) |
| notify management | `/settings/notify/{connectors,users,policies,templates,defaults,deliveries}` | ✓ | 🔌 `routes/notify/*` |
| personal notify settings | `/account/settings/notify` | ✓ | 🔌 `/users/:id/notify-{endpoints,preferences}` |
| `/alerts/import-semantic-groups` | `/alerts/import-semantic-groups` | ☐ | 🚧 (deferred to a future change) |
| `/alerts/anomaly/{add,edit/:id}` | `/alerts/anomaly/*` | ☐ | 🚧 (deferred to a future change) |
| resource-scoped share | `/s/:token` → `/shared` | ✓ | 🔌 `routes/resource_shares.rs` (受限 Share Principal) |
| `/ingestion/*` (per-vendor real pages) | `/ingest/:category/:source` | ✓ | 🔌 `routes/Ingest/sources.ts` vendor catalog + `/healthz` smoke check |

## Routes intentionally not mirrored

| openobserve | Reason |
| --- | --- |
| `/logout` | molesignal uses `auth.logout()` from the topbar menu — no dedicated page |
| `/cb` | OAuth callback handled inside `/login` |
| `/member_subscription` | molesignal bills outside the app |
| `/about` | replaced by `Topbar` build badge |
| `/incidents` ( top-level) | already covered under `/alerts/incidents` |
