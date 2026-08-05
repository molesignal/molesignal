## 1. Settings And Runtime Pages

- [x] 1.1 Replace backend-pending empty states with normal empty states for implemented settings endpoints: license, alert templates, regex patterns, model pricing, AI toolsets, and query management.
- [x] 1.2 Update settings-admin i18n copy so empty states describe no configured rows instead of missing endpoints.
- [x] 1.3 Remove stale comments from running-query client/page that claim `/query/running` is not implemented.

## 2. RUM Pages

- [x] 2.1 Replace backend-pending empty states on query-backed RUM sessions, errors, detail, and performance pages with normal no-data states.
- [x] 2.2 Confirm `web/src/api/rum.ts` maps query results defensively for empty rows and missing optional columns.

## 3. IAM Derived Views

- [x] 3.1 Connect `/iam/service-accounts` to existing users data as a derived service-account list.
- [x] 3.2 Connect `/iam/quota` to the license snapshot for plan limits and render unavailable usage values without backend-pending copy.

## 4. Other Implemented Backend Surfaces

- [x] 4.1 Connect ingest test events to the existing ingest APIs instead of showing backend-pending copy.
- [x] 4.2 Connect scheduled pipeline recent runs to `/scheduled_pipelines/{id}/runs`.
- [x] 4.3 Connect scheduled report create/update to `/scheduled_reports`.

## 5. QA

- [x] 5.1 Run frontend typecheck and targeted lint for touched files.
- [x] 5.2 Run OpenSpec validation for `connect-backend-pending-pages`.
