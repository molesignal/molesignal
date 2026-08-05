## ADDED Requirements

### Requirement: Headless Chrome Renderer For PDF / PNG

When configured (`[scheduled_reports.renderer].enabled = true` + `cfg=enterprise`), the system SHALL render dashboards or saved-view reports using a pool of headless Chrome / Chromium instances:

- `ReportRenderer::render(target, format, viewport, session_token) -> Result<Bytes>` for `format ∈ { png, pdf }`
- Reuses Chrome instances across renders (pool size = `concurrent_renders`, default 2)
- Per-render timeout = `render_timeout_secs` (default 30s); on timeout: kill the instance, count as `Failed`
- Constructs an internal URL `http://127.0.0.1:<api_port>/dashboards/{id}/embed?session={token}` (target=dashboard) or `/saved_views/{id}/embed?session={token}` (target=saved_view)

#### Scenario: PNG render returns image bytes

- **WHEN** ScheduledReport with `format=png`, `dashboard_id=d1` fires
- **AND** the embed page renders successfully within timeout
- **THEN** `ReportRenderer::render` returns `Bytes` whose first 8 bytes are PNG magic (`\x89PNG\r\n\x1a\n`)

#### Scenario: PDF render returns PDF bytes

- **WHEN** ScheduledReport with `format=pdf` fires
- **THEN** the returned bytes start with `%PDF-` and Chrome's `printToPDF` was invoked with the configured viewport

#### Scenario: Timeout marks delivery as failed

- **WHEN** Chrome takes > `render_timeout_secs` to render
- **THEN** the renderer kills that Chrome instance, returns `Err`, and `report_deliveries` row gets `status=failed`, `error="render timeout"` for each recipient

#### Scenario: No renderer configured falls back to SVG with warn

- **WHEN** ScheduledReport has `format=png` but `[scheduled_reports.renderer].enabled=false`
- **THEN** the runner falls back to existing SVG placeholder (so delivery still happens)，并 emit a single warn log per process lifetime hinting to enable renderer

### Requirement: Renderer Resource Bounds

The renderer SHALL enforce upper bounds to avoid OOM in shared deployments:
- Wall-clock `render_timeout_secs` ≤ 60s
- Memory: Chrome launched with `--js-flags=--max-old-space-size=512`
- Pool size hard-capped at 4 instances regardless of config

#### Scenario: Pool exhausted queues caller

- **WHEN** `concurrent_renders=2` and 3rd render arrives while both instances are busy
- **THEN** the 3rd call awaits a semaphore permit (no panic, no new Chrome instance spawned)

#### Scenario: Misconfigured timeout clamped

- **WHEN** config has `render_timeout_secs=600`
- **THEN** effective timeout is 60s with a warn log "render_timeout clamped to 60s"
