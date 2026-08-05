## ADDED Requirements

### Requirement: Sidebar Misc-Pages Entries

The Sidebar SHALL expose top-level entries for `Actions` (under DATA PLANE) and `Service graph` (under OBSERVE), each linking to its route under `web-misc-pages`. The Sidebar SHALL NOT add entries for trace detail / stream explore / dashboard import / new-panel / short-url — those are reachable from in-page navigation, not Sidebar.

#### Scenario: Sidebar lists new top-level entries

- **WHEN** the Sidebar is open
- **THEN** a `Service graph` entry appears in the OBSERVE group
- **AND** an `Actions` entry appears in the DATA PLANE group
- **AND** no Sidebar entries are added for trace / stream / dashboard subroutes

### Requirement: Alerts Sub-Nav Adds History And Insights

The `/alerts` shell SHALL render an in-page sub-nav with tabs for `Rules` (existing), `History` (`/alerts/history`), and `Insights` (`/alerts/insights`). The active tab matches the current path.

#### Scenario: Sub-nav highlights active tab

- **WHEN** the user opens `/alerts/history`
- **THEN** the alerts sub-nav highlights `History`

### Requirement: Route Table Adds Misc Routes

The router SHALL register all 14 routes introduced by `web-misc-pages` (`/logs/inspector`, `/metrics/promql-builder`, `/traces/:id`, `/traces/session/:id`, `/streams/:id`, `/service-graph`, `/dashboards/import`, `/dashboards/:id/panels/new`, `/alerts/history`, `/alerts/insights`, `/actions`, `/short/:code`, `/ingest/:category/:source` placeholder replacement). The `a11y-routes.spec.ts` array SHALL include each new route so axe `critical=0` is enforced.

#### Scenario: New routes are reachable

- **WHEN** the user types any of the 14 new routes in the address bar
- **THEN** the router mounts the corresponding page from `web-misc-pages`
- **AND** Playwright's a11y-routes spec covers the route
