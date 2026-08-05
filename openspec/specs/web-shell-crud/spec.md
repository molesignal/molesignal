# Web Shell CRUD Capability

## Purpose

Alert / Dashboard / Metrics explorer / Pipeline editor / Functions UDF editor / RUM / Sourcemaps / Scheduled reports / Settings / Ingestion wizard / Short URL / Annotations / Incidents 共 13 个前端 CRUD 页面的统称；每页含 list / detail / create / edit / delete + 键盘热键 + investigation-stack 集成。本 spec 是占位伞 spec，前端实现按用户指示暂缓。

## Requirements

### Requirement: Feature module layout

Every CRUD page SHALL live under `web/src/features/<name>/` with the structure `{ routes.tsx, api.ts, list/index.tsx, detail/index.tsx, form.tsx, keyboard.ts }`. Routes SHALL be lazy-loaded (React `lazy()` + Suspense). Feature modules SHALL NOT import from each other; cross-feature interaction goes through `shell/` shared modules.

#### Scenario: Lazy-loaded feature chunk

- **WHEN** a user navigates to a route owned by a feature not yet visited
- **THEN** the feature's JS chunk is fetched on demand and rendered after Suspense boundary resolves

### Requirement: Keyboard hotkey registration

Every feature module SHALL register its keyboard chords in `keyboard.ts` and bind via the `shell/keyboard/KeyboardController` API. No feature module SHALL attach raw `document.addEventListener('keydown')` listeners.

#### Scenario: Go-to-alerts chord

- **WHEN** the user presses `g a` from anywhere in the app
- **THEN** the router navigates to `/alerts` (the chord is registered by the `alerts` feature's `keyboard.ts`)

### Requirement: Investigation stack compatibility

Every feature's detail view SHALL be pushable onto the `shell/stack/InvestigationStack`. Push SHALL serialize the view state (route + query params + filters) so it can be restored from a pinned frame even after navigation.

#### Scenario: Push detail to stack and return

- **WHEN** the user opens a dashboard detail, presses `⌘P` to push, navigates to alerts, then presses `⌘[`
- **THEN** the previous dashboard detail view is restored with the same filters and time range

### Requirement: Pages covered by this change

The web app SHALL render real CRUD pages for the following capabilities, each driven by its REST endpoints (no front-end mock fallback): alerts (`/api/v1/alerts/rules`, `/api/v1/alerts/incidents`), Notify (`/api/v1/notify/*`), escalation policies (`/api/v1/alerts/escalations`), on-call schedules (`/api/v1/schedules`), dashboards list (`/api/v1/dashboards`), ingestion sources (`/api/v1/ingestion/sources`), and ad-hoc query (`/api/v1/query`). Each client module SHALL be path/method/params-aligned with its backing Rust route.

#### Scenario: Real backend powers the page

- **WHEN** a developer runs `pnpm dev` with a live `localhost:5080` backend and opens `/alerts`
- **THEN** the page calls `GET /api/v1/alerts/rules` and renders the returned items
- **AND** no hard-coded sample alert array remains in `web/src/api/alerts.ts` or its callers

#### Scenario: Endpoint audit catches mismatches

- **WHEN** any `web/src/api/<feature>.ts` declares a path that does not match `crates/api/src/http/routes/<feature>.rs`
- **THEN** the dev console logs an `endpoint-mismatch` warning at startup (debug build)
- **AND** the audit step in `pnpm test:run` fails with the diverging route name and expected path

### Requirement: Permission rendering

Pages SHALL hide / disable mutating actions when the current user lacks the required `Permission`. The disabled state SHALL include a tooltip explaining which permission is needed.

#### Scenario: Viewer cannot create alert

- **WHEN** a user with role `Viewer` is on the alerts list page
- **THEN** the "New alert" button is disabled and the tooltip reads "requires AlertWrite permission"

### Requirement: Functions And Extend Tables Top-Level Routes

The web app SHALL expose `/functions` and `/extend-tables` as top-level routes (not nested under `/pipelines` or `/settings`). They appear in the DATA Sidebar group alongside Streams / Pipelines / Reports.

#### Scenario: Functions reachable directly

- **WHEN** the user pastes the URL `/functions` into the address bar
- **THEN** the page loads the function library without requiring navigation via Pipelines
- **AND** the Sidebar highlights the Functions entry

### Requirement: Pipeline Sub-Route Set

The web app SHALL expose the following pipeline sub-routes (in addition to the existing `/pipelines` health list): `/pipelines/new`, `/pipelines/import`, `/pipelines/:id`, `/pipelines/:id/edit`, `/pipelines/:id/history`, `/pipelines/:id/backfill`. The list page opens `/pipelines/:id`; detail actions link to edit, history, and backfill.

#### Scenario: Action menu surfaces history + backfill

- **WHEN** the user opens the row action menu on the pipelines list
- **THEN** the menu contains "Edit", "History", "Backfill", "Export", "Delete"
- **AND** "Backfill" / "History" link to the corresponding sub-routes
