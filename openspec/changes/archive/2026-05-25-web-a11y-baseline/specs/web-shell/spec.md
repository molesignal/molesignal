## ADDED Requirements

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
