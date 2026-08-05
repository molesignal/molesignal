## ADDED Requirements

### Requirement: LLM Trace Stream and Fan-Out

When OTLP traces (any receiver) carry spans with attribute keys matching `gen_ai.*` (per OpenTelemetry GenAI semantic conventions), the system SHALL fan out a normalized copy of each such span to a derived stream `llm_traces` for the same org, projecting columns `{ _timestamp, trace_id, span_id, model, provider, prompt_tokens, completion_tokens, total_tokens, cost_usd, latency_ms, status, prompt_redacted, completion_redacted, user_id, session_id }`. The original `traces` stream copy is NOT replaced; both coexist.

#### Scenario: GenAI span auto-derived
- **WHEN** an OTLP traces ingest contains a span with `gen_ai.system = "openai", gen_ai.request.model = "gpt-4o", gen_ai.usage.prompt_tokens = 120, gen_ai.usage.completion_tokens = 200`
- **THEN** one row appears in both `traces` and `llm_traces` for that org; `llm_traces` row has the normalized columns populated

#### Scenario: Non-LLM span not fanned out
- **WHEN** a span has no `gen_ai.*` attribute
- **THEN** only the `traces` stream receives a row; `llm_traces` is unchanged

### Requirement: PII Redaction Default For Prompt/Completion

`llm_traces` ingest SHALL run `gen_ai.prompt` and `gen_ai.completion` (or `gen_ai.completion[*].content`) through a default VRL function `redact_pii` (built-in) before storing them as `prompt_redacted` / `completion_redacted`. Orgs MAY override the function via `[llm].redact_function_id = <function_id>`.

#### Scenario: Default redaction strips email-like patterns
- **WHEN** a prompt contains `"please email me at john@example.com"`
- **THEN** the stored `prompt_redacted` reads `"please email me at <REDACTED_EMAIL>"`

#### Scenario: Org override applied
- **WHEN** an org sets `[llm].redact_function_id = <id>` to a custom VRL function
- **THEN** that function replaces the default; failures fall back to the default and log a warning

### Requirement: LLM Stats Endpoints

The system SHALL expose `GET /api/v1/llm/stats`, `/top_models`, and `/top_users` derived queries that aggregate `llm_traces` over an `?from=&to=` window, returning total tokens, total cost, and per-dimension top-N.

#### Scenario: Stats over last hour
- **WHEN** a user GETs `/api/v1/llm/stats?from=now-1h&to=now`
- **THEN** the response includes `{ total_prompt_tokens, total_completion_tokens, total_cost_usd, request_count, error_count }`
