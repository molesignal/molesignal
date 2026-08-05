# Scheduled Reports Capability

## Purpose

Dashboard / saved view 按 cron 渲染并周期投递（email / webhook / S3）；订阅 CRUD、渲染预览、投递历史保留。
## Requirements
### Requirement: Scheduled report CRUD

The system SHALL expose `/api/v1/scheduled_reports` with `{ id, org_id, name, dashboard_id?, saved_view_id?, cron, recipients: [{ kind: email|webhook|s3, target }], format: png|pdf|csv|json, time_range: relative|absolute, enabled }`. Either `dashboard_id` or `saved_view_id` SHALL be set, not both.

#### Scenario: Create report with cron

- **WHEN** a user creates a report `{ "dashboard_id": "d1", "cron": "0 9 * * MON", "recipients": [{"kind":"email","target":"team@x"}], "format":"pdf" }`
- **THEN** subsequent Monday 9:00 the report engine triggers rendering + email delivery

### Requirement: Render + deliver pipeline

The render engine SHALL render dashboards to SVG / PDF / PNG (PDF/PNG via headless Chrome). Each delivery SHALL be persisted in `report_deliveries` with `{ status: pending|sent|failed, attempted_at, error?, recipient_target }`.

#### Scenario: Delivery failure recorded

- **WHEN** an SMTP delivery returns a permanent failure
- **THEN** `report_deliveries` row is updated to `status: failed` with the error body; retry SHALL occur on next cron tick (capped at 3 attempts)

### Requirement: Headless Chrome Renderer For PDF / PNG

The system SHALL render dashboards or saved-view reports using a configured pool of headless Chrome / Chromium instances:

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

#### Scenario: Chrome runtime is unavailable

- **WHEN** ScheduledReport has `format=png` but Chrome cannot be started
- **THEN** the runner records an explicit failed delivery and does not disguise SVG or JSON bytes as PNG

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
