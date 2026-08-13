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
| `/rum/overview` | ✓ | 🚧 RUM stream queries |
| `/rum/applications` | ✓ | 🚧 `rum_sessions` aggregation |
| `/rum/sessions` | ✓ | 🚧 RUM stream queries |
| `/rum/sessions/view/:id` | ✓ | 🚧 `rum_sessions` + `rum_actions` queries |
| `/rum/pages` | ✓ | 🚧 `rum_actions` aggregation |
| `/rum/errors` | ✓ | 🚧 `rum_errors` queries |
| `/rum/errors/view/:id` | ✓ | 🚧 `rum_errors` queries |
| `/rum/performance/*` | ✓ | 🚧 RUM stream aggregation |
| `/rum/session-replay` | ✓ | 🔌 `GET /rum/replay/:session_id` |
| `/rum/settings/source-maps` | ✓ | 🔌 `GET /debug-artifacts` |
| `/rum/settings/source-maps/upload` | ✓ | 🔌 `POST /debug-artifacts` |

RUM intake uses `POST /rum/{sessions,actions,errors,replay}`. Browser pages query
the `rum_sessions`, `rum_actions`, and `rum_errors` log streams and render an
explicit empty state when no data is available.

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
| `/iam/quota` | ✓ | 🚧 explicit empty state |
| `/iam/invitations` | ✓ | 🚧 explicit empty state |
| `/settings/notify/*` | ✓ | 🔌 `routes/notify/*` |
| `/settings/cipher_keys` | ✓ | 🔌 `routes/cipher_keys.rs` |
| `/settings/regex_patterns` | ✓ | 🔌 `routes/regex_patterns.rs` |
| `/settings/model_pricing` | ✓ | 🔌 `routes/model_prices.rs` |
| `/settings/query_management` | ✓ | 🔌 running-query and cancellation routes |
| `/settings/nodes` | ✓ | 🔌 `routes/clusters.rs` |
| `/settings/domain_management` | ✓ | 🔌 `routes/domains.rs` |
| `/settings/correlation` | ✓ | 🔌 read-only correlation routes |
| `/settings/organization_management` | ✓ | 🔌 organization directory routes |
| `/settings/license` | ✓ | 🔌 `routes/license.rs` |

## Investigation and secondary routes

| MoleSignal path | Status | Backend |
| --- | --- | --- |
| `/logs/inspector` | ✓ | 🔌 search jobs |
| `/traces/:id` | ✓ | 🔌 trace detail |
| `/traces/session/:id` | ✓ | 🔌 trace stream query |
| `/streams/:id` | ✓ | 🔌 Web search routes |
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
