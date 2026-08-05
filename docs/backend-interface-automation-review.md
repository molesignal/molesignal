# Backend Interface Automation Review

Date: 2026-05-26

Scope:
- Start the current backend against a Docker Postgres dependency.
- Exercise newly implemented backend interfaces through the browser/frontend origin.
- Create, update, query, and delete QA data where supported.
- Record defects, fix them, and rerun the failing checks.

## Findings

### Fixed

1. Duplicate SQL migration version blocked fresh database startup.
   - Symptom: backend startup failed during `sqlx` migrations with `duplicate key value violates unique constraint "_sqlx_migrations_pkey"`.
   - Cause: `20260701000002_invitations.sql` reused the same version as `20260701000002_pipeline_runs.sql`.
   - Fix: rename the invitations migration to the next unique version.
   - Verification: backend started successfully against fresh Docker Postgres database `molesignal_qa_20260526b`.

2. Frontend auth role casing did not match backend API output.
   - Symptom: `POST /auth/login` returns roles as `owner`, `admin`, `editor`, `viewer`, while frontend product gates compare against `Owner`, `Admin`, `Editor`, `Viewer`.
   - Impact: real login sessions could be treated as lower-privilege in role-gated product surfaces.
   - Fix: normalize roles when sessions are written to the auth store, and cover login/org-switch role normalization with unit tests.

3. Frontend schedule type did not match backend JSON shape.
   - Symptom: frontend `RotationKind` type modeled unit variants as `{ kind: "daily" }`, but Rust serde accepts and emits `"daily"` / `"weekly"` for unit variants.
   - Fix: update the shared frontend type to the actual backend contract.

### Reviewed

- Enrichment table list initially appeared to fail in automation, but this was a test assertion issue. The endpoint correctly returns summary objects shaped as `{ table_name, row_count, updated_at }`; row upsert, list, and delete all pass after correcting the assertion.

## Verification Log

- Backend health: `GET /api/v1/healthz` returned `{"status":"ok"}`.
- API automation: 40 checks passed, 0 failed. Covered auth login; roles; alert channels, escalations, rules; schedules and overrides; enrichment table rows; invitations; functions create/update/run/delete; report templates; correlation providers; audit recent.
- Browser automation: Playwright headless login and page smoke passed on `/home`, `/alerts`, `/functions`, `/enrichment-tables`, `/iam/invitations`, `/settings/correlation`, `/reports`, and `/iam/roles`; no visible `backend pending`, `awaiting backend`, `not implemented`, or `等待后端` text was found.
- Frontend validation: `pnpm -C web exec tsc --noEmit`, targeted `vitest`, and targeted `eslint` passed.
