## ADDED Requirements

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
