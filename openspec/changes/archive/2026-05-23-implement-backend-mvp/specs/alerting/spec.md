## ADDED Requirements

### Requirement: AlertRule CRUD

The system SHALL expose `GET/POST /api/v1/alerts/rules`, `GET/PUT/DELETE /api/v1/alerts/rules/:id` backed by `AlertRuleRepository`.

#### Scenario: Create rule
- **WHEN** an Editor POSTs a valid `AlertRule` payload
- **THEN** the response is `201 Created` with the persisted rule (server-assigned `id`, `created_at`, `updated_at`)

#### Scenario: Delete rule cascades incidents
- **WHEN** a rule with open incidents is deleted
- **THEN** all open incidents for that rule are auto-resolved (`status = Resolved`, `resolved_by = system`) before the rule row is removed

### Requirement: Alert Rule Evaluation Loop

The alert_manager role SHALL evaluate every enabled `AlertRule` once per `alert_manager.eval_interval_secs` (default 30s) by running its `AlertQuery` over `[now - period_secs, now]`, comparing the scalar result against `AlertTrigger.{operator,threshold}` for `for_periods` consecutive evaluations, and creating an `Incident` on first match or resolving the existing one when the condition no longer holds.

#### Scenario: Threshold crossed for required periods
- **WHEN** a rule with `for_periods = 3` evaluates true three consecutive ticks
- **THEN** a new `Incident { status: Open, current_step: 0, current_step_started_at = now, fingerprint = hash(rule_id, label_set) }` is inserted unless one with the same fingerprint already exists

#### Scenario: Silenced period suppresses re-trigger
- **WHEN** a rule fires and `silence_secs` has not elapsed since the last `Incident.created_at`
- **THEN** no new incident is created even if the condition re-matches

#### Scenario: Condition clears
- **WHEN** the rule's condition is no longer true and an open incident exists
- **THEN** the incident transitions to `Resolved` with `resolved_by = system`

### Requirement: Escalation Dispatch

`EscalationDispatcher::tick` SHALL run once per `alert_manager.dispatch_interval_secs` (default 10s) and for each open incident send the current step's notifications on first encounter, then advance to the next step when `now - current_step_started_at >= step.ack_timeout_secs` and the incident is still unacknowledged.

#### Scenario: First-step dispatch on incident creation
- **WHEN** the dispatcher sees an open incident whose `current_step == 0` and which has no prior `Delivery` rows
- **THEN** it resolves the step's `targets` (user / schedule / team) and sends one notification per `(target, channel)` pair, recording one `Delivery` row each

#### Scenario: Timeout advances to next step
- **WHEN** an open incident's current step has been live `>= ack_timeout_secs` and `acknowledged_at IS NULL`
- **THEN** `incident.current_step` is incremented, `current_step_started_at` is reset to `now`, and the new step's notifications are dispatched

#### Scenario: Repeat at policy end
- **WHEN** the dispatcher would advance past the last step of a policy whose `repeat = true`
- **THEN** it loops back to step 0 up to `max_loops` total iterations, after which it stops dispatching for that incident

### Requirement: On-call Resolution

`GET /api/v1/schedules/:id/on-call?at=<unix_ts>` SHALL return the user currently on call per `Schedule::who_is_on_call`, honoring `ScheduleOverride` first and then `Rotation` cadence, with `at` defaulting to the current time when omitted.

#### Scenario: Override takes precedence
- **WHEN** `at` falls inside a `ScheduleOverride`
- **THEN** the response is `{ "user_id": <override.user_id> }`

#### Scenario: Outside any active window returns null
- **WHEN** no rotation's `ActiveWindow` matches and no override applies at `at`
- **THEN** the response is `{ "user_id": null }`

### Requirement: ActiveWindow Time-zone Awareness

`Rotation::resolve` SHALL convert `at` to the schedule's IANA `timezone` before evaluating `ActiveWindow.{weekday_mask, hour_start, hour_end}`.

#### Scenario: Hour window respects zone
- **WHEN** a schedule has `timezone = "Asia/Shanghai"` and `ActiveWindow { hour_start: 9, hour_end: 18 }`
- **THEN** an `at` corresponding to 02:00 UTC on a weekday (10:00 in Shanghai) is considered in-window

### Requirement: Incident Acknowledge & Resolve

`POST /api/v1/alerts/incidents/:id/ack` and `.../resolve` SHALL transition `IncidentStatus`, stamp `acknowledged_at`/`resolved_at`, and set `acknowledged_by`/`resolved_by` from the authenticated user.

#### Scenario: Ack stops escalation
- **WHEN** a user acks an incident currently at step N
- **THEN** subsequent dispatcher ticks do not advance the step (since `step_timed_out` returns false for non-Open statuses)

### Requirement: Multi-channel Notification

`MultiNotifier` SHALL dispatch to Slack, Webhook, PagerDuty, Email (lettre SMTP), and SMS (pluggable provider trait, default no-op) according to `ChannelKind`, and record a `Delivery` row with `status`, `attempted_at`, `finished_at`, and `error` on every attempt.

#### Scenario: Slack failure recorded
- **WHEN** the Slack webhook returns 5xx
- **THEN** the `Delivery` row is `status = Failed` with the HTTP status in `error`, and the dispatcher does NOT advance the step purely because of the failure (timeout logic is independent)

#### Scenario: Email sent via SMTP
- **WHEN** an `Email` channel is targeted and `[notify.smtp]` is configured
- **THEN** lettre sends to all `to` addresses using the configured SMTP server and TLS settings
