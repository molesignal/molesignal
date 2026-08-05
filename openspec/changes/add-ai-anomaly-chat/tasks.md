## 1. Persistence And Migrations

- [x] 1.1 Add migrations for `ai_model_providers`, encrypted provider secret storage, `ai_prompt_templates`, prompt defaults/version metadata, chat evidence metadata, and AI archive metadata.
- [x] 1.2 Seed built-in prompt templates in Postgres with stable builtin keys: `system.default`, `analysis.anomaly`, `analysis.root_cause`, `alert.explain`, and `query.generate`.
- [x] 1.3 Extend chat session/message schema for provider id, analysis mode, time range, prompt template id, prompt builtin key, prompt version, prompt hash, evidence metadata, archive object key, and soft-delete timestamp.
- [x] 1.4 Add repository tests for provider CRUD, encrypted key round-trip, prompt resolution order, prompt override versioning, archive metadata, and soft-delete behavior.

## 2. Backend AI Provider And Prompt APIs

- [x] 2.1 Implement AI provider repository and Admin+ HTTP routes for list, create, update, disable/delete, and rotate key.
- [x] 2.2 Implement provider adapter construction from PG provider rows and encrypted secrets, preserving optional env fallback only for local development if configured.
- [x] 2.3 Implement AI prompt repository and HTTP routes for list, create override, update override, set default, disable/delete override, and restore from builtin.
- [x] 2.4 Validate prompt template variables against `variables_schema` and reject unknown render variables before persistence.
- [x] 2.5 Record audit events for provider create/update/delete/rotate and prompt create/update/set_default/delete without secret or oversized prompt leakage.

## 3. Chat Runtime And Archive

- [x] 3.1 Replace `NoopToolDispatcher` in chat message handling with `RealToolDispatcher` and enforce auth-context-derived org/user identity for every tool call.
- [x] 3.2 Extend message requests to accept time range, analysis mode, stream hints, provider id, and prompt template id.
- [x] 3.3 Resolve active prompt by explicit id, user default, org default, then builtin default; render allowed variables and persist prompt id/version/builtin key/hash.
- [x] 3.4 Persist tool-call evidence summaries and spill large raw tool results to object storage under the AI chat prefix.
- [x] 3.5 Implement transcript archive writing with object key, checksum, byte size, status, and audit event recording.
- [x] 3.6 Convert chat deletion to soft-delete and keep archived metadata available to audit/retention paths.
- [x] 3.7 Add integration tests for SSE happy path, provider error path, prompt metadata persistence, tool call evidence, archive failure, and cross-org tool isolation.

## 4. Audit Query Backend

- [x] 4.1 Extend `AuditEventRepository` with filtered cursor pagination by time range, actor kind, actor, action, target kind, target id, limit, and cursor.
- [x] 4.2 Update `GET /api/v1/audit` to use Admin+/Owner audit permission and return `{ items, next_cursor }`.
- [x] 4.3 Add audit tests for filter correctness, cursor stability, Viewer rejection, and AI lifecycle redaction rules.

## 5. Frontend Admin Pages

- [x] 5.1 Add web API clients for audit query, AI providers, and AI prompt templates.
- [x] 5.2 Add `/settings/audit` with filters, cursor pagination, dense event table, and JSON detail drawer.
- [x] 5.3 Add `/settings/ai_providers` with provider list, create/edit drawer, write-only API key field, disabled state, delete, and rotate key action.
- [x] 5.4 Add `/settings/ai_prompts` with built-in prompt visibility, read-only badges, customize flow, override editor, default-by-purpose controls, and restore from builtin.
- [x] 5.5 Update settings navigation, product IA, command palette, route guards, and en/zh-CN i18n for the new admin pages.

## 6. Frontend AI Chat Page

- [x] 6.1 Add `/ai` route with session history, starter cards, suggested prompts, time-range controls, provider/prompt selectors, and streaming transcript.
- [x] 6.2 Implement SSE client handling for chunk, tool_start, tool_end, done, and error events.
- [x] 6.3 Render structured anomaly answers with summary, anomaly points, evidence, likely causes, suggested next steps, related links, and confidence.
- [x] 6.4 Link evidence rows to logs, metrics, traces, alerts, archive objects, or investigation routes with preserved time range and stream context.
- [x] 6.5 Add loading, empty, error, permission denied, unlicensed, and mobile responsive states.

## 7. Verification

- [x] 7.1 Run `openspec validate add-ai-anomaly-chat --type change --strict`.
- [x] 7.2 Run targeted Rust tests for AI provider/prompt repositories, audit query repository, chat SSE/tool/archive flow, and secret redaction.
- [x] 7.3 Run frontend typecheck and lint for touched web files.
- [x] 7.4 Run targeted Playwright/a11y coverage for `/ai`, `/settings/audit`, `/settings/ai_providers`, and `/settings/ai_prompts`.
