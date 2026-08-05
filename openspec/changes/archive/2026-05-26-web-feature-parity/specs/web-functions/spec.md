## ADDED Requirements

### Requirement: VRL Function Library

The web app SHALL expose `/functions` listing reusable Vector Remap Language (VRL) functions from `GET /api/v1/functions`, and an inline editor for create / edit. Each function carries `name`, `body` (VRL source), `params` (signature), and `description`.

#### Scenario: List functions with test runner

- **WHEN** the user opens `/functions`
- **THEN** the page renders a list of every function with name + arity + last-modified
- **AND** a "+ New function" button opens the editor in create mode

#### Scenario: Edit and dry-run

- **WHEN** the user opens `/functions/<name>` and edits the VRL body
- **AND** clicks "Run on sample"
- **THEN** the right pane runs the function against a sample event and shows input → output diff
- **AND** "Save" persists via `POST /api/v1/functions` or `PUT /api/v1/functions/<name>`

### Requirement: Enrichment Tables

The web app SHALL expose `/enrichment-tables` for managing lookup tables (CSV / TSV) that pipelines can join against during transform. Each row carries `name`, `column_count`, `row_count`, `last_loaded_at`. Upload accepts CSV / TSV with auto-detected schema.

#### Scenario: Upload CSV by URL or paste

- **WHEN** the user opens `/enrichment-tables/new` and pastes a CSV blob
- **THEN** the page issues `POST /api/v1/enrichment_tables` with the table name + body
- **AND** the schema preview shows detected columns + types before save
