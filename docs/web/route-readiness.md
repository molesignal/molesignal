# MoleSignal Web route readiness

This document records MoleSignal's canonical Web routes and their backend
readiness. The route declarations in `web/src/routes/index.tsx` remain the source
of truth; this inventory is a maintenance aid for UI navigation, accessibility
coverage, and API integration work.

## Status legend

| Marker | Meaning |
| --- | --- |
| ✓ | Route and page are implemented |
| 🔌 | Dedicated backend endpoint is available |
| 🚧 | Page currently uses stream queries, a read-only view, or an explicit empty state |
| — | No dedicated backend endpoint is required |

## Core navigation

| MoleSignal path | Status | Backend |
| --- | --- | --- |
| `/home` | ✓ | — |
| `/logs` | ✓ | 🔌 `routes/query.rs` |
| `/metrics` | ✓ | 🔌 `routes/metrics.rs` + `routes/query.rs` |
| `/traces` | ✓ | 🔌 `routes/traces.rs` |
| `/profiles` | ✓ | 🔌 `routes/profiles.rs` |
| `/streams` | ✓ | 🔌 `routes/web/streams` |
| `/dashboards` | ✓ | 🔌 `routes/dashboards.rs` |
| `/dashboards/:id` | ✓ | 🔌 `routes/dashboards.rs` |
| `/alerts/incidents` | ✓ | 🔌 `routes/alerting.rs` |
| `/alerts/rules` | ✓ | 🔌 `routes/alerting.rs` |
| `/reports` | ✓ | 🔌 `routes/scheduled_reports.rs` |
| `/pipelines` | ✓ | 🔌 `routes/scheduled_pipelines.rs` |
| `/settings/general` | ✓ | mixed |
| `/intake/:category/:source` | ✓ | — (integration guidance) |

## APM

| MoleSignal path | Status | Backend |
| --- | --- | --- |
| `/apm/overview` | ✓ | 🔌 `GET /api/v1/apm/overview` |
| `/apm/services` | ✓ | 🔌 `GET /api/v1/apm/services` |
| `/apm/services/:service` | ✓ | 🔌 `GET /api/v1/apm/services/:service` |
| `/apm/transactions` | ✓ | 🔌 `GET /api/v1/apm/transactions` |
| `/apm/dependencies` | ✓ | 🔌 `GET /api/v1/apm/dependencies` |
| `/apm/errors` | ✓ | 🔌 `GET /api/v1/apm/errors` |
| `/apm/errors/:fingerprint` | ✓ | 🔌 `GET /api/v1/apm/errors/:fingerprint` |
| `/apm/deployments` | ✓ | 🔌 `GET /api/v1/apm/versions/compare` |

## RUM

| MoleSignal path | Status | Backend |
| --- | --- | --- |
| `/rum/overview` | ✓ | 🔌 `GET /api/v1/rum/aggregate`, `/rum/facets`, `/rum/applications` |
| `/rum/errors` | ✓ | 🔌 `GET /api/v1/rum/issues`, `/rum/issues/{id}`, `PATCH /rum/issues/{id}`, `/rum/aggregate`, `/rum/facets` |
| `/rum/performance` | ✓ | 🔌 `GET /api/v1/rum/aggregate`, `/rum/search`, `/rum/facets`, `/rum/applications` |
| `/rum/performance/resources` | ✓ | 🔌 `GET /api/v1/rum/aggregate`, `/rum/search`, `/rum/entity`, `/rum/facets`, `/rum/applications` |
| `/rum/performance/long-tasks` | ✓ | 🔌 `GET /api/v1/rum/aggregate`, `/rum/search`, `/rum/entity`, `/rum/facets`, `/rum/applications` |
| `/rum/frustration` | ✓ | 🔌 `GET /api/v1/rum/aggregate`, `/rum/search`, `/rum/facets`, `/rum/applications` |
| `/rum/explorer` | ✓ | 🔌 `GET /api/v1/rum/search`, `/rum/facets`, `/rum/histogram`, `/rum/distribution`, `/rum/schema/{type}`, `/me/ui-preference/rum.saved_views/explorer` |
| `/rum/analytics/heatmaps` | ✓ | 🔌 `GET /api/v1/rum/heatmaps/clicks`, `/rum/heatmaps/scroll`, `/rum/heatmaps/snapshot`, `/rum/facets` |
| `/rum/feature-flags` | ✓ | 🔌 `GET /api/v1/rum/keys`, `/rum/aggregate`, `/rum/applications` |
| `/rum/applications` | ✓ | 🔌 `/api/v1/rum/applications`, `GET /api/v1/auth/tokens/rum` |
| `/rum/sessions/view/:id` | ✓ | 🔌 `GET /api/v1/rum/sessions/{id}/timeline`, `/rum/replay/{session_id}`, `/rum/sessions/{id}/related-traces`, `/rum/sessions/{id}/comments`, `/rum/sessions/{id}/summary` |
| `/rum/settings/{sdk,sampling,privacy,session-replay}` | ✓ | — |
| `/rum/settings/source-maps` | ✓ | 🔌 `GET /debug-artifacts` |
| `/rum/settings/source-maps/upload` | ✓ | 🔌 `POST /debug-artifacts` |

RUM intake uses `POST /rum/events` and `POST /rum/replay`. The explorer reads the eight `rum_*` log streams (sessions,
views, actions, errors, resources, long tasks, vitals, and logs) through the RUM endpoints, and shows a first-run guide while the
organization has no application.

## Data workflows

| MoleSignal path | Status | Backend |
| --- | --- | --- |
| `/functions` | ✓ | 🔌 `routes/functions.rs` |
| `/functions/:id` | ✓ | 🔌 `routes/functions.rs` |
| `/extend-tables` | ✓ | 🔌 extend-table routes |
| `/extend-tables/:table` | ✓ | 🔌 extend-table routes |
| `/pipelines/new` | ✓ | 🔌 `POST /scheduled_pipelines` |
| `/pipelines/:id` | ✓ | 🔌 pipeline detail + runs |
| `/pipelines/:id/edit` | ✓ | 🔌 pipeline read/write endpoints |
| `/pipelines/import` | ✓ | 🔌 `POST /scheduled_pipelines` |
| `/pipelines/:id/history` | ✓ | 🔌 pipeline runs |
| `/pipelines/:id/backfill` | ✓ | 🔌 pipeline backfill |

## IAM and settings

| MoleSignal path | Status | Backend |
| --- | --- | --- |
| `/iam/users` | ✓ | 🔌 user routes |
| `/iam/service-accounts` | ✓ | 🔌 service-account routes |
| `/iam/organizations` | ✓ | 🔌 organization routes |
| `/iam/groups` | ✓ | 🔌 role bindings and cross-org grants |
| `/iam/roles` | ✓ | 🚧 read-only permission matrix |
| `/iam/invitations` | ✓ | 🚧 explicit empty state |
| `/settings/notify/*` | ✓ | 🔌 `routes/notify/*` |
| `/settings/cipher_keys` | ✓ | 🔌 `routes/cipher_keys.rs` |
| `/settings/regex_patterns` | ✓ | 🔌 `routes/regex_patterns.rs` |
| `/settings/query_management` | ✓ | 🔌 running-query and cancellation routes |
| `/settings/nodes` | ✓ | 🔌 `routes/clusters.rs` |
| `/settings/domain_management` | ✓ | 🔌 `routes/domains.rs` |
| `/settings/organization_management` | ✓ | 🔌 organization directory routes |

## Investigation and secondary routes

| MoleSignal path | Status | Backend |
| --- | --- | --- |
| `/logs/inspector` | ✓ | 🔌 search jobs |
| `/logs/inspections` | ✓ | 🔌 `routes/log_inspections` (scheduled log inspections: list, limits, save, enable, run now, delete) |
| `/logs/inspections/:id` | ✓ | 🔌 `routes/log_inspections` (one inspection: its runs and the report of each) |
| `/traces/:id` | ✓ | 🔌 trace detail |
| `/traces/session/:id` | ✓ | 🔌 trace stream query |
| `/streams/:id` | ✓ | 🔌 Web search routes |
| `/streams/jobs` | ✓ | 🔌 `routes/search_jobs.rs` (search jobs: list, counts, cancel, retry, delete, results) |
| `/service-graph` | ✓ | 🔌 topology routes |
| `/dashboards/import` | ✓ | 🔌 dashboard creation |
| `/dashboards/:id/panels/new` | ✓ | 🔌 dashboard editor |
| `/alerts/history` | ✓ | 🔌 resolved incident queries |
| `/alerts/insights` | ✓ | 🔌 incident aggregation |
| `/account/settings/notify` | ✓ | 🔌 user notification endpoints |
| `/s/:token` | ✓ | 🔌 resource shares |

## Route behavior notes

- `/` redirects to `/home`.
- `/settings` redirects to `/settings/general`.
- `/alerts` redirects to `/alerts/incidents`.
- Authentication logout is initiated from the top bar and does not require a
  dedicated route.
- OAuth callbacks are handled by the login flow.
- Build information is displayed in the top bar.
- Incident management is rooted at `/alerts/incidents`.
