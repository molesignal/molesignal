## ADDED Requirements

### Requirement: Product Information Architecture

The web app SHALL organize authenticated navigation around user workflows rather than implementation parity. Top-level navigation SHALL expose Home, Observe, Data, Automate, and Admin groups, with each route assigned to exactly one group and one owner module.

#### Scenario: Navigation groups are stable
- **WHEN** the authenticated shell renders
- **THEN** the sidebar shows route groups for Home, Observe, Data, Automate, and Admin
- **AND** each visible route belongs to exactly one group
- **AND** route labels come from i18n keys, not hard-coded JSX strings

#### Scenario: Route ownership is inspectable
- **WHEN** a developer opens the route IA registry
- **THEN** each route entry declares path, group, label key, icon, required edition, required role, and default empty-state strategy

### Requirement: Persona-Aware Home

The Home route SHALL act as a product cockpit for the current org, showing operational health, activation progress, recently used resources, and next best actions based on data availability and edition.

#### Scenario: Empty OSS org gets activation path
- **WHEN** an OSS org has no streams, dashboards, alerts, or pipelines
- **THEN** Home renders activation tasks for sending logs, sending metrics, sending traces, creating a dashboard, and configuring an alert
- **AND** each task links to the specific route that completes it

#### Scenario: Active org gets operational summary
- **WHEN** an org has ingested data in the selected time window
- **THEN** Home renders KPI summaries for ingest volume, active alerts, query latency, and recent dashboards
- **AND** activation tasks move below the operational summary or collapse into a progress section

### Requirement: Workflow Entry Points

Every major workflow SHALL have a clear creation or continuation entry point from both its module landing page and the command palette.

#### Scenario: Create actions are discoverable
- **WHEN** a user opens Dashboards, Alerts, Pipelines, Functions, Reports, or Ingest
- **THEN** the page header exposes a primary action for the dominant creation workflow
- **AND** the same action is available through the command palette with matching label text

#### Scenario: Deep pages provide return path
- **WHEN** a user opens a detail or builder route
- **THEN** the page renders a breadcrumb or back affordance that returns to the owning module without losing query parameters needed for context

### Requirement: Product Page Quality Baseline

Every authenticated route SHALL satisfy a minimum product quality baseline: page title, one-sentence purpose or context, primary action when applicable, loading state, error state, empty state, and at least one useful next action.

#### Scenario: Backend-pending page is not blank
- **WHEN** a route depends on an endpoint that is not implemented
- **THEN** the page renders a backend-pending state naming the planned endpoint
- **AND** the state includes a useful fallback action such as docs, ingest setup, or returning to the module landing page

#### Scenario: Error state is actionable
- **WHEN** an API call fails with a non-auth error
- **THEN** the page shows an inline error with Retry
- **AND** raw exception objects, stack traces, and unlocalized error keys are not shown to end users
