## ADDED Requirements

### Requirement: Scheduled report CRUD

The system SHALL expose `/api/v1/scheduled_reports` with `{ id, org_id, name, dashboard_id?, saved_view_id?, cron, recipients: [{ kind: email|webhook|s3, target }], format: png|pdf|csv|json, time_range: relative|absolute, enabled }`. Either `dashboard_id` or `saved_view_id` SHALL be set, not both.

#### Scenario: Create report with cron

- **WHEN** a user creates a report `{ "dashboard_id": "d1", "cron": "0 9 * * MON", "recipients": [{"kind":"email","target":"team@x"}], "format":"pdf" }`
- **THEN** subsequent Monday 9:00 the report engine triggers rendering + email delivery

### Requirement: Render + deliver pipeline

The render engine SHALL render dashboards to SVG / PDF / PNG (SVG MVP; PDF/PNG via headless Chrome behind an optional feature). Each delivery SHALL be persisted in `report_deliveries` with `{ status: pending|sent|failed, attempted_at, error?, recipient_target }`.

#### Scenario: Delivery failure recorded

- **WHEN** an SMTP delivery returns a permanent failure
- **THEN** `report_deliveries` row is updated to `status: failed` with the error body; retry SHALL occur on next cron tick (capped at 3 attempts)
