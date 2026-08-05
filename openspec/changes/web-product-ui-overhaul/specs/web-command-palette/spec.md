## MODIFIED Requirements

### Requirement: Command Source Coverage

The static action registry SHALL include commands to navigate to every top-level route in the product IA, toggle global time presets, open key creation flows, trigger SQL / PromQL editors, pin or unpin the current cursor as a time anchor, copy the current investigation URL, switch organizations, open keyboard help, open onboarding tasks, open support/account actions when available, and sign out. Commands SHALL be filtered by route context, role, and edition metadata.

#### Scenario: Time preset action
- **WHEN** the user opens the palette and selects `Time: last 1 hour`
- **THEN** the global time window store updates to `from: now - 1h, to: now`
- **AND** all subscribed views re-query within 50ms

#### Scenario: Copy investigation link
- **WHEN** the user selects `Copy investigation link`
- **THEN** the palette writes the current location, including `?time`, `?anchor`, and `?stack`, to clipboard
- **AND** shows a localized `Link copied` toast

#### Scenario: Edition-gated command is explained
- **WHEN** a user searches for an -only action while running OSS
- **THEN** the result either does not appear or appears with a gated badge
- **AND** selecting it opens the relevant FeatureGate instead of failing with a raw 403

## ADDED Requirements

### Requirement: Contextual Product Actions

The command palette SHALL rank and expose actions relevant to the current route, selected resource, active org, and onboarding state.

#### Scenario: Empty org ranks ingest actions first
- **WHEN** the active org has no streams and the user opens the palette with an empty query
- **THEN** ingest and sample-data actions appear before advanced admin actions

#### Scenario: Detail route exposes resource actions
- **WHEN** the user opens the palette from a dashboard detail page
- **THEN** dashboard-specific actions such as Edit dashboard, Add panel, Copy dashboard link, and Export dashboard appear when permitted

### Requirement: Support And Account Commands

The command palette SHALL include support, docs, license, trial, billing, and account commands when the active edition metadata exposes those surfaces.

#### Scenario: SaaS org sees billing command
- **WHEN** the active org is SaaS and billing metadata is available
- **THEN** searching `billing` returns an account or billing command

#### Scenario: Self-hosted org sees license command
- **WHEN** the active org is self-hosted
- **THEN** searching `license` returns the License settings route
