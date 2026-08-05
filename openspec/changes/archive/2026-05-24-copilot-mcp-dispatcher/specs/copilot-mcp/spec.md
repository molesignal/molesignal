## ADDED Requirements

### Requirement: Real ToolDispatcher For Builtin Tools

The system SHALL provide a `RealToolDispatcher` implementation of `ToolDispatcher` that routes each of the 5 builtin tools to the corresponding OSS subsystem:

| tool | impl |
|---|---|
| `query_logs` | `QueryService::run(req)` with `language=SQL`, `stream_type=Logs` |
| `query_metrics` | `QueryService::run(req)` with `language=PromQL` |
| `list_streams` | `StreamRepository::list(org_id)` optionally filtered by `stream_type` |
| `get_trace` | SQL `SELECT * FROM traces WHERE trace_id = ?` via `QueryService`, 行转 `Span` 树 |
| `list_recent_alerts` | `IncidentRepository::list_active(org_id)` limit by `args.limit` |

#### Scenario: query_logs returns rows as ToolContent::Json

- **WHEN** MCP client calls `tools/call` with `{ name: "query_logs", arguments: { sql, stream, time_range } }`
- **AND** the underlying `QueryService::run` returns 3 rows
- **THEN** `ToolResult { content: [Json{ json: { columns, rows } }], is_error: false }` is sent back over WebSocket

#### Scenario: Unknown tool returns error result

- **WHEN** client calls `tools/call` with `name: "delete_everything"`
- **THEN** dispatcher returns `ToolResult { content: [Text { text: "unknown tool: delete_everything" }], is_error: true }`

#### Scenario: org isolation enforced

- **WHEN** session authenticated as `org=A` calls `list_streams`
- **THEN** `StreamRepository::list("A")` is invoked，结果 only contains streams of org A; org B's streams are never visible

#### Scenario: get_trace returns spans tree

- **WHEN** trace_id `abc123` exists in traces stream with 5 spans
- **THEN** `ToolResult.content[0]` is `Json { json: { trace_id, root_span_id, spans: [...] } }`

### Requirement: MCP Tool Calls Recorded To copilot_traces

For every successful or failed `tools/call`, the dispatcher SHALL emit one event to the `copilot_traces` stream with fields `{ type: "mcp_tool_call", tool_name, org_id, user_id, status: success|error, duration_ms, error?: string }`. Best-effort: failure to write the trace MUST NOT fail the tool call itself.

#### Scenario: Successful call writes one trace event

- **WHEN** `query_logs` completes successfully in 12ms
- **THEN** `copilot_traces` stream has one event with `tool_name="query_logs"`, `status="success"`, `duration_ms=12`

#### Scenario: Trace write failure does not break dispatch

- **WHEN** ingest path is unhealthy and writing `copilot_traces` returns Err
- **THEN** the `tools/call` response is still returned to the WebSocket client (tool result wins), and a warn log records the trace write failure
