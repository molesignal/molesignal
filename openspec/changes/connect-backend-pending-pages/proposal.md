## Why

Several frontend routes still present backend-pending states even when the backend now exposes usable APIs or when the data can be read through the generic query endpoint. This makes finished backend work look unavailable and blocks users from managing settings, RUM data, and runtime operations from the UI.

## What Changes

- Replace backend-pending empty states with real empty states on pages backed by existing endpoints: license, model pricing, alert templates, regex patterns, AI toolsets, running queries, scheduled pipelines, and scheduled reports.
- Wire RUM list/detail/performance pages to query-backed data without presenting ingest-only backend gaps as missing APIs.
- Wire onboarding test events to the existing ingest APIs so verification actions exercise the backend instead of showing pending copy.
- Add or connect minimal backend/API surfaces for IAM operational gaps where a durable model already exists or can be derived safely, starting with quota and service-account views.
- Update copy and tests so backend-pending is reserved for true API gaps only.

## Capabilities

### New Capabilities
- `web-backend-pending-connectivity`: Frontend routes that previously degraded to backend-pending SHALL use live backend APIs when an endpoint or query-backed data source exists.

### Modified Capabilities
- `web-settings-admin`: Settings pages with implemented endpoints SHALL render data-backed lists and normal empty states instead of backend-pending states.
- `web-rum`: RUM pages SHALL read browser telemetry through query-backed streams and use normal empty states when no telemetry exists.
- `web-iam`: IAM pages SHALL connect quota and service-account views to backend-derived data where possible.
- `query-runtime-control`: Running-query management SHALL expose the live `/query/running` and cancel endpoints in the UI.

## Impact

- Affected frontend code: `web/src/routes/settings/*`, `web/src/routes/rum/*`, `web/src/routes/iam/*`, `web/src/api/*`, i18n namespaces, and relevant tests.
- Affected backend code: only minimal routes needed to expose already-modeled data, if the frontend cannot rely on existing APIs.
- No new frontend framework or design system dependency.
