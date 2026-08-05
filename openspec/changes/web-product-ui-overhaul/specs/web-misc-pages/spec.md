## ADDED Requirements

### Requirement: Standalone Route Production Quality

Standalone routes covered by `web-misc-pages` SHALL be upgraded from route coverage to production page quality using shared templates, localized copy, route-specific actions, and consistent query states.

#### Scenario: Inspector page has product framing
- **WHEN** the user opens `/logs/inspector`
- **THEN** the page renders a localized title, description, search-job context, and back action to Logs
- **AND** missing `id` state explains how to reach the inspector from a search job

#### Scenario: Import page validates inline
- **WHEN** the user opens `/dashboards/import`
- **THEN** malformed input errors appear inline near the input
- **AND** the primary Import action remains disabled until input is parseable

### Requirement: Deep-Link Preservation

Standalone pages SHALL preserve meaningful context such as org, time range, stream, trace id, dashboard id, and originating route when navigating between related routes.

#### Scenario: Trace detail links to logs with context
- **WHEN** the user clicks Search around from `/traces/:id`
- **THEN** the app opens Logs with the trace id and time window encoded in the URL

#### Scenario: Stream explore opens query route
- **WHEN** the user clicks Query in Logs from `/streams/:id`
- **THEN** the app opens Logs with the selected stream encoded in the URL

### Requirement: Backend Gap Transparency

When a standalone route exists before its backend endpoint is complete, the page SHALL be explicit about the missing backend contract and still provide useful local context or documentation.

#### Scenario: Pending endpoint named
- **WHEN** a route cannot load because its backend endpoint is not implemented
- **THEN** the page names the endpoint or capability it is waiting for
- **AND** the page does not render a generic blank table
