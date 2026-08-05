## ADDED Requirements

### Requirement: First-Run Activation

The web app SHALL detect empty or new organizations and present a first-run activation path that guides users to ingest data, verify data arrival, create a dashboard, and configure an alert.

#### Scenario: New org sees first-run checklist
- **WHEN** an authenticated org has no streams and no dashboards
- **THEN** Home renders a first-run checklist with steps for sending logs, sending metrics, sending traces, creating a dashboard, and creating an alert
- **AND** each step includes status, estimated effort, and a route link

#### Scenario: Completed step is marked done
- **WHEN** the org has at least one stream with recent data
- **THEN** the "Send first signal" activation step is marked complete
- **AND** the checklist suggests the next incomplete step

### Requirement: Ingest Setup Wizard

The Ingest routes SHALL provide setup flows for common sources. Each source page SHALL show the org-specific endpoint, copyable configuration, validation instructions, and a test event action when supported.

#### Scenario: User copies source config
- **WHEN** a user opens an ingest source page
- **THEN** the page renders copyable config snippets for that source
- **AND** clicking Copy writes the exact snippet to the clipboard and shows a localized success toast

#### Scenario: Test event reports result
- **WHEN** the source supports test events and the user clicks Test
- **THEN** the page sends a test payload to the configured health endpoint
- **AND** the UI renders status, latency, and any validation error inline

### Requirement: Sample Data Path

The web app SHALL provide a sample data path for local OSS and trial SaaS users so they can evaluate logs, metrics, traces, dashboards, and alerts without external infrastructure.

#### Scenario: Sample data unavailable
- **WHEN** the backend does not expose sample data generation
- **THEN** the UI renders a disabled or backend-pending sample data action
- **AND** it provides manual ingest instructions instead

#### Scenario: Sample data succeeds
- **WHEN** sample data generation is available and succeeds
- **THEN** the UI navigates to a prebuilt dashboard or logs query showing the generated data
- **AND** the activation checklist updates without a full page reload

### Requirement: Contextual Next Steps

Each major empty state SHALL offer contextual next steps based on route and edition.

#### Scenario: Empty dashboards suggest creation
- **WHEN** the user opens Dashboards and no dashboards exist
- **THEN** the empty state offers Create dashboard, Import dashboard, and Use sample data actions

#### Scenario: Empty alerts suggest prerequisite
- **WHEN** the user opens Alerts and no streams exist
- **THEN** the empty state suggests sending data before creating alert rules
