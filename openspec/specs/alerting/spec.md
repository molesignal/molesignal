# Alerting Capability

## Purpose

AlertRule（scheduled / realtime / anomaly 三种 kind）评估循环、持久化连续匹配计数、Incident 生命周期（open/ack/resolved）、Schedule on-call 解析，以及 EscalationPolicy 升级与重复。所有告警投递统一通过 Notify 事件引擎完成。
## Requirements
### Requirement: AlertRule CRUD

The system SHALL expose `GET/POST /api/v1/alerts/rules`, `GET/PUT/DELETE /api/v1/alerts/rules/:id` backed by `AlertRuleRepository`.

#### Scenario: Create rule
- **WHEN** an Editor POSTs a valid `AlertRule` payload
- **THEN** the response is `201 Created` with the persisted rule (server-assigned `id`, `created_at`, `updated_at`)

#### Scenario: Delete rule cascades incidents
- **WHEN** a rule with open incidents is deleted
- **THEN** all open incidents for that rule are auto-resolved (`status = Resolved`, `resolved_by = system`) before the rule row is removed

### Requirement: AlertRule Kind Discriminator

`AlertRule` SHALL carry a `kind: "scheduled" | "realtime" | "anomaly"` discriminator (default `scheduled` for backward compatibility). Different kinds dispatch to different evaluation pipelines: scheduled (existing periodic loop), realtime (per-event matcher in ingester), anomaly (baseline-comparison detector). `RealTime` rules carry an additional `matcher: { field, op, value }` or `where_sql` constraint that compiles to a per-event predicate; `Anomaly` rules carry `anomaly_params` (see `anomaly-detection` capability).

#### Scenario: Scheduled rule unchanged
- **WHEN** an existing AlertRule with no `kind` field is loaded
- **THEN** the loader defaults `kind = "scheduled"` and the rule continues to evaluate on the periodic loop

#### Scenario: Realtime rule rejected by scheduler
- **WHEN** the periodic evaluator iterates rules
- **THEN** rules with `kind != "scheduled"` are skipped; counter `alert_evaluator_skipped_total{kind}` increments

### Requirement: Alert Rule Evaluation Loop

The alert_manager role SHALL evaluate every enabled `AlertRule` once per `alert_manager.eval_interval_secs` (default 30s) by running its `AlertQuery` over `[now - period_secs, now]`, comparing the scalar result against `AlertTrigger.{operator,threshold}`, persisting the consecutive-match counter via `alert_rule_eval_state`, and creating an `Incident` on first reaching `for_periods` consecutive matches or resolving the existing one when the condition no longer holds.

#### Scenario: Threshold crossed for required periods
- **WHEN** a rule with `for_periods = 3` evaluates true three consecutive ticks
- **THEN** a new `Incident { status: Open, current_step: 0, current_step_started_at = now, fingerprint = hash(rule_id, label_set) }` is inserted unless one with the same fingerprint already exists
- **AND** an idempotent `alert.triggered` event is enqueued into Notify

#### Scenario: Silenced period suppresses re-trigger
- **WHEN** a rule fires and `silence_secs` has not elapsed since the last `Incident.created_at`
- **THEN** no new incident is created even if the condition re-matches

#### Scenario: Condition clears
- **WHEN** the rule's condition is no longer true and an open incident exists
- **THEN** the incident transitions to `Resolved` with `resolved_by = system` and the rule's `consecutive_matches` resets to 0 in the same transaction

#### Scenario: Disabled rule skipped
- **WHEN** a rule has `enabled = false`
- **THEN** the loop skips it entirely; its `alert_rule_eval_state` row is left untouched

### Requirement: Real-Time Alert Evaluation At Ingest

`IngestService::ingest` SHALL, immediately after WAL append, evaluate every enabled `RealTime` AlertRule whose `query.stream` matches the inbound batch's stream against each record using the rule's compiled `matcher`/`where_sql`; matches emit an `IncidentEvent { rule_id, fingerprint, value, labels, ts }` published to a tokio broadcast channel that the alert_manager consumes for dispatch. End-to-end latency target ≤ 1s.

#### Scenario: Real-time match triggers incident
- **WHEN** a record `{ level: "fatal", msg: "OOM" }` arrives and a RealTime rule has matcher `level == "fatal"`
- **THEN** an `IncidentEvent` is published on the broadcast channel within the ingest handler; the alert_manager opens the incident and enqueues `alert.triggered` into Notify

#### Scenario: Non-matching event ignored
- **WHEN** a record does not satisfy any RealTime rule
- **THEN** no `IncidentEvent` is published; `realtime_match_evaluations_total{matched="false"} += 1`

### Requirement: Alert Rule Evaluation State Persistence

The alert_manager role SHALL persist per-rule consecutive-match counters in an `alert_rule_eval_state` table with columns `(rule_id PK, consecutive_matches INT, last_eval_at TIMESTAMPTZ, last_eval_value DOUBLE PRECISION NULL)`. The counter SHALL increment on each evaluation that matches the trigger, reset to zero on a non-match, and zero out when any open `Incident` for the rule transitions to `Resolved`.

#### Scenario: Counter increments across ticks
- **WHEN** a rule with `for_periods = 3` matches on three consecutive ticks
- **THEN** `alert_rule_eval_state.consecutive_matches` goes `1 → 2 → 3` and an `Incident` is created exactly when the row reaches 3

#### Scenario: Non-match resets counter
- **WHEN** the rule matches twice then misses on tick 3
- **THEN** `consecutive_matches` resets to 0 on tick 3, and no incident is created

#### Scenario: Resolve clears the counter
- **WHEN** an open incident for `rule_id = X` transitions to `Resolved` (system or user)
- **THEN** `alert_rule_eval_state.consecutive_matches` for `rule_id = X` is set to 0 in the same transaction

### Requirement: Evaluation Timeout

Each rule's `QueryEngine::execute` call SHALL be wrapped in `tokio::time::timeout(alert_manager.eval_timeout_secs)` (default 10s). Timeouts SHALL NOT abort the tick; they only abort the offending rule's evaluation and record a metric.

#### Scenario: Long-running rule does not block other rules
- **WHEN** rule `R1`'s query takes 30s and `eval_timeout_secs = 10`
- **THEN** `R1`'s evaluation aborts at 10s, `alert_rule_eval_timeout_total{rule_id="R1"}` increments, the rule's counter is NOT incremented, and rules `R2...Rn` evaluate on the same tick

### Requirement: Alert Rule Full HTTP CRUD

The system SHALL accept `POST /api/v1/alerts/rules` and `PUT /api/v1/alerts/rules/:id` with a fully-typed payload `{ name, query: { language, statement, time_range_secs }, trigger: { op, threshold }, for_periods, silence_secs, escalation_policy_id, enabled, labels: Map<String,String> }`. Request and response payload structures SHALL live in `crates/api/src/http/alerts/rule_request.rs` and `rule_response.rs` (file names MUST NOT contain `dto`).

#### Scenario: Create with all fields
- **WHEN** an Editor POSTs a complete valid payload
- **THEN** the response is `201 Created` with the persisted `AlertRule` (server-assigned `id`, `created_at`, `updated_at`)

#### Scenario: Invalid escalation_policy_id rejected
- **WHEN** the payload references a non-existent or cross-org `escalation_policy_id`
- **THEN** the response is `400 Bad Request` with `{ "error": "escalation_policy_id: not found in org" }`

#### Scenario: Update changes threshold
- **WHEN** a PUT changes `trigger.threshold`
- **THEN** the row updates atomically, `updated_at = now`, the row's `alert_rule_eval_state.consecutive_matches` is reset to 0 (threshold change invalidates running streak)

### Requirement: Escalation Dispatch

`EscalationDispatcher::tick` SHALL run once per `alert_manager.dispatch_interval_secs` (default 10s). For each open incident it SHALL enqueue one stable `alert.escalated` Notify event per current-step target, then advance to the next applicable step when `now - current_step_started_at >= step.ack_timeout_secs` and the incident is still unacknowledged.

#### Scenario: First-step dispatch on incident creation

- **WHEN** the dispatcher sees an open incident at step 0
- **THEN** it enqueues one `alert.escalated` event per target (user / schedule / team)
- **AND** the stable event id prevents a repeated tick from creating duplicate Notify deliveries

#### Scenario: Timeout advances to next step

- **WHEN** an open incident's current step has been live `>= ack_timeout_secs` and `acknowledged_at IS NULL`
- **THEN** `incident.current_step` is incremented, `current_step_started_at` is reset to `now`, and the new step's target events are enqueued

#### Scenario: Repeat at policy end

- **WHEN** the dispatcher would advance past the last step of a policy whose `repeat = true`
- **THEN** it loops back to step 0 up to `max_loops` total iterations, after which it stops dispatching for that incident

### Requirement: Escalation Policy Full HTTP CRUD

The system SHALL accept `POST/PUT /api/v1/alerts/escalations` with payload `{ name, steps: Vec<{ targets: Vec<{ kind: "user" | "schedule" | "team", id }>, ack_timeout_secs, min_severity? }>, repeat, max_loops }`. Escalation policies SHALL NOT select connectors or templates; Notify policies own delivery routing.

#### Scenario: Multi-step policy created
- **WHEN** a POST has 3 steps with different target kinds
- **THEN** the row stores all steps in order (JSONB column), and dispatcher honors the order at runtime

#### Scenario: Empty steps rejected
- **WHEN** `steps = []`
- **THEN** the response is `400 Bad Request` with `{ "error": "steps: at least one step required" }`

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

### Requirement: Schedule Full HTTP CRUD with Rotations and Overrides

The system SHALL accept `POST/PUT /api/v1/schedules` with payload `{ name, timezone, rotations: Vec<Rotation>, overrides: Vec<{ user_id, start, end }> }` and additionally `POST/DELETE /api/v1/schedules/:id/overrides/:override_id` for incremental override management. Request/response types live in `schedule_request.rs` and `schedule_response.rs`.

#### Scenario: Schedule with two rotations created
- **WHEN** a POST has weekday + weekend rotations
- **THEN** the response is `201 Created` and `who_is_on_call(at)` returns the correct member at any `at` per the active window resolution

#### Scenario: Override window added incrementally
- **WHEN** an existing schedule receives `POST /schedules/:id/overrides` with `{ user_id, start, end }`
- **THEN** the response is `201 Created` with the inserted override and `who_is_on_call(at within window)` returns the override's user_id

### Requirement: Incident Acknowledge & Resolve

`POST /api/v1/alerts/incidents/:id/ack` and `.../resolve` SHALL transition `IncidentStatus`, stamp `acknowledged_at`/`resolved_at`, and set `acknowledged_by`/`resolved_by` from the authenticated user.

#### Scenario: Ack stops escalation
- **WHEN** a user acks an incident currently at step N
- **THEN** subsequent dispatcher ticks do not advance the step (since `step_timed_out` returns false for non-Open statuses)

### Requirement: Alert Lifecycle Notify Events

Alert rule evaluation and incident actions SHALL enqueue `alert.triggered`,
`alert.acknowledged`, and `alert.resolved` events. Escalation dispatch SHALL
enqueue `alert.escalated`. Alerting SHALL NOT own connector CRUD, message
templates, subscription fan-out, or delivery attempt persistence.

#### Scenario: Acknowledge emits an event and stops escalation

- **WHEN** a user acknowledges an open incident
- **THEN** the incident transitions to acknowledged and an
  `alert.acknowledged` event is enqueued
- **AND** later dispatcher ticks do not advance its escalation step
