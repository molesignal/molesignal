## ADDED Requirements

### Requirement: Dispatcher Adapter Port

The application layer SHALL expose an `ActionExecutorPort` trait that takes `(action_id, IncidentContext)` and returns an `ExecutionResult`. The OSS build SHALL provide a `NoopActionExecutor` returning `ExecutionStatus::Skipped` with reason `"actions feature not licensed"`. The enterprise wire stage SHALL inject an adapter wrapping `molesignal_enterprise_actions::ActionExecutor` + `ActionRepository::get` + `ActionRepository::record_execution`.

#### Scenario: OSS noop adapter skips invocation

- **WHEN** an OSS build's dispatcher invokes `ActionExecutorPort::execute("a1", ctx)`
- **THEN** it returns `ExecutionResult { status: Skipped, error: Some("actions feature not licensed"), … }` without contacting the network

#### Scenario: Enterprise adapter records execution

- **WHEN** enterprise build with valid `actions` license invokes the adapter against a stored `actions` row whose `kind = Webhook { url, headers, body_template }`
- **THEN** the adapter fetches the action, calls `ActionExecutor::execute`, and writes one `action_executions` row with status + status_code + duration_ms

#### Scenario: Unknown action_id is recorded as failed

- **WHEN** the adapter is invoked with `action_id` that does not exist in the `actions` table
- **THEN** it returns `ExecutionResult { status: Failed, error: Some("action not found") }` and inserts a corresponding `action_executions` row (so users can see the broken reference in the executions list)
