## ADDED Requirements

### Requirement: Model pricing catalog

The system SHALL maintain a `model_prices` table with `{ provider, model, input_per_million_usd, output_per_million_usd, effective_at }`. An Admin+ HTTP CRUD `/api/v1/model_prices` SHALL allow management. A default catalog ships with current OpenAI / Anthropic prices on first migration.

#### Scenario: Catalog seeded on migration

- **WHEN** the migration `add_model_prices` runs against an empty table
- **THEN** at least `gpt-4o / claude-3.5-sonnet / gpt-4o-mini / claude-3-haiku` rows are inserted with their published per-million prices

### Requirement: Per-request cost attribution

Every Copilot LLM call SHALL be costed at request time using the active price (latest `effective_at <= now`) and the result persisted to `copilot_traces.cost_usd`. Org-level rollup SHALL be queryable via the existing `/api/v1/copilot/stats` endpoint.

#### Scenario: Cost computed correctly

- **WHEN** a call uses model `gpt-4o` with `prompt_tokens=1000, completion_tokens=500`
- **AND** active prices are `input_per_million_usd=5.0, output_per_million_usd=15.0`
- **THEN** `cost_usd = (1000 * 5.0 + 500 * 15.0) / 1_000_000 = 0.0125`
