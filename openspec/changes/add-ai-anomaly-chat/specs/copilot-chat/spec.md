## MODIFIED Requirements

### Requirement: Tool-augmented inference

The chat backend SHALL be wired to the same real backend tool dispatcher as `copilot-mcp`. When the LLM emits a tool call, the system SHALL validate the tool name and arguments, invoke the tool with the authenticated user's org scope, persist compact tool evidence metadata, optionally spill large results to object storage, and feed a bounded result summary back to the LLM. All tool invocations SHALL be logged to `copilot_traces` and audited as AI tool-call lifecycle events.

#### Scenario: Natural language -> SQL -> answer

- **WHEN** a user asks "how many errors in the last 24 hours"
- **THEN** the LLM emits a `query_logs` tool call
- **AND** the system executes the SQL through the backend query service using the authenticated org
- **AND** the LLM produces the final answer from the bounded tool result summary

#### Scenario: Tool result evidence is retained

- **WHEN** a chat tool call completes successfully
- **THEN** the chat message metadata includes tool name, sanitized arguments, status, latency, row count or scanned row count when available, and optional object key

### Requirement: Provider selection

The chat backend SHALL support OpenAI, Anthropic, and a generic OpenAI-compatible endpoint. Provider configuration SHALL be selected from Postgres-backed org provider rows, not process environment variables, unless a deployment explicitly enables an environment fallback for local development. Each chat session SHALL store the selected provider id, provider type, and model. API keys SHALL be read through the encrypted provider secret repository and MUST NOT be returned to clients.

#### Scenario: Per-org provider isolation

- **WHEN** org A configures Anthropic and org B configures OpenAI
- **THEN** chat requests from org A use org A's encrypted Anthropic provider configuration
- **AND** chat requests from org B use org B's encrypted OpenAI provider configuration
- **AND** neither org can list or use the other's providers

#### Scenario: Disabled provider rejected

- **WHEN** a chat session references a disabled provider
- **THEN** posting a message returns `400 Bad Request` or another typed client error
- **AND** no outbound model request is made

### Requirement: Chat session and message endpoints

The system SHALL expose `POST /api/v1/copilot/chat/sessions` to create a session and `POST /api/v1/copilot/chat/sessions/:id/messages` to send a user message and receive a streamed SSE assistant reply. Each session is org-scoped and user-bound. Both endpoints require `license.has_feature("copilot")`. Message requests MAY include time range, analysis mode, stream hints, provider id, and prompt template id. The backend SHALL persist user messages, assistant messages, prompt references, prompt hashes, tool evidence metadata, token counts, and cost attribution.

#### Scenario: Streaming reply

- **WHEN** a user POSTs a message to `/api/v1/copilot/chat/sessions/<id>/messages` with `Accept: text/event-stream`
- **THEN** the response is `text/event-stream`
- **AND** emits `event: chunk` frames until `event: done` or `event: error`

#### Scenario: Prompt metadata persisted

- **WHEN** a user sends a message with analysis mode `root_cause`
- **THEN** the persisted message or assistant response metadata includes selected prompt template id, builtin key, version, and rendered prompt hash

#### Scenario: Time range persisted

- **WHEN** a user asks a question scoped to a time range
- **THEN** the chat session/message metadata stores that time range for history, audit, and archive review

## ADDED Requirements

### Requirement: Prompt-backed system context

Before calling a provider, the chat backend SHALL resolve the active prompt template for the request purpose, render it with allowed variables, and include it as system context or provider-equivalent instruction. The rendered prompt body SHALL NOT be exposed in audit payloads when it exceeds the audit payload limit; prompt id, version, builtin key, and hash SHALL be sufficient for traceability.

#### Scenario: Root-cause mode uses root-cause prompt

- **WHEN** a chat request specifies `analysis_mode = "root_cause"`
- **THEN** the backend resolves the default root-cause prompt for the caller
- **AND** uses it as the model instruction for that provider call

### Requirement: Soft delete for chat sessions

Deleting an AI chat session SHALL mark the session deleted instead of physically deleting messages immediately. Deleted sessions SHALL be hidden from normal history lists but remain available to archive/audit retention paths until retention removes them.

#### Scenario: Deleted session hidden but retained

- **WHEN** a user deletes a chat session
- **THEN** normal session list responses exclude it
- **AND** audit/archive lookup paths can still resolve the session metadata until retention cleanup
