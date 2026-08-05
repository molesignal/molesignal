## ADDED Requirements

### Requirement: MCP server endpoint

The system SHALL expose `/mcp` implementing Model Context Protocol over WebSocket. The handshake SHALL include capability negotiation per MCP spec; supported version is recorded as `MCP_PROTOCOL_VERSION` constant.

#### Scenario: Handshake completes

- **WHEN** an MCP client connects to `/mcp` with valid bearer token
- **THEN** the server responds with `initialize` capabilities including registered tools, and the connection moves to ready state

### Requirement: Tool registry — telemetry primitives

The MCP server SHALL register tools: `query_logs(sql, time_range, stream)`, `query_metrics(promql, time_range, step)`, `list_streams(stream_type)`, `get_trace(trace_id)`, `list_recent_alerts(limit)`. Each tool's invocation SHALL be permission-checked via the bearer token's identity.

#### Scenario: query_logs respects org scope

- **WHEN** an MCP client invokes `query_logs` with a token bound to org A
- **THEN** the underlying SQL has `org_id = A` rewritten in via the planner; results from other orgs are impossible

### Requirement: License gating

MCP endpoint SHALL be available only when `license.has_feature("copilot")` is true. OSS builds SHALL NOT compile the MCP module at all (cfg-gated).

#### Scenario: OSS path unreachable

- **WHEN** OSS build and a client attempts to connect to `/mcp`
- **THEN** the system returns 404 (route is not registered)
