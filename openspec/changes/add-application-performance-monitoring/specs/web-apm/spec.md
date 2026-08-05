## ADDED Requirements

### Requirement: APM Route And In-Page Navigation

The web app SHALL expose `/apm/overview`, `/apm/services`, `/apm/services/:service`, `/apm/transactions`, `/apm/dependencies`, `/apm/errors`, `/apm/errors/:fingerprint` and `/apm/deployments`. `/apm` SHALL redirect to `/apm/overview`. The APM shell SHALL provide Overview, Services, Transactions, Traces, Dependencies, Errors and Deployments navigation that preserves the global time window and applicable service/environment/version filters. Traces SHALL continue to use the canonical shared Trace Explorer.

#### Scenario: APM entry opens overview
- **WHEN** the user selects APM from the Sidebar
- **THEN** the app opens `/apm/overview`
- **AND** Overview is selected in the APM sub-navigation

#### Scenario: Filters survive APM navigation
- **WHEN** the user selects service `checkout`, environment `prod` and version `2.5.0`, then opens Transactions
- **THEN** the destination URL and API request preserve those filters and the global time range

### Requirement: APM Overview

The APM overview SHALL show organization-wide request rate, error rate and P95 latency, service health counts, highest-impact services, top error groups, dependency regressions and recently observed versions for the selected time/environment scope. Every summary SHALL link to a filtered APM detail view.

#### Scenario: Degraded service drills down
- **WHEN** the overview identifies `checkout` as degraded
- **THEN** selecting it opens `/apm/services/checkout` with the same time and environment filters

#### Scenario: No APM facts shows activation guidance
- **WHEN** the selected organization has no APM service facts
- **THEN** the overview displays an activation empty state
- **AND** links to the existing OpenTelemetry data-source instructions rather than showing fabricated KPIs

### Requirement: APM Service Catalog

The service catalog SHALL list services independently of Service Graph edges and display environment, recent versions, last seen, request rate, error rate, P95 and instrumentation health. It SHALL support search, environment/version filters and sorting by health or RED metrics.

#### Scenario: Standalone service is listed
- **WHEN** the API returns a service with request metrics but no dependencies
- **THEN** the service appears in `/apm/services`
- **AND** its dependency count is zero rather than the service being omitted

### Requirement: APM Service Workbench

The service detail page SHALL present a consistent service/environment/version scope and provide Overview, Transactions, Errors, Dependencies and Versions sections. Overview SHALL include RED trends, recent representative Traces and cross-links to related Logs, Metrics and Profiles.

#### Scenario: Service scope applies to every section
- **WHEN** the user opens `checkout` in `prod` for version `2.5.0`
- **THEN** every service section queries that same identity, environment, version and time range
- **AND** links to signal explorers carry equivalent filters

#### Scenario: Partial data is not hidden
- **WHEN** the service API reports projection gaps in the selected window
- **THEN** the service workbench displays a non-blocking partial-data notice
- **AND** identifies the last complete bucket

### Requirement: Transaction Explorer

The Transaction explorer SHALL display grouped operation name, service, throughput, error rate, P50/P95/P99 and total time. Users SHALL be able to sort/filter the table and open Trace search scoped to the selected Transaction and time range.

#### Scenario: Slow Transaction opens traces
- **WHEN** the user selects a slow `POST /checkout` Transaction and chooses “查看 Traces”
- **THEN** `/traces` opens with service and operation filters plus the originating time range

### Requirement: Dependency Explorer

The Dependency explorer SHALL group service, database, cache, messaging and external HTTP/RPC dependencies, showing calling service, target, request rate, error rate and latency percentiles. It SHALL support table and topology-oriented views without representing raw URL or SQL values.

#### Scenario: Database dependency drills into caller traces
- **WHEN** the user selects the PostgreSQL `orders` dependency used by `checkout`
- **THEN** the UI offers filtered Transactions/Traces for `checkout` and that dependency context
- **AND** no raw SQL parameters are rendered

### Requirement: Backend Error Explorer

The backend error list SHALL show fingerprinted issues with representative type/message, affected services, Transactions and versions, first/last seen, occurrence trend and sample count. Error detail SHALL show a sanitized representative stack and only render Trace links that the API marks available.

#### Scenario: Error detail connects evidence
- **WHEN** an error group has retained representative Traces
- **THEN** the detail page links to those Trace views and related Logs
- **AND** preserves service, environment and time context

### Requirement: Deployment And Version Comparison Experience

The Deployments page SHALL expose contextual version comparison for two versions of the same service/environment using throughput, error rate, P50/P95/P99 and top regressed Transactions/errors. It SHALL show sample counts and honor the API's insufficient-data state. Version comparison SHALL NOT appear as a standalone first-level navigation label.

#### Scenario: Insufficient sample remains neutral
- **WHEN** either selected version is marked insufficient data
- **THEN** the page shows the available counts and metrics
- **AND** does not apply improved/regressed semantic status

### Requirement: RUM As An Independent User Experience Product

RUM overview, applications, sessions, pages, frontend errors, performance and session replay SHALL appear under the canonical `/rum/*` hierarchy with RUM-only navigation. Source Maps and other configuration SHALL appear under `/rum/settings/*`, outside the analysis tabs. Existing `/apm/user-experience/*` URLs SHALL redirect to the equivalent `/rum/*` route while preserving suffix, path parameters, query string and fragment. Backend RUM API paths, SDK instructions and `rum_*` streams SHALL remain unchanged.

#### Scenario: Existing RUM deep link survives
- **WHEN** a user opens `/apm/user-experience/sessions/view/s-123?time=-2h..now`
- **THEN** the same session view is rendered under the canonical RUM context
- **AND** session ID and time query are preserved

#### Scenario: Empty RUM overview guides activation
- **WHEN** the selected scope contains no RUM sessions
- **THEN** the overview explains how to install and configure the Web, Android, iOS or Flutter SDK
- **AND** shows a four-step activation path and links to test-event and setup documentation actions

#### Scenario: RUM links to backend evidence
- **WHEN** a RUM session or browser API event carries a related Trace ID
- **THEN** the user can open the corresponding Trace while retaining the session time context

### Requirement: APM Cross-Signal Navigation

APM pages SHALL reuse existing SignalReference and investigation-stack conventions to link to Traces, Logs, Metrics, Profiles and RUM without duplicating those explorers. Links SHALL carry organization scope, time range and stable low-cardinality filters.

#### Scenario: Service opens profile comparison
- **WHEN** a service/version regression has related profiles
- **THEN** the user can open `/profiles` or `/profiles/compare` prefiltered to the service and relevant windows

### Requirement: APM Page State And Accessibility

Every APM page SHALL implement loading, empty, permission-denied, error, stale and partial-data states using the shared product-state primitives. New routes SHALL be included in i18n length, keyboard navigation and axe critical-violation coverage, and SHALL comply with the repository-wide focus styling rules.

#### Scenario: APM routes pass accessibility gate
- **WHEN** the APM route set is exercised in dark and light themes
- **THEN** axe reports zero critical violations
- **AND** keyboard focus uses background, text or icon feedback without ring, outline or focus box-shadow styles
