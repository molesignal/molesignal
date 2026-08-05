# Copilot Chat Capability ()

## Purpose

对话式查询接口：自然语言 → SQL/PromQL，叠加 RAG over `copilot_traces` 派生流，流式响应。 特性。
## Requirements
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

### Requirement: Provider Adapter Implementations

The `copilot-chat` crate SHALL ship three concrete `ProviderAdapter` impls covering the dominant LLM API protocols:

1. `OpenAiAdapter`：POST `{base_url}/chat/completions` with `stream: true`，按 `data: <json>\n\n` SSE 帧解析 `choices[0].delta.content` 与 `tool_calls`。
2. `AnthropicAdapter`：POST `{base_url}/v1/messages` with `stream: true`，按 `event: content_block_delta` / `event: message_delta` SSE 帧解析。
3. `OpenAiCompatibleAdapter`：与 OpenAi 同协议，`base_url` 可指向 Together / vLLM / Ollama / Groq；行为与 OpenAi 完全一致，仅 `base_url` 不同。

Each adapter SHALL surface incremental tokens through an `async fn complete_stream(req) -> impl Stream<Item = ChunkOrToolCall>`.

#### Scenario: OpenAI adapter parses SSE chunks

- **WHEN** a mock HTTP server returns `data: {"choices":[{"delta":{"content":"hi"}}]}\n\ndata: [DONE]\n\n`
- **THEN** `OpenAiAdapter::complete_stream(req)` yields exactly one `ChunkOrToolCall::Text("hi")` then stream end

#### Scenario: Anthropic adapter parses message_delta

- **WHEN** mock returns `event: content_block_delta\ndata: {"delta":{"type":"text_delta","text":"hi"}}\n\n`
- **THEN** `AnthropicAdapter::complete_stream(req)` yields `ChunkOrToolCall::Text("hi")`

#### Scenario: Tool call yielded as separate chunk

- **WHEN** OpenAI response contains `delta.tool_calls: [{id, function: {name, arguments}}]`
- **THEN** the stream yields `ChunkOrToolCall::ToolCall { id, name, arguments_partial }`，ChatLoop 据此调 dispatcher 后回灌

### Requirement: ChatLoop Integrated With Real Providers

The `ChatLoop::run` execution SHALL replace the placeholder path in `crates/api/src/http/routes/copilot_chat.rs::post_message`:

1. Load message history from `chat_messages`.
2. Call `ProviderAdapter::complete_stream`.
3. Forward each text chunk to client as SSE `event: chunk`.
4. On tool_call chunk, await `ToolDispatcher::dispatch`, append result as `role: tool` message, restart provider call.
5. Persist final assistant message + token counts + `cost_usd` (computed via `model_prices`).

#### Scenario: End-to-end happy path (no tool call)

- **WHEN** POST `/api/v1/copilot/chat/sessions/{id}/messages` with `{ content: "hi" }`
- **AND** provider returns "hello world" in 3 chunks
- **THEN** client receives 3 `event: chunk` SSE frames then `event: done`
- **AND** `chat_messages` contains one new `role: assistant` row with `content = "hello world"` and `cost_usd` populated

#### Scenario: Tool call loop terminates within MAX_TOOL_LOOPS

- **WHEN** provider keeps requesting tool calls beyond `MAX_TOOL_LOOPS=8`
- **THEN** ChatLoop aborts with `event: error` SSE frame and `chat_messages` gets a `role: assistant` row with content `"[aborted: tool loop budget exhausted]"`

#### Scenario: Provider error surfaces to client

- **WHEN** provider returns HTTP 429 or connection reset
- **THEN** SSE emits `event: error` with `{ status: 429, message }` and stream ends
- **AND** a `role: assistant` row is appended with `content = "[error: <message>]"` so the conversation history stays coherent

### Requirement: Cost Computed Per Assistant Message

For each completed assistant message, the system SHALL look up `(session.provider, session.model)` in `model_prices` and compute `cost_usd = prompt_tokens/1000 * prompt_usd_per_1k + completion_tokens/1000 * completion_usd_per_1k`, persisting it to `chat_messages.cost_usd`. Missing catalog entry → `cost_usd = NULL` (not zero) and a warn log.

#### Scenario: Known model yields non-zero cost

- **WHEN** session uses `provider=openai, model=gpt-4o` and assistant chunk reports `usage: { prompt_tokens: 100, completion_tokens: 200 }`
- **THEN** `chat_messages.cost_usd = 0.0035` (0.005 * 0.1 + 0.015 * 0.2)

#### Scenario: Unknown model leaves cost null

- **WHEN** session uses `model=fictional-9001` not in `model_prices`
- **THEN** `chat_messages.cost_usd IS NULL` and a structured log records `model_prices missed: provider=..., model=...`

