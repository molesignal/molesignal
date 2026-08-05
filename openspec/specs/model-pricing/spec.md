# Model Pricing Capability ()

## Purpose

Copilot 模型成本表 + token 计费 + 与 `quotas` 配额对账。 特性，伴随 `copilot-chat` / `copilot-mcp` 出现。

## Requirements

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

### Requirement: Model Prices HTTP CRUD

The system SHALL expose `GET /api/v1/model_prices`, `POST /api/v1/model_prices` (upsert), and `DELETE /api/v1/model_prices/{provider}/{model}` backed by the existing `ModelPriceRepository`. All routes require `OrgAdmin+`. The endpoint completes the surface the `model-pricing` capability promised; the underlying `model_prices` table already exists.

#### Scenario: List returns all rows

- **WHEN** an Admin GETs `/api/v1/model_prices`
- **THEN** the response is a JSON array of every model price row sorted by `(provider, model)`

#### Scenario: Upsert overwrites existing

- **WHEN** an Admin POSTs `{ provider: "openai", model: "gpt-4o", input_per_million_usd: 6.0, output_per_million_usd: 18.0 }` for a row that already exists
- **THEN** the response is the updated row with `updated_at_micros = <now>`
- **AND** subsequent list reflects the new prices

#### Scenario: Delete by composite key

- **WHEN** an Admin DELETEs `/api/v1/model_prices/openai/gpt-4o`
- **THEN** the response is `{ "deleted": true }` and the row is gone from list output
