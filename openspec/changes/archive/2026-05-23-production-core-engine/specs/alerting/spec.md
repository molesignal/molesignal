## ADDED Requirements

### Requirement: AlertRule Kind Discriminator

`AlertRule` SHALL carry a `kind: "scheduled" | "realtime" | "anomaly"` discriminator (default `scheduled` for backward compatibility). Different kinds dispatch to different evaluation pipelines: scheduled (existing periodic loop), realtime (per-event matcher in ingester), anomaly (baseline-comparison detector). `RealTime` rules carry an additional `matcher: { field, op, value }` or `where_sql` constraint that compiles to a per-event predicate; `Anomaly` rules carry `anomaly_params` (see `anomaly-detection` capability).

#### Scenario: Scheduled rule unchanged
- **WHEN** an existing AlertRule with no `kind` field is loaded
- **THEN** the loader defaults `kind = "scheduled"` and the rule continues to evaluate on the periodic loop

#### Scenario: Realtime rule rejected by scheduler
- **WHEN** the periodic evaluator iterates rules
- **THEN** rules with `kind != "scheduled"` are skipped; counter `alert_evaluator_skipped_total{kind}` increments

### Requirement: Real-Time Alert Evaluation At Ingest

`IngestService::ingest` SHALL, immediately after WAL append, evaluate every enabled `RealTime` AlertRule whose `query.stream` matches the inbound batch's stream against each record using the rule's compiled `matcher`/`where_sql`; matches emit an `IncidentEvent { rule_id, fingerprint, value, labels, ts }` published to a tokio broadcast channel that the alert_manager consumes for dispatch. End-to-end latency target ≤ 1s.

#### Scenario: Real-time match triggers incident
- **WHEN** a record `{ level: "fatal", msg: "OOM" }` arrives and a RealTime rule has matcher `level == "fatal"`
- **THEN** an `IncidentEvent` is published on the broadcast channel within the ingest handler; the alert_manager picks it up and dispatches notifications via the existing `EscalationDispatcher`

#### Scenario: Non-matching event ignored
- **WHEN** a record does not satisfy any RealTime rule
- **THEN** no `IncidentEvent` is published; `realtime_match_evaluations_total{matched="false"} += 1`

### Requirement: Alert Message Templates

`channels` and each `escalation_step` SHALL accept an optional `body_template: String` containing mustache-style `{{var}}` placeholders. The renderer (used by `MultiNotifier`) SHALL substitute from a context `{ rule: { name, kind }, incident: { fingerprint, labels, status }, value, threshold, evaluated_at, org: { name, id } }`. Missing variables SHALL render as the literal placeholder text and increment `alert_template_missing_var_total{var}`.

#### Scenario: Slack template substituted
- **WHEN** a slack channel has `body_template: "[{{rule.name}}] value={{value}} > threshold={{threshold}} fp={{incident.fingerprint}}"`
- **THEN** the rendered message contains real values for each placeholder

#### Scenario: Unknown variable preserved
- **WHEN** the template references `{{rule.unknown_field}}`
- **THEN** the literal `{{rule.unknown_field}}` appears in the output and the counter increments

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

### Requirement: Escalation Policy Full HTTP CRUD

The system SHALL accept `POST/PUT /api/v1/alerts/policies` with payload `{ name, steps: Vec<{ targets: Vec<{ kind: "user" | "schedule" | "team", id }>, ack_timeout_secs, channel_ids: Vec<Id> }>, repeat, max_loops }`. Request/response types live in `policy_request.rs` and `policy_response.rs`.

#### Scenario: Multi-step policy created
- **WHEN** a POST has 3 steps with different target kinds
- **THEN** the row stores all steps in order (JSONB column), and dispatcher honors the order at runtime

#### Scenario: Empty steps rejected
- **WHEN** `steps = []`
- **THEN** the response is `400 Bad Request` with `{ "error": "steps: at least one step required" }`

### Requirement: Channel Full HTTP CRUD

The system SHALL accept `POST/PUT /api/v1/alerts/channels` with payload `{ name, kind: "email" | "slack" | "webhook", config: { ... per-kind shape ... } }`. Per-kind validation SHALL reject malformed config (e.g., slack without `webhook_url`, email without `to`).

#### Scenario: Slack channel created
- **WHEN** a POST has `kind="slack"`, `config={ "webhook_url": "https://hooks.slack.com/..." }`
- **THEN** the response is `201 Created`

#### Scenario: Slack channel missing webhook_url rejected
- **WHEN** a POST has `kind="slack"` and `config={}`
- **THEN** the response is `400 Bad Request` with `{ "error": "slack channel: webhook_url required" }`

### Requirement: Schedule Full HTTP CRUD with Rotations and Overrides

The system SHALL accept `POST/PUT /api/v1/schedules` with payload `{ name, timezone, rotations: Vec<Rotation>, overrides: Vec<{ user_id, start, end }> }` and additionally `POST/DELETE /api/v1/schedules/:id/overrides/:override_id` for incremental override management. Request/response types live in `schedule_request.rs` and `schedule_response.rs`.

#### Scenario: Schedule with two rotations created
- **WHEN** a POST has weekday + weekend rotations
- **THEN** the response is `201 Created` and `who_is_on_call(at)` returns the correct member at any `at` per the active window resolution

#### Scenario: Override window added incrementally
- **WHEN** an existing schedule receives `POST /schedules/:id/overrides` with `{ user_id, start, end }`
- **THEN** the response is `201 Created` with the inserted override and `who_is_on_call(at within window)` returns the override's user_id

### Requirement: Evaluation Timeout

Each rule's `QueryEngine::execute` call SHALL be wrapped in `tokio::time::timeout(alert_manager.eval_timeout_secs)` (default 10s). Timeouts SHALL NOT abort the tick; they only abort the offending rule's evaluation and record a metric.

#### Scenario: Long-running rule does not block other rules
- **WHEN** rule `R1`'s query takes 30s and `eval_timeout_secs = 10`
- **THEN** `R1`'s evaluation aborts at 10s, `alert_rule_eval_timeout_total{rule_id="R1"}` increments, the rule's counter is NOT incremented, and rules `R2...Rn` evaluate on the same tick

## MODIFIED Requirements

### Requirement: Alert Rule Evaluation Loop

The alert_manager role SHALL evaluate every enabled `AlertRule` once per `alert_manager.eval_interval_secs` (default 30s) by running its `AlertQuery` over `[now - period_secs, now]`, comparing the scalar result against `AlertTrigger.{operator,threshold}`, persisting the consecutive-match counter via `alert_rule_eval_state`, and creating an `Incident` on first reaching `for_periods` consecutive matches or resolving the existing one when the condition no longer holds.

#### Scenario: Threshold crossed for required periods
- **WHEN** a rule with `for_periods = 3` evaluates true three consecutive ticks
- **THEN** a new `Incident { status: Open, current_step: 0, current_step_started_at = now, fingerprint = hash(rule_id, label_set) }` is inserted unless one with the same fingerprint already exists

#### Scenario: Silenced period suppresses re-trigger
- **WHEN** a rule fires and `silence_secs` has not elapsed since the last `Incident.created_at`
- **THEN** no new incident is created even if the condition re-matches

#### Scenario: Condition clears
- **WHEN** the rule's condition is no longer true and an open incident exists
- **THEN** the incident transitions to `Resolved` with `resolved_by = system` and the rule's `consecutive_matches` resets to 0 in the same transaction

#### Scenario: Disabled rule skipped
- **WHEN** a rule has `enabled = false`
- **THEN** the loop skips it entirely; its `alert_rule_eval_state` row is left untouched
