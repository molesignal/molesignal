## ADDED Requirements

### Requirement: Escalation Target Action Variant

`EscalationTarget` SHALL support an `Action { action_id: Id }` variant in addition to `User` / `Schedule` / `Team`. Policy steps SHALL accept any mix of these target kinds.

#### Scenario: Action target serde round-trips

- **WHEN** an `EscalationPolicy` is stored with one step containing `[{kind: "action", action_id: "a1"}]`
- **AND** it is read back from `escalation_policies.steps` JSONB
- **THEN** the deserialized variant equals `EscalationTarget::Action { action_id: Id("a1") }`

#### Scenario: Mixed step accepted

- **WHEN** a POST `/api/v1/alerts/policies` body declares a step with `targets: [{kind: "user", user_id, channel_ids}, {kind: "action", action_id}]`
- **THEN** the policy is created and both target kinds are persisted in order

## MODIFIED Requirements

### Requirement: Escalation Dispatch

`EscalationDispatcher::tick` SHALL run once per `alert_manager.dispatch_interval_secs` (default 10s) and for each open incident send the current step's notifications on first encounter, then advance to the next step when `now - current_step_started_at >= step.ack_timeout_secs` and the incident is still unacknowledged. When a step's target is of kind `action`, the dispatcher SHALL invoke the action via `ActionExecutorPort::execute(action_id, IncidentContext)` and record the result in `action_executions`; failures of the action target SHALL NOT block dispatch of sibling targets in the same step.

#### Scenario: First-step dispatch on incident creation

- **WHEN** the dispatcher sees an open incident whose `current_step == 0` and which has no prior `Delivery` rows
- **THEN** it resolves the step's `targets` (user / schedule / team / action) and sends one notification per `(target, channel)` pair (for user/schedule/team) or one action execution (for action), recording one `Delivery` or one `action_executions` row each

#### Scenario: Timeout advances to next step

- **WHEN** an open incident's current step has been live `>= ack_timeout_secs` and `acknowledged_at IS NULL`
- **THEN** `incident.current_step` is incremented, `current_step_started_at` is reset to `now`, and the new step's notifications are dispatched

#### Scenario: Repeat at policy end

- **WHEN** the dispatcher would advance past the last step of a policy whose `repeat = true`
- **THEN** it loops back to step 0 up to `max_loops` total iterations, after which it stops dispatching for that incident

#### Scenario: Action target without enterprise feature is skipped

- **WHEN** an OSS build (no `enterprise` feature) or `license.has_feature("actions") == false` encounters a step with an `action` target
- **THEN** the dispatcher logs a warning and continues processing siblings; the step still advances per its `ack_timeout_secs`

#### Scenario: Action execution recorded on success and failure

- **WHEN** an action target invokes `ActionExecutor::execute` and returns `ExecutionResult { status: success|failed|skipped|timeout, … }`
- **THEN** the dispatcher inserts one row into `action_executions` with `(action_id, incident_id, status, status_code, response_body, error, duration_ms)`
