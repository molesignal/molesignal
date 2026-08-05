## Context

The repository already contains most low-level primitives needed for an AI investigation feature:

- `copilot-chat` provider adapters and streaming `ChatLoop`.
- `copilot-mcp` and a real backend `ToolDispatcher` for logs, metrics, streams, traces, and recent alerts.
- `chat_sessions` / `chat_messages` persistence.
- `model_prices` for cost attribution.
- `audit_events` persistence and a minimal `/audit` endpoint.
- `CipherRootKey` envelope encryption patterns for secrets.
- Object-store backed blob storage for large investigation payloads.

The missing product behavior is the composition layer: provider configuration is not managed in Postgres, prompts are not first-class editable resources, the chat route still needs to consistently use trusted backend tools, audit search is too limited for compliance review, and there is no frontend page for either audit history or the AI chat workflow.

## Goals / Non-Goals

**Goals:**

- Provide an AI chat page for time-range based anomaly and root-cause analysis.
- Keep all tenant data access behind backend-owned tools and auth context.
- Store model provider configuration in Postgres and encrypt API keys with `CipherRootKey`.
- Provide built-in default prompts for common AI investigation modes and let Admins/users customize active prompts without code changes.
- Persist chat sessions/messages and archive full transcripts plus large tool results to object storage.
- Add auditable lifecycle events for provider, prompt, chat, tool-call, and archive operations.
- Add a searchable audit query page in the web admin surface.

**Non-Goals:**

- Build a general autonomous remediation agent.
- Let prompts execute arbitrary SQL or bypass tool schemas.
- Add a new object-store abstraction; this uses the existing configured object store.
- Add vector/RAG infrastructure in this pass.
- Implement model-provider billing beyond existing `model_prices` cost attribution.

## Decisions

### Provider Configuration

Add `ai_model_providers` and `ai_model_provider_secrets` persistence:

- `ai_model_providers`: org-scoped provider metadata such as `provider`, `name`, `base_url`, `default_model`, `enabled`, `timeout_ms`, `max_tokens`, and timestamps.
- `ai_model_provider_secrets`: encrypted API key material keyed by provider id; plaintext is sealed with `CipherRootKey` and never serialized.

Provider routes are Admin+:

- `GET /api/v1/ai/providers`
- `POST /api/v1/ai/providers`
- `PUT /api/v1/ai/providers/{id}`
- `POST /api/v1/ai/providers/{id}/rotate_key`
- `DELETE /api/v1/ai/providers/{id}`

Reasoning: storing provider config in PG matches the existing metadata plane and allows audit, UI management, and per-org defaults. Reusing the `CipherRootKey` pattern avoids introducing a second secret management model.

Alternative considered: keep provider API keys in environment variables. Rejected because the user explicitly needs PG-backed model key management and user-facing configuration.

### Built-in and Editable Prompt Templates

Add `ai_prompt_templates` with a stable builtin catalog and scoped overrides:

- `scope`: `builtin`, `org`, or `user`.
- `builtin_key`: stable key such as `system.default`, `analysis.anomaly`, `analysis.root_cause`, `alert.explain`, `query.generate`.
- `purpose`: `system`, `anomaly_analysis`, `root_cause`, `alert_explain`, `query_generation`.
- `body`: prompt template content.
- `variables_schema`: JSON schema for allowed render variables.
- `is_default`, `enabled`, `version`, `parent_id`, `created_by`, `updated_by`, timestamps.

Built-in prompts are inserted by migration or bootstrap with `scope = builtin` and stable `builtin_key`. They are read-only. When a user modifies a built-in prompt, the backend creates an org-scoped or user-scoped override linked by `parent_id` and increments version. Runtime resolution order is:

1. explicit `prompt_template_id` on the request,
2. user's enabled default for the purpose,
3. org enabled default for the purpose,
4. builtin default for the purpose.

The rendered prompt hash, template id, builtin key, and version are persisted with the chat message and audit event. Prompt rendering only accepts whitelisted variables from `variables_schema`.

Reasoning: this gives every org a working default from first start while preserving an audit trail for later customized behavior.

Alternative considered: ship default prompts as code constants only. Rejected because users need to inspect and modify prompts from the UI, and investigations must be reproducible by prompt version.

### Chat Orchestration

Extend `POST /api/v1/copilot/chat/sessions/{id}/messages` so each request may include:

- `content`
- `time_range`
- `analysis_mode`
- `stream_hints`
- optional `provider_id`
- optional `prompt_template_id`

The handler resolves provider and prompt config, records the user message, injects a rendered system prompt, then executes `ChatLoop::run_stream` with `RealToolDispatcher`. Tool arguments are validated by the backend and org/user identity always comes from `AuthContext`, not model-supplied arguments.

The default final answer shape is structured JSON plus markdown-friendly text:

- summary
- anomaly_points
- evidence
- likely_causes
- suggested_next_steps
- related_links
- confidence

Reasoning: the model explains and correlates; backend services remain the authority for data retrieval and tenant isolation.

### Tool Evidence and Limits

Each tool call records:

- tool name
- input time range and stream hints
- sanitized arguments
- status and latency
- row count / scanned rows / took_ms
- evidence summary
- object key for large raw results, if any

Large tool results are written to object storage under `ai-chat/{org_id}/{session_id}/tool-results/{message_id}/{tool_call_id}.json`. Chat history stores only a compact summary and object key.

Tool execution limits are enforced per request: max tool loops, max rows per tool, max bytes archived, and max time window if configured.

### Archive and Retention

Add archive metadata to chat persistence or a dedicated `ai_chat_archives` table:

- `session_id`
- `org_id`
- `object_key`
- `sha256`
- `bytes`
- `created_at_micros`
- `created_by`
- `status`
- `error`

Transcript objects are JSON documents containing session metadata, messages, prompt references, rendered prompt hashes, tool-call summaries, evidence object keys, token usage, and cost. Deleting a chat session soft-deletes metadata; retention cleanup removes expired archive objects through the existing compactor-style background path.

Reasoning: PG remains queryable while object storage carries large compliance artifacts.

### Audit Query

Extend the audit repository and API to support filtered cursor pagination:

- `from`, `to`
- `actor_kind`, `actor`
- `action`
- `target_kind`, `target_id`
- `limit`
- `cursor`

The frontend audit query page lives under `/settings/audit` because it is an Admin+ compliance/admin surface. It renders filters, a dense table, and a JSON detail drawer.

### Frontend Structure

Add:

- `/ai` as the main investigation chat route.
- `/settings/audit` for audit search.
- `/settings/ai_providers` for provider/key management.
- `/settings/ai_prompts` for prompt template management.

The chat page reuses product shell patterns and should resemble a focused work surface, not a marketing page: session history, starter actions, prompt suggestions, time-range controls, model/prompt selectors, streaming message transcript, and evidence panels.

## Risks / Trade-offs

- [Risk] Prompt edits can degrade model behavior. -> Mitigation: keep builtin prompts immutable, version every override, allow restoring from builtin, and persist prompt hashes with each message.
- [Risk] Model prompt injection could request cross-tenant data. -> Mitigation: tools ignore model-supplied org/user fields and derive identity only from `AuthContext`.
- [Risk] Audit payloads could accidentally include secrets or large raw data. -> Mitigation: audit helpers must redact provider keys and store only object keys/hashes for large results.
- [Risk] Long time ranges could create expensive tool calls. -> Mitigation: enforce backend row/time/byte/tool-loop budgets and require explicit user confirmation for future higher-cost flows.
- [Risk] Archive writes can fail after the user receives a response. -> Mitigation: record archive status separately, write audit failure events, and keep PG chat history intact.

## Migration Plan

1. Add migrations for provider, provider secret, prompt template, prompt default/version metadata, and archive metadata.
2. Seed builtin prompt templates in PG using stable `builtin_key` values.
3. Add repositories and wire them into `AppState`.
4. Add provider and prompt HTTP APIs.
5. Extend chat route to resolve provider/prompt config and record prompt/tool/evidence metadata.
6. Extend audit repository/API and add AI lifecycle audit event writers.
7. Add frontend settings pages and audit query page.
8. Add the AI chat page and SSE client.
9. Validate with unit, integration, OpenSpec, and frontend typecheck/lint tests.

Rollback is additive: disabling the AI routes hides the UI and leaves new tables unused. Existing chat/session tables remain readable. Provider secret rows are encrypted and can remain in place until a follow-up cleanup migration.

## Open Questions

- Should user-scoped prompt overrides be available to Editors, or only Admin-owned org prompts in the first release?
- What default max lookback should anomaly chat enforce for logs/metrics/traces?
- Should chat archive retention use global compactor retention or a dedicated AI archive retention setting?
