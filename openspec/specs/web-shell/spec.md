# Web Application Shell

## Purpose

Minimal "quiet canvas" chrome (32px status strip + 52px hover-revealed icon rail), 9-color × 2-theme × 2-density token system, route map, JWT auth bootstrap, and the 5 web-aggregation backend contracts the SRE investigation UI relies on.
## Requirements
### Requirement: Minimal Application Chrome

The web app SHALL render a single application shell composed of: a 32px top status strip, a 52px left icon rail that is collapsed by default and hover-revealed within an 8px hot zone on the left edge, and a main content area that fills the remaining viewport with no breadcrumbs, page headers, or footers.

#### Scenario: Status strip shows fixed sections
- **WHEN** the app boots with an authenticated user
- **THEN** the status strip from left to right shows: org name, alive cluster count (`<n> nodes`), current global time window summary (`-1h` or `09:00 – 10:00 UTC`), the literal text `⌘K` as a hint, and the current user avatar+role; nothing else

#### Scenario: Left rail collapses by default
- **WHEN** the app first renders or a route changes
- **THEN** the 52px icon rail is not visible; pointer entering the leftmost 8px column expands it within 80ms; pointer leaving for >300ms collapses it

#### Scenario: No persistent navigation chrome
- **WHEN** any route is active
- **THEN** the DOM SHALL NOT render breadcrumb, page title bar, or page footer elements; the main area's top edge sits directly under the status strip

### Requirement: Theme And Density Tokens

The shell SHALL expose CSS custom properties for exactly nine semantic colors (`bg`, `surface`, `primary`, `accent`, `red`, `green`, `yellow`, `blue`, `purple`) per theme (`dark`, `light`), and two density modes (`compact`, `comfortable`) controlling row height and padding tokens, with `compact` as the default for authenticated routes. **All foreground/background pairs that may be combined at runtime MUST meet WCAG 2.1 AA contrast (4.5:1 for body text, 3:1 for large/UI elements), as verified by `pnpm -C web a11y:contrast`.**

#### Scenario: Theme tokens are limited

- **WHEN** a developer inspects `:root`
- **THEN** the only chrome color CSS variables defined are the nine semantic names (per theme) plus their `*-muted` and `*-bg` variants; no additional palette colors are exported

#### Scenario: Density default

- **WHEN** an authenticated user opens any route for the first time and has not set a density preference
- **THEN** the body element carries `data-density="compact"` and the row height token resolves to `28px`; switching to comfortable yields `36px`

#### Scenario: All active token pairs meet AA contrast

- **WHEN** `pnpm -C web a11y:contrast` runs in CI after token edits
- **THEN** it exits 0 and prints zero `FAIL` lines for both dark and light themes

### Requirement: Route Map

The shell SHALL register exactly the following routes: `/login`, `/investigate` (default authenticated landing), `/services`, `/services/:service`, `/dashboards`, `/dashboards/:id`, `/alerts/rules`, `/alerts/incidents`, `/alerts/incidents/:id`, `/saved-views`, `/settings/*`. All deep links SHALL re-hydrate the global time window, pinned anchor, and investigation stack from URL parameters before rendering data.

#### Scenario: Default authenticated landing
- **WHEN** a user logs in
- **THEN** the router redirects to `/investigate` with the global time window defaulted to the last 1 hour

#### Scenario: URL hydration before render
- **WHEN** a user opens a URL containing `?time=...&anchor=...&stack=...`
- **THEN** the app SHALL parse those parameters and update the time and stack stores synchronously before mounting the main view (no flash of empty state)

### Requirement: Auth Bootstrap

On app boot the shell SHALL read the JWT or `ms_` API token from secure storage, exchange it for `AuthContext { user_id, org_id, role }`, and either render the authenticated shell or redirect to `/login` preserving the original URL in `?next=`. **The Login form SHALL collect email + password only — no `workspace` field — and on success route the user to the `?next=` URL (defaulting to `/home`); the JWT's `org_id` claim is the authoritative current-org source.**

#### Scenario: Unauthenticated deep link

- **WHEN** an unauthenticated user navigates to `/investigate?time=-2h..now`
- **THEN** the app redirects to `/login?next=%2Finvestigate%3Ftime%3D-2h..now`; after successful login the user lands back on the original URL

#### Scenario: Token expiry mid-session

- **WHEN** any API call returns `401 token expired`
- **THEN** the shell clears stored tokens and redirects to `/login?next=<current>`; in-flight queries are cancelled

#### Scenario: Login form has no workspace field

- **WHEN** an unauthenticated user opens `/login`
- **THEN** the form renders exactly two text inputs (`email`, `password`) plus a primary "Sign in" button and an "Continue offline (dev)" link
- **AND** no `workspace` / `org` selector is shown on the form

#### Scenario: 401 also fires on org-switch failure

- **WHEN** `POST /api/v1/orgs/{id}/select` returns 401
- **THEN** the http interceptor logs out the user and navigates to `/login?next=<current pathname + search>`

### Requirement: Web-Side Backend Contracts

The web app SHALL consume four web-aggregation endpoints provided by the backend: `GET /api/v1/web/search?q&types&limit`, `GET /api/v1/web/topology?from&to`, `GET /api/v1/web/trace/:trace_id`, and `GET /api/v1/web/correlation/:from_kind/:to_kind?ctx=<base64>`; plus a streaming variant `GET /api/v1/query/stream` (NDJSON over chunked or SSE). Each endpoint SHALL be scoped to the caller's org and return `200` on success or `4xx`/`5xx` with `{ "error": "<msg>" }`.

#### Scenario: Search aggregation respects types filter
- **WHEN** the palette issues `GET /api/v1/web/search?q=lat&types=streams,saved_views&limit=10`
- **THEN** the response is `{ "items": [{ "kind": "stream"|"saved_view", "id", "label", "subtitle"?, "icon"? }] }` containing only items of the requested types, total length <= 10

#### Scenario: Streaming query frames
- **WHEN** the client requests `GET /api/v1/query/stream?language=sql&statement=...` with `Accept: application/x-ndjson`
- **THEN** the server returns `200` with `Transfer-Encoding: chunked`, body is newline-delimited JSON where each line is a `RecordBatchFrame { rows: [...] }`, and a final `{"meta": {...}}` line closes the stream

#### Scenario: Cross-org request rejected
- **WHEN** a request to any `/api/v1/web/*` endpoint references an id owned by another org
- **THEN** the response is `404 Not Found` (no enumeration) and no payload is leaked

### Requirement: Strict TypeScript Build Gate

The `web/` workspace SHALL build cleanly with the project's strict TypeScript configuration enabled: `pnpm -C web typecheck` MUST exit 0 with zero `tsc` errors; `pnpm -C web lint --max-warnings 0` MUST pass; `pnpm -C web build` MUST emit dist. These three commands together form the build gate that CI's `web.yml` workflow enforces.

#### Scenario: `pnpm -C web typecheck` passes with zero errors

- **WHEN** a developer or CI runs `pnpm -C web typecheck`
- **THEN** the command exits with code 0
- **AND** no `TS####` diagnostics are printed to stdout/stderr

#### Scenario: ESLint blocks unused React import regression

- **WHEN** a contributor adds `import * as React from 'react'` to a file that does not reference `React.*`
- **AND** they run `pnpm -C web lint`
- **THEN** lint reports an `@typescript-eslint/no-unused-vars` error and exits non-zero

#### Scenario: exactOptionalPropertyTypes is enforced

- **WHEN** any new code passes `field: SomeType | undefined` to a third-party prop typed as `field?: SomeType`
- **THEN** `pnpm -C web typecheck` MUST fail with TS2375, prompting the author to use the conditional-spread pattern `...(value !== undefined && { field: value })`

### Requirement: Playwright Runtime Gate

The `web/` workspace SHALL ship a deterministic Playwright e2e suite gated entirely on in-test mock backends, with no dependency on a live `molesignal-bootstrap` HTTP server. `pnpm -C web playwright test` MUST exit 0 in a fresh checkout where docker / postgres / dev backend are not available.

#### Scenario: e2e suite passes without dev backend

- **WHEN** a contributor runs `pnpm -C web playwright test` on a host where no `/api/v1/*` endpoints are reachable
- **THEN** all 4 behavior specs (01-04) plus visual + smoke pass under 60s
- **AND** zero requests escape the `page.route('**\/api/v1/**')` interceptor (verified via Playwright network log)

#### Scenario: clock and theme are frozen across all e2e

- **WHEN** any e2e spec mounts a page
- **THEN** `page.clock.install({ time: '2026-05-23T10:00:00.000Z' })` is in effect
- **AND** body `data-theme` + `data-density` are seeded via `addInitScript` before React boots (no flash of wrong theme)

### Requirement: Performance Suite Budgets

The `web/` workspace SHALL define a `@perf` Playwright suite that mounts the 4 visualization demo routes with synthetic data sized to spec (1M log rows / 100k spans / 10M ts points / 200 topology nodes) and asserts wall-clock render budgets. CI MAY run this suite on a separate cadence (not every PR), but it MUST be runnable locally via `pnpm -C web playwright test --grep @perf`.

#### Scenario: 100k span trace renders within budget

- **WHEN** the perf spec navigates to `/_demo/trace?spans=100000`
- **AND** waits for the canvas to first paint
- **THEN** the elapsed wall-clock from `page.goto` to `waitForSelector('canvas')` is below the CI-runner budget (1.5s on GitHub Actions Linux x64)

#### Scenario: 1M log scroll keeps FPS ≥ 55

- **WHEN** the perf spec scrolls `/_demo/log?rows=1000000` for 5 seconds via `page.mouse.wheel`
- **THEN** the Chrome DevTools Protocol `Tracing.dataCollected` events show average FPS ≥ 55 across the scroll window

### Requirement: Trace Artefact Upload On Failure

The `web.yml` CI workflow SHALL upload Playwright trace artefacts (`web/playwright-report/`) when the playwright job fails, retaining them for 14 days. Successful runs SHALL NOT upload to save CI storage.

#### Scenario: Failing PR uploads trace zip

- **WHEN** a Playwright test fails in CI
- **THEN** `actions/upload-artifact@v4` runs with `if: failure()` and uploads the `playwright-report/` directory
- **AND** the artefact name is `playwright-trace` for easy retrieval

### Requirement: WCAG 2.1 AA Contrast Gate

The `web/` workspace SHALL ship a `pnpm -C web a11y:contrast` script that parses `web/src/shell/tokens.css`, derives all foreground/background color pairs across both themes, computes WCAG 2.1 contrast ratios, and exits non-zero when any active pair falls below 4.5:1 for body text or 3:1 for large/UI elements. The script's output MUST include the failing pair, its actual ratio, and the WCAG target.

#### Scenario: All token pairs meet AA contrast

- **WHEN** `pnpm -C web a11y:contrast` runs in CI
- **AND** every active fg/bg pair across dark + light themes meets the minimum ratio
- **THEN** the script exits 0 and prints a green summary table

#### Scenario: One pair fails contrast check

- **WHEN** a token change makes `--yellow on --surface` fall to 3.8:1 in dark theme
- **THEN** the script exits non-zero with a line like
  `FAIL dark.yellow ON dark.surface: 3.80:1 < 4.50:1 (WCAG AA body)`

### Requirement: Axe-Core Critical Violations Gate

The Playwright e2e suite SHALL include an `a11y-routes.spec.ts` that navigates each of the 11 authenticated routes and runs `@axe-core/playwright::AxeBuilder().analyze()`. The test MUST assert that `violations.filter(v => v.impact === 'critical').length === 0`. Moderate / minor violations are reported but do NOT fail the build.

#### Scenario: All routes are critical-violation-free

- **WHEN** `pnpm -C web playwright test playwright/tests/a11y-routes.spec.ts` runs
- **THEN** all 11 routes report `critical = 0`
- **AND** the test prints a per-route count of moderate / minor for visibility

#### Scenario: Critical violation introduced

- **WHEN** a developer ships a `<button>` with no `aria-label` and no visible text
- **THEN** axe reports a critical violation and the test fails with the affected selector

### Requirement: Focus Ring Visual Baseline

The Playwright suite SHALL include `a11y-focus-ring.spec.ts` that focuses a representative element on each of the 4 viz routes (timeseries / trace / log / topology) across `(dark|light) × (compact|comfortable)` = 4 combos, snapshotting the focused element. 16 PNG baselines (4 viz × 4 combos) SHALL be committed to `a11y-focus-ring.spec.ts-snapshots/`.

#### Scenario: Focus ring snapshot matches baseline

- **WHEN** the spec focuses the topology root node in dark/compact theme
- **THEN** the captured PNG matches `topology-focus-dark-compact.png` within `maxDiffPixelRatio = 0.005`

### Requirement: Keyboard Map Coverage

For every `Binding` exported from `web/src/keyboard/bindings.ts::GLOBAL_KEYMAP`, the Playwright suite SHALL include at least one assertion that the binding fires its handler when its key is pressed in its scope. This is implemented via `a11y-keyboard-map.spec.ts` which iterates `GLOBAL_KEYMAP` at test-collection time.

#### Scenario: New binding is auto-covered

- **WHEN** a developer adds `{ keys: 'g r', description: 'go reports', ... }` to `GLOBAL_KEYMAP`
- **AND** runs `pnpm -C web playwright test a11y-keyboard-map.spec.ts`
- **THEN** a new test case `keyboard binding: g r` runs automatically (no manual spec update)
- **AND** the test fails until the binding is wired to a handler that produces an observable DOM effect

### Requirement: StatusStrip Spacing Standard

The top status strip SHALL use a 4px `•` dot as the section separator between org / cluster / window / `⌘K` hint / avatar, with 16px gap between sections (replacing the previous `|` + 12px). The anchor (`📌 hh:mm:ss`) element SHALL reserve `min-width: 12ch` so its appearance does not shift neighbor sections when the time changes.

#### Scenario: Status strip layout is byte-stable on time tick

- **WHEN** the visual baseline `login-*.png` is regenerated at `2026-05-23T10:00:00Z` vs the same fixture at `10:00:30Z`
- **THEN** non-anchor pixels are identical (no neighbor reflow)

### Requirement: StatusStrip Settings Dropdown

The StatusStrip SHALL include a Settings (gear) trigger to the left of the avatar; the dropdown SHALL contain four sections — Theme / Palette / Density / Language — surfacing every option per section as a checkable item. Existing scattered toggles (sun-moon icon, palette `Toggle theme` / `Toggle density` static actions) SHALL keep working but are no longer the primary affordance.

#### Scenario: Gear opens the unified settings dropdown

- **WHEN** the user clicks the gear icon in the StatusStrip
- **THEN** a dropdown opens listing Theme (dark, light), Palette (default, high-contrast, warm), Density (compact, comfortable), Language (en, zh-CN)
- **AND** each section's active option carries a leading checkmark

#### Scenario: Legacy theme toggle still works

- **WHEN** the user clicks the sun/moon icon (legacy single-purpose toggle)
- **THEN** `useThemeStore.theme` flips between dark and light
- **AND** the gear dropdown's Theme section reflects the new value

### Requirement: No Hardcoded Light-Mode Black

JSX or inline styles in `web/src/**/*.tsx` SHALL NOT use any of `text-black`, `bg-black`, `border-black`, `color: #000`, or `color: black`. All color references go through tokens (`text-foreground`, `text-tx-*`, `bg-bg`, `bg-surface`, `border-border`, etc.). A lint or grep gate enforces this on PR.

#### Scenario: Grep gate catches a regression

- **WHEN** a contributor adds `className="text-black"` to a new component
- **AND** runs `pnpm -C web lint`
- **THEN** an ESLint rule (or scripted grep step) reports the offending line and exits non-zero

### Requirement: Sidebar Misc-Pages Entries

The Sidebar SHALL expose a top-level entry for `Service graph` under OBSERVE, linking to its route under `web-misc-pages`. The Sidebar SHALL NOT add entries for trace detail / stream explore / dashboard import / new-panel / short-url — those are reachable from in-page navigation, not Sidebar.

#### Scenario: Sidebar lists new top-level entries

- **WHEN** the Sidebar is open
- **THEN** a `Service graph` entry appears in the OBSERVE group
- **AND** no Sidebar entries are added for trace / stream / dashboard subroutes

### Requirement: Alerts Sub-Nav Adds History And Insights

The `/alerts` shell SHALL render an in-page sub-nav with tabs for `Rules` (existing), `History` (`/alerts/history`), and `Insights` (`/alerts/insights`). The active tab matches the current path.

#### Scenario: Sub-nav highlights active tab

- **WHEN** the user opens `/alerts/history`
- **THEN** the alerts sub-nav highlights `History`

### Requirement: Route Table Adds Misc Routes

The router SHALL register the routes introduced by `web-misc-pages` (`/logs/inspector`, `/traces/:id`, `/traces/session/:id`, `/streams/:id`, `/service-graph`, `/dashboards/import`, `/dashboards/:id/panels/new`, `/alerts/history`, `/alerts/insights`, `/short/:code`, `/ingest/:category/:source` placeholder replacement). The `a11y-routes.spec.ts` array SHALL include each new route so axe `critical=0` is enforced. PromQL Builder is part of `/metrics` and SHALL NOT have a separate route.

#### Scenario: New routes are reachable

- **WHEN** the user types any listed route in the address bar
- **THEN** the router mounts the corresponding page from `web-misc-pages`
- **AND** Playwright's a11y-routes spec covers the route

### Requirement: Sidebar Extended Nav

The Sidebar SHALL surface three new top-level entries beyond the existing 11: **RUM** (under OBSERVE), **Functions** (under DATA), **IAM** (under ADMIN). Each new entry uses a distinct Lucide icon and links to its module's default landing route (`/rum/sessions`, `/functions`, `/iam/users` respectively).

#### Scenario: New entries appear in collapsed and expanded states

- **WHEN** the user opens the app
- **THEN** the Sidebar shows 14 entries grouped into OVERVIEW (1), INGEST (1), OBSERVE (6), DATA (4), ADMIN (2)
- **AND** in collapsed state every entry's icon is keyboard-reachable via `Tab`
- **AND** the active route's left rail rail tick is rendered

### Requirement: Sitemap Coverage Audit

The repo SHALL include `docs/web/sitemap-diff.md` enumerating every openobserve route, our current molesignal equivalent, and the gap status (P0 / P1 / P2 / done). This file is updated when this change applies and when each follow-up (`web-feature-parity-settings`, `web-feature-parity-misc`) lands.

#### Scenario: Sitemap diff includes every route

- **WHEN** a contributor adds a new top-level route
- **THEN** `docs/web/sitemap-diff.md` lists it under the right section
- **AND** the CI lints the markdown for "TODO" entries that have outlived a release
