## MODIFIED Requirements

### Requirement: Pages covered by this change

The web app SHALL render real CRUD pages for the following capabilities, each driven by the corresponding `crates/api/src/http/routes/<feature>.rs` REST endpoints (no front-end mock fallback): alerts (`/api/v1/alerts/rules`, `/api/v1/alerts/incidents`), notification channels (`/api/v1/alerts/channels`), escalation policies (`/api/v1/alerts/escalations`), on-call schedules (`/api/v1/schedules`), dashboards list (`/api/v1/dashboards`), ingestion sources (`/api/v1/ingestion/sources`), and ad-hoc query (`/api/v1/query`). Each `web/src/api/<feature>.ts` client module SHALL be path/method/params-aligned with its backing Rust route at `crates/api/src/http/routes/<feature>.rs`.

#### Scenario: Real backend powers the page

- **WHEN** a developer runs `pnpm dev` with a live `localhost:5080` backend and opens `/alerts`
- **THEN** the page calls `GET /api/v1/alerts/rules` and renders the returned items
- **AND** no hard-coded sample alert array remains in `web/src/api/alerts.ts` or its callers

#### Scenario: Endpoint audit catches mismatches

- **WHEN** any `web/src/api/<feature>.ts` declares a path that does not match `crates/api/src/http/routes/<feature>.rs`
- **THEN** the dev console logs an `endpoint-mismatch` warning at startup (debug build)
- **AND** the audit step in `pnpm test:run` fails with the diverging route name and expected path
