## ADDED Requirements

### Requirement: Functions And Enrichment Top-Level Routes

The web app SHALL expose `/functions` and `/enrichment-tables` as top-level routes (not nested under `/pipelines` or `/settings`). They appear in the DATA Sidebar group alongside Streams / Pipelines / Reports.

#### Scenario: Functions reachable directly

- **WHEN** the user pastes the URL `/functions` into the address bar
- **THEN** the page loads the function library without requiring navigation via Pipelines
- **AND** the Sidebar highlights the Functions entry

### Requirement: Pipeline Sub-Route Set

The web app SHALL expose the following pipeline sub-routes (in addition to the existing `/pipelines` list): `/pipelines/add`, `/pipelines/import`, `/pipelines/:id/edit`, `/pipelines/:id/history`, `/pipelines/:id/backfill`. The list page links to each sub-route from per-row action menus.

#### Scenario: Action menu surfaces history + backfill

- **WHEN** the user opens the row action menu on the pipelines list
- **THEN** the menu contains "Edit", "History", "Backfill", "Export", "Delete"
- **AND** "Backfill" / "History" link to the corresponding sub-routes
