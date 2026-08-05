## ADDED Requirements

### Requirement: Chat session and message endpoints

The system SHALL expose `POST /api/v1/copilot/chat/sessions` to create a session and `POST /api/v1/copilot/chat/sessions/:id/messages` to send a user message and receive a streamed (SSE) assistant reply. Each session is org-scoped and user-bound. Both endpoints require `license.has_feature("copilot")`.

#### Scenario: Streaming reply

- **WHEN** a user POSTs a message to `/api/v1/copilot/chat/sessions/<id>/messages` with `Accept: text/event-stream`
- **THEN** the response is `text/event-stream` and emits `data: <token>` chunks until `event: done`

### Requirement: Tool-augmented inference

The chat backend SHALL be wired to the same tool registry as `copilot-mcp`. When the LLM emits a tool call, the system SHALL invoke the tool with the user's org scope and feed the result back to the LLM. All tool invocations SHALL be logged to `copilot_traces` with `gen_ai.tool.name` and `gen_ai.tool.result.tokens`.

#### Scenario: Natural language → SQL → answer

- **WHEN** a user asks "how many errors in the last 24 hours"
- **THEN** the LLM emits a `query_logs` tool call, the system executes the SQL, returns the count, the LLM produces the final answer

### Requirement: Provider selection

The chat backend SHALL support OpenAI, Anthropic, and a generic OpenAI-compatible endpoint (for self-hosted models). Provider is configured per-org via `[copilot.chat.provider]` setting.

#### Scenario: Per-org provider isolation

- **WHEN** org A uses Anthropic and org B uses OpenAI
- **THEN** chat requests from A go to api.anthropic.com, B to api.openai.com; no cross-mixing
