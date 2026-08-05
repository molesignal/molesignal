## ADDED Requirements

### Requirement: Shared Page Templates

The frontend SHALL provide shared templates for OverviewPage, ListPage, DetailPage, BuilderPage, SettingsPage, and GatePage. New or migrated routes SHALL use one of these templates unless a documented exception exists.

#### Scenario: List page uses standard anatomy
- **WHEN** a route renders a list of resources
- **THEN** it uses the ListPage anatomy: header, primary action, optional filters, table or grid body, bulk action area, loading state, empty state, and error state

#### Scenario: Detail page uses standard anatomy
- **WHEN** a route renders a single resource detail
- **THEN** it uses the DetailPage anatomy: breadcrumb/back action, summary header, metadata strip, content tabs or sections, and related-resource actions

### Requirement: Shared State Components

The design system SHALL expose shared components for loading, empty, error, backend-pending, permission-denied, and license-gated states. These components SHALL support title, description, icon, primary action, secondary action, and i18n keys.

#### Scenario: Empty state has a next action
- **WHEN** a list route has zero rows
- **THEN** the empty state includes a localized title and description
- **AND** it includes at least one next action unless the user lacks permission

#### Scenario: Permission state avoids raw 403
- **WHEN** an API call fails because the user lacks permission
- **THEN** the page renders PermissionDeniedState
- **AND** the visible copy explains the missing role or permission without displaying the raw 403 payload

### Requirement: Data-Dense Visual System

Authenticated product UI SHALL use a quiet, data-dense visual style optimized for scanning: compact controls, stable dimensions, restrained color, token-based backgrounds, and no decorative gradients or marketing-style hero blocks inside the app shell.

#### Scenario: Toolbar dimensions stay stable
- **WHEN** filters, badges, or counts update in a toolbar
- **THEN** the toolbar height does not change
- **AND** neighboring controls do not shift horizontally unless the viewport breakpoint changes

#### Scenario: Cards are not nested
- **WHEN** a page uses cards for repeated items or summaries
- **THEN** cards are not placed inside other cards
- **AND** page sections use bands or unframed layouts rather than floating nested card stacks

### Requirement: Responsive And Accessibility Baseline

The design system SHALL define responsive behavior for 375px, 768px, 1024px, and 1440px widths. Navigation, settings, onboarding, lists, and basic query pages SHALL remain usable below desktop width; dense visualization workspaces MAY require horizontal workspace affordances but MUST not break the shell.

#### Scenario: Mobile navigation is reachable
- **WHEN** the viewport width is 375px
- **THEN** global navigation is reachable through an accessible menu trigger
- **AND** the org switcher, command palette, user menu, and settings menu remain keyboard accessible

#### Scenario: Axe critical violations are blocked
- **WHEN** Playwright runs the authenticated route a11y suite
- **THEN** every migrated template route reports zero axe critical violations
