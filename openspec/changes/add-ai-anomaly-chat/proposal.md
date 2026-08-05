## Why

MoleSignal has the backend primitives for Copilot chat, MCP tools, audit events, model pricing, Postgres metadata, and object-store backed blobs, but they are not yet composed into a usable AI investigation workflow. Users need an AI chat page that can analyze a selected time range, call trusted backend tools to inspect logs, metrics, traces, and alerts, preserve conversation history, archive evidence, and expose all model/prompt configuration through auditable UI surfaces.

## What Changes

- Add an AI anomaly/root-cause chat experience that streams model responses and uses backend-owned tools for data access instead of letting the model directly access tenant data.
- Add Postgres-backed model provider configuration with encrypted API keys, provider metadata, default model selection, and key rotation audit events.
- Add Postgres-backed prompt templates with built-in default prompts for system behavior, anomaly analysis, root-cause analysis, alert explanation, and query generation; users can modify or fork prompts according to role.
- Extend chat persistence so sessions, messages, prompt versions, tool calls, evidence summaries, and object-store archive keys are retained for audit and review.
- Store full transcripts and large tool results in the configured object store under an AI archive prefix, while keeping searchable metadata in Postgres.
- Add an audit query page and expand the audit API beyond "recent events" so Admin+ users can search by time range, actor, action, target, and cursor.
- Add settings UI for model providers and AI prompt templates, plus a first-class AI chat route in the investigation area.

## Capabilities

### New Capabilities

- `ai-anomaly-chat`: AI model provider configuration, built-in and editable prompt templates, anomaly/root-cause chat orchestration, tool evidence capture, and object-store chat archive.

### Modified Capabilities

- `audit`: Audit APIs SHALL support searchable, paginated audit history and SHALL record AI provider, prompt, chat, tool-call, and archive lifecycle events.
- `copilot-chat`: Chat sessions SHALL use configured providers/prompts from Postgres, call the real backend tool dispatcher, persist prompt/tool/evidence metadata, and avoid direct model access to tenant data.
- `web-settings-admin`: Settings/admin UI SHALL expose audit query, AI provider configuration, and AI prompt template management pages.

## Impact

- Affected backend code: `crates/api/src/http/routes/copilot_chat.rs`, `copilot_mcp_dispatcher.rs`, `audit.rs`, new AI provider/prompt routes, `AppState`, bootstrap wire, migrations, persistence repositories, and object-store archive helpers.
- Affected frontend code: new AI chat route, audit query route, AI provider and prompt settings routes, API clients, navigation metadata, command palette entries, i18n, and route tests.
- Affected storage: new Postgres tables for AI providers, encrypted provider secrets, prompt templates, prompt versions/defaults, and archive metadata; object-store keys under an AI chat archive prefix.
- Security impact: API keys must be encrypted with `CipherRootKey`; model-visible context must be constrained by backend tool schemas; audit payloads must exclude plaintext secrets and raw oversized data.
