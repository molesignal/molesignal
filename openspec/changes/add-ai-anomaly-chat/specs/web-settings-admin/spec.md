## MODIFIED Requirements

### Requirement: Settings Layout And Sub-Navigation

The web app SHALL expose `/settings` as a layout route that renders an internal SettingsSidebar plus an `<Outlet />` for sub-pages. The SettingsSidebar SHALL include the existing settings sections and add Admin-facing entries for audit query, AI model providers, and AI prompt templates. Navigating to `/settings` SHALL redirect to `/settings/general`.

#### Scenario: Direct deep link selects the right section

- **WHEN** the user opens `/settings/audit`
- **THEN** the SettingsLayout mounts the Audit sub-page
- **AND** the SettingsSidebar highlights "Audit" under SECURITY
- **AND** the browser title reflects the section

#### Scenario: Bare /settings redirects to general

- **WHEN** the user opens `/settings` with no trailing segment
- **THEN** the router replaces the URL with `/settings/general`

## ADDED Requirements

### Requirement: Audit Query Section

The page at `/settings/audit` SHALL let Admin+ users query audit history by time range, actor, action, target kind, and target id. It SHALL render a paginated table ordered newest first and a detail drawer for the selected audit event payload JSON. Viewer-role users SHALL be blocked by route guard or a permission-aware error state.

#### Scenario: Filtered audit query renders rows

- **WHEN** an Admin opens `/settings/audit`, selects a time range, and filters `action = ai.prompt.update`
- **THEN** the page calls `GET /api/v1/audit` with matching query params
- **AND** renders returned events in a table with timestamp, actor, action, target, and status metadata

#### Scenario: Audit detail drawer

- **WHEN** the user selects an audit row
- **THEN** a detail drawer opens with formatted payload JSON and copied identifiers

### Requirement: AI Model Providers Section

The page at `/settings/ai_providers` SHALL let Admin+ users list, create, edit, disable, delete, and rotate keys for AI model providers. API key fields SHALL use write-only controls; loaded rows SHALL show only masked key metadata.

#### Scenario: Create provider

- **WHEN** an Admin submits a provider form with provider type, base URL, default model, and API key
- **THEN** the page POSTs to `/api/v1/ai/providers`
- **AND** the saved row appears with masked key metadata but no plaintext key

#### Scenario: Rotate provider key

- **WHEN** an Admin rotates a provider key
- **THEN** the page POSTs to `/api/v1/ai/providers/{id}/rotate_key`
- **AND** the row refreshes after success

### Requirement: AI Prompt Templates Section

The page at `/settings/ai_prompts` SHALL let Admin+ users view built-in prompt templates, create org-scoped overrides, edit enabled org prompts, set defaults by purpose, disable overrides, and restore from built-in defaults. Built-in templates SHALL be visibly marked read-only. Users with user-prompt write permission MAY manage their own scoped prompts from the same page or a user-settings variant.

#### Scenario: Built-in prompts visible

- **WHEN** an Admin opens `/settings/ai_prompts` in a fresh org
- **THEN** the page lists built-in prompts for system, anomaly analysis, root-cause analysis, alert explanation, and query generation
- **AND** each built-in row is marked read-only

#### Scenario: Customize built-in prompt

- **WHEN** an Admin clicks Customize on a built-in root-cause prompt
- **THEN** the page opens an editor seeded with the built-in body
- **AND** saving creates an org-scoped override through `/api/v1/ai/prompts`

#### Scenario: Set default prompt by purpose

- **WHEN** an Admin sets an org prompt as the default for `anomaly_analysis`
- **THEN** the page calls `/api/v1/ai/prompts/{id}/set_default`
- **AND** subsequent anomaly chat requests use that prompt unless the request explicitly selects another prompt

### Requirement: AI Chat Route

The web app SHALL expose `/ai` as a first-class investigation route for AI anomaly chat. The route SHALL include starter cards, suggested prompts, time-range controls, provider/prompt selectors when permitted, session history, streaming transcript, and evidence panels linking back to logs, metrics, traces, alerts, and archives.

#### Scenario: Start anomaly chat

- **WHEN** a user opens `/ai`, selects a time range, and sends a question
- **THEN** the page creates or reuses a chat session
- **AND** streams assistant chunks and tool events into the transcript

#### Scenario: Evidence link navigates to source view

- **WHEN** an assistant response includes a log evidence link
- **THEN** clicking it navigates to the logs or investigation route with the relevant time range and stream context
