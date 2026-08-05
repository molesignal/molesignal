# Copilot Telemetry Capability

## Purpose

OpenTelemetry GenAI 语义约定 span 的归一化派生流（`copilot_traces`）、默认 PII 脱敏、Copilot 统计端点。付费特性，受 `license.has_feature("copilot")` 闸门保护；实现位于 `/crates/copilot/`。

## Requirements

### Requirement: Copilot Trace Stream and Fan-Out

When OTLP traces (any receiver) carry spans with attribute keys matching `gen_ai.*` (per OpenTelemetry GenAI semantic conventions), the system SHALL fan out a normalized copy of each such span to a derived stream `copilot_traces` for the same org, projecting columns `{ _timestamp, trace_id, span_id, model, provider, prompt_tokens, completion_tokens, total_tokens, cost_usd, latency_ms, status, prompt_redacted, completion_redacted, user_id, session_id }`. The original `traces` stream copy is NOT replaced; both coexist. The fan-out path SHALL be gated by `license.has_feature("copilot")`; community license skips fan-out silently.

#### Scenario: GenAI span auto-derived
- **WHEN** an OTLP traces ingest contains a span with `gen_ai.system = "openai", gen_ai.request.model = "gpt-4o", gen_ai.usage.prompt_tokens = 120, gen_ai.usage.completion_tokens = 200`
- **THEN** one row appears in both `traces` and `copilot_traces` for that org; `copilot_traces` row has the normalized columns populated

#### Scenario: Non-Copilot span not fanned out
- **WHEN** a span has no `gen_ai.*` attribute
- **THEN** only the `traces` stream receives a row; `copilot_traces` is unchanged

### Requirement: PII Redaction Default For Prompt/Completion

`copilot_traces` ingest SHALL run `gen_ai.prompt` and `gen_ai.completion` (or `gen_ai.completion[*].content`) through a default VRL function `redact_pii` (built-in) before storing them as `prompt_redacted` / `completion_redacted`. Orgs MAY override the function via `[copilot].redact_function_id = <function_id>`.

#### Scenario: Default redaction strips email-like patterns
- **WHEN** a prompt contains `"please email me at john@example.com"`
- **THEN** the stored `prompt_redacted` reads `"please email me at <REDACTED_EMAIL>"`

#### Scenario: Org override applied
- **WHEN** an org sets `[copilot].redact_function_id = <id>` to a custom VRL function
- **THEN** that function replaces the default; failures fall back to the default and log a warning

### Requirement: Copilot Stats Endpoints

The system SHALL expose `GET /api/v1/copilot/stats`, `/top_models`, and `/top_users` derived queries that aggregate `copilot_traces` over an `?from=&to=` window, returning total tokens, total cost, and per-dimension top-N. The endpoints SHALL return `403 Forbidden` when `license.has_feature("copilot")` is false.

#### Scenario: Stats over last hour
- **WHEN** a user GETs `/api/v1/copilot/stats?from=now-1h&to=now`
- **THEN** the response includes `{ total_prompt_tokens, total_completion_tokens, total_cost_usd, request_count, error_count }`

### Requirement: AI Toolset Registry CRUD

The system SHALL expose `GET /api/v1/ai_toolsets`, `POST /api/v1/ai_toolsets`, and `DELETE /api/v1/ai_toolsets/{id}` backed by an `AiToolsetRepository` trait. The trait has two impls:

- `EmptyAiToolsetRepository` (OSS default) — `list` returns `Ok(vec![])`; `create` / `delete` return `Err::forbidden("ai_toolsets requires  license")`.
- `PgAiToolsetRepository` ( crate `/crates/ai_toolsets/`) — backed by an `ai_toolsets` Postgres table with `(id TEXT PK, org_id TEXT, name TEXT, schema JSONB, enabled BOOL, created_at_micros, updated_at_micros, UNIQUE(org_id, name))`.

All routes require `OrgAdmin+`. The OSS `GET` succeeds with an empty list so the Settings UI can render "no toolsets" without 403'ing.

#### Scenario: OSS GET returns empty list

- **WHEN** the OSS build is running and an OrgAdmin GETs `/api/v1/ai_toolsets`
- **THEN** the response is `200 OK` with `[]`

#### Scenario: OSS write rejected

- **WHEN** an OrgAdmin POSTs to `/api/v1/ai_toolsets` on the OSS build
- **THEN** the response is `403 Forbidden` with `{ "error": "ai_toolsets requires  license" }`

#### Scenario:  create persists

- **WHEN** the  build is running and an Admin POSTs `{ name: "logs.search", schema: {...}, enabled: true }`
- **THEN** the row persists in `ai_toolsets` and is returned by subsequent GETs
