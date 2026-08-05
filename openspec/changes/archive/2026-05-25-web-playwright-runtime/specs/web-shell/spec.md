## ADDED Requirements

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
