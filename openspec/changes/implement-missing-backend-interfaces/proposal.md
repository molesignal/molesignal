## Why

Several UI surfaces still show backend-pending copy because the API either returns a deliberate "not implemented" error or no route exists yet. The most visible gaps are alert write operations, schedules, enrichment tables, IAM invitations, correlation provider registry, report templates, and function dry-run execution.

## What Changes

- Implement body-driven create/update endpoints for alert rules, escalation policies, Notify connectors, and schedules.
- Add API coverage for enrichment tables, IAM invitations, correlation provider listing, report template listing, and function dry-run.
- Wire the remaining frontend pending states/actions to these real endpoints and reserve backend-pending only for genuinely unavailable functionality.

## Capabilities

### New Capabilities
- `backend-api-completion`: Missing API surfaces SHALL provide durable or deterministic responses instead of "not implemented" placeholders.

### Modified Capabilities
- `web-backend-pending-connectivity`: Frontend pending placeholders SHALL be replaced when the backend now exposes the required endpoint.

## Impact

- Affected backend code: `crates/api/src/http/routes/*`, `crates/api/src/state.rs`, `crates/bootstrap/src/wire.rs`, `crates/infra/src/persistence/repositories/*`, migrations.
- Affected frontend code: remaining pages/actions that currently show backend-pending copy.
- Database impact: add `invitations` table and small repository; reuse existing alert/schedule/enrichment repositories.
