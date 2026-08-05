## MODIFIED Requirements

### Requirement: Minimal Application Chrome

The web app SHALL render a single product cockpit shell composed of a persistent topbar, grouped sidebar navigation, bottom runtime status bar, and main content area. The shell SHALL show current org, region/cluster context, global command search, settings, user menu, and route navigation without obscuring page content.

#### Scenario: Topbar shows fixed product context
- **WHEN** the app boots with an authenticated user
- **THEN** the topbar shows product mark, sidebar toggle, org switcher, environment/cluster chip, command palette trigger, NOC/fullscreen action, notification/help affordances, settings, and current user menu
- **AND** org switching is available from the topbar

#### Scenario: Sidebar groups product routes
- **WHEN** any authenticated route is active
- **THEN** the sidebar shows grouped navigation entries for Home, Observe, Data, Automate, and Admin
- **AND** the active route is visibly highlighted

#### Scenario: Main content is not hidden
- **WHEN** the topbar, sidebar, or status bar is fixed
- **THEN** the main content area has matching padding or layout offsets
- **AND** no page header, toolbar, or first row is hidden under fixed chrome

### Requirement: Route Map

The shell SHALL register and own the authenticated route map for Home, Observe, Data, Automate, RUM, IAM, Settings, and legacy deep links. All deep links SHALL re-hydrate org, global time window, pinned anchor, and investigation stack from URL/session state before rendering data.

#### Scenario: Default authenticated landing
- **WHEN** a user logs in without a `?next=` URL
- **THEN** the router redirects to `/home`
- **AND** the global time window defaults to the last 1 hour

#### Scenario: URL hydration before render
- **WHEN** a user opens a URL containing `?time=...&anchor=...&stack=...`
- **THEN** the app parses those parameters and updates the time and stack stores synchronously before mounting the main view
- **AND** no flash of an unrelated default state is visible

#### Scenario: Legacy paths remain reachable
- **WHEN** a user opens an existing deep link covered by `docs/web/sitemap-diff.md`
- **THEN** the router mounts the corresponding page or redirects to its replacement route
- **AND** the user is not sent to a generic placeholder unless the feature is explicitly backend-pending

### Requirement: Topbar Settings Dropdown

The shell SHALL include a Settings trigger in the topbar near the user menu; the dropdown SHALL contain sections for Theme, Palette, Density, and Language, surfacing every option per section as a checkable item. Existing scattered toggles SHALL keep working but are no longer the primary affordance.

#### Scenario: Gear opens the unified settings dropdown
- **WHEN** the user clicks the settings icon in the topbar
- **THEN** a dropdown opens listing Theme, Palette, Density, and Language sections
- **AND** each section's active option carries a leading checkmark

#### Scenario: Legacy theme toggle still works
- **WHEN** the user clicks the sun/moon icon
- **THEN** theme flips between dark and light
- **AND** the settings dropdown's Theme section reflects the new value

## ADDED Requirements

### Requirement: Breadcrumbs For Deep Product Routes

The shell/page template system SHALL provide breadcrumbs or back affordances for routes deeper than one product level, including detail, edit, import, inspector, and builder routes.

#### Scenario: Dashboard edit has breadcrumb
- **WHEN** the user opens `/dashboards/:id/edit`
- **THEN** the page shows a breadcrumb or back action to Dashboards and the dashboard detail route
- **AND** using the back action preserves the current org and time context

### Requirement: Responsive Shell Behavior

The shell SHALL define responsive navigation behavior for desktop, tablet, and mobile widths. Desktop keeps persistent sidebar; smaller widths collapse navigation behind an accessible trigger.

#### Scenario: Mobile shell remains navigable
- **WHEN** the viewport width is 375px
- **THEN** the sidebar is collapsed behind a menu trigger
- **AND** the org switcher, command palette, settings menu, and user menu remain reachable by keyboard
