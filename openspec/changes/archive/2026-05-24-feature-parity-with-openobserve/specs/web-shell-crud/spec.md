## ADDED Requirements

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

The change SHALL deliver feature modules for: `alerts` (rule CRUD + silence + test), `dashboards` (builder + JSON editor + sharing), `metrics` (PromQL IDE + chart builder), `pipelines` (visual workflow), `functions` (UDF editor), `rum` (sessions / errors / performance), `sourcemaps` (upload + lookup), `scheduled_reports` (subscription CRUD + preview), `settings` (org + tokens + quotas + SSO + cipher + connectors tabs), `ingestion` (SDK wizard), `short_urls` (manager), `annotations` (editor), `incidents` (group list + drill-down). Each page SHALL support list / detail / create / edit / delete via the corresponding REST API.

#### Scenario: Alert page reachable

- **WHEN** the user navigates to `/alerts`
- **THEN** the alerts list view renders with at least 1 row per existing alert rule and supports `n` hotkey to create new rule

### Requirement: Permission rendering

Pages SHALL hide / disable mutating actions when the current user lacks the required `Permission`. The disabled state SHALL include a tooltip explaining which permission is needed.

#### Scenario: Viewer cannot create alert

- **WHEN** a user with role `Viewer` is on the alerts list page
- **THEN** the "New alert" button is disabled and the tooltip reads "requires AlertWrite permission"
