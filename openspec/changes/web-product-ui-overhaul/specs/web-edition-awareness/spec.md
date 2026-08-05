## ADDED Requirements

### Requirement: Edition Metadata Model

The frontend SHALL maintain a normalized edition metadata model describing the active deployment mode, license features, SaaS trial state, role permissions, and backend-pending features.

#### Scenario: Metadata loads once per session
- **WHEN** the authenticated shell mounts
- **THEN** the frontend loads edition/license metadata once per org session when an endpoint is available
- **AND** missing metadata falls back to OSS-safe defaults

#### Scenario: Metadata is queryable by UI
- **WHEN** a route or command needs to know if a feature is available
- **THEN** it reads the normalized edition metadata instead of duplicating license or role checks locally

### Requirement: Feature Gates

Unavailable features SHALL render through a shared FeatureGate or GatePage component that distinguishes -required, SaaS-only, trial-available, permission-denied, and backend-pending states.

#### Scenario:  feature in OSS
- **WHEN** an OSS user opens an -only feature route
- **THEN** the page explains what the feature does and that an  license is required
- **AND** it offers a non-blocking next action such as read docs, configure license, or contact sales
- **AND** it does not display a raw 403 error

#### Scenario: SaaS-only feature self-hosted
- **WHEN** a self-hosted user opens a SaaS-only billing or usage route
- **THEN** the page explains that the feature is managed by MoleSignal Cloud
- **AND** it provides a path back to license or organization settings

### Requirement: OSS-Friendly Upsell Boundaries

Open-source workflows SHALL remain usable without persistent upsell banners.  or SaaS prompts SHALL appear only on gated routes, gated actions, or contextual admin/account surfaces.

#### Scenario: Core observe page has no upsell banner
- **WHEN** an OSS user opens Logs, Metrics, Traces, Streams, Dashboards, or Ingest
- **THEN** the page does not show persistent sales banners
- **AND** unavailable  actions are either hidden or shown as clearly gated secondary actions

#### Scenario: Gated action explains alternative
- **WHEN** a user clicks an unavailable gated action from a list or toolbar
- **THEN** the UI explains the gate
- **AND** it offers an OSS-compatible alternative when one exists

### Requirement: SaaS Account Surfaces

When SaaS metadata is available, the web app SHALL expose account surfaces for trial status, usage, billing, support, and upgrade paths without changing the self-hosted OSS navigation defaults.

#### Scenario: SaaS trial status appears
- **WHEN** the active org is in a SaaS trial
- **THEN** the topbar or account menu shows trial status
- **AND** the account/settings route includes usage and upgrade actions

#### Scenario: Self-hosted hides billing by default
- **WHEN** the active deployment is self-hosted OSS or 
- **THEN** SaaS billing routes are not shown in primary navigation
- **AND** direct deep links render a SaaS-only GatePage
