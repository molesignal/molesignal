# Web Navigation

Authenticated navigation is owned by `web/src/product/ia.ts`. The registry is
the single source of truth for route group, label key, icon, edition
availability, role visibility, owner module, empty-state strategy, and deep
route breadcrumb/back metadata.

## Primary Groups

| Group | Purpose | Current entries |
| --- | --- | --- |
| Home | Activation, operational summary, and next best actions. | Home |
| Observe | Daily signal investigation and monitoring. | Dashboards, Metrics, Logs, Traces, APM, RUM, Profiles, Alerts |
| Data | Collection, shaping, enrichment, and scheduled outputs. | Ingest, Streams, Pipelines, Functions, Enrichment tables, Reports |
| Automate | Scheduled operational workflows. | Pipeline backfill metadata |
| Admin | Governance, access, org, license, and platform settings. | IAM, Settings |

## Registry Rules

- Every authenticated product route must have exactly one `group` and one
  `owner`.
- Primary sidebar entries set `nav: true`; deep routes stay hidden from the
  sidebar and expose `breadcrumbs` plus `backTo` where a parent workflow exists.
- Labels use the `nav` namespace. Deep labels live under `nav.breadcrumbs`.
- Edition and role metadata are descriptive in this foundation slice. Filtering
  and gate rendering are handled by the edition-awareness work.
- Empty-state strategies are product-level intent. Route implementations should
  use them when migrating to shared page templates.

## Deep Route Coverage

The initial breadcrumb set covers dashboard create/import/edit/panel pages,
trace detail and session detail, stream explore, logs inspector,
ingest source drilldowns, pipeline create/import/edit/history/backfill,
APM overview/service/Transaction/dependency/backend-error/deployment pages,
independent RUM application/session/page/error/performance/replay/settings flows, Notify management under system settings
(`/settings/notify/{connectors,users,policies,templates,defaults,deliveries}`), personal
Notify settings (`/account/settings/notify`), and key settings detail routes.

## APM Routes And Compatibility

APM owns the canonical application-centric routes:

- `/apm/overview`
- `/apm/services` and `/apm/services/:service`
- `/apm/transactions`
- `/apm/dependencies`
- `/apm/errors` and `/apm/errors/:fingerprint`
- `/apm/deployments`

`/apm` redirects to `/apm/overview`. Existing `/services*` bookmarks redirect
to the corresponding APM service route. `/apm/versions/compare` redirects to
`/apm/deployments` while retaining its comparison query. Logs, Metrics, Traces,
and Profiles remain independent canonical signal routes.

RUM owns the canonical user-centric `/rum/*` hierarchy:

- `/rum/overview`, `/rum/applications`, `/rum/sessions`, `/rum/pages`
- `/rum/errors`, `/rum/performance/*`, `/rum/session-replay`
- `/rum/settings/{sdk,source-maps,sampling,privacy,session-replay}`

Every legacy `/apm/user-experience/*` suffix maps back to `/rum/*`; path
parameters, query strings, and URL fragments are preserved. Source Map legacy
paths map to `/rum/settings/source-maps*`. The internal RUM API, stream names,
and component ownership remain unchanged.

Backend APM pages require either `streams.query` in organization scope or
`sys.telemetry.read` in system scope. RUM remains
organization-scoped and requires `streams.query`; Source Map writes additionally
require `streams.configure`.

The `_sys` workspace exposes the read-only observability surfaces backed by
self telemetry: Home, Logs, Metrics, Traces, APM, Profiles, and the Streams
catalog. Platform administrators use `sys.telemetry.read` for these routes.
Organization-only collection, stream mutation, dashboards, alerting, pipelines,
IAM, and RUM remain unavailable in system scope.
