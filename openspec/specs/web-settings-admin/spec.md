# Web Settings Admin Capability

## Purpose

Provides the `/settings` admin surface in the web app: a layout route with access-aware navigation grouped under ACCOUNT / NOTIFY / DATA PLANE / SECURITY / AI & ML. Current workspace metadata and editing live in General rather than a duplicate Organization page. Settings strings ship in a dedicated `settings-admin` i18n namespace (en + zh-CN), and discoverable settings pages are covered by the Playwright accessibility suite.

## Requirements

### Requirement: Settings Layout And Sub-Navigation

The web app SHALL expose `/settings` as a layout route that renders an internal SettingsSidebar plus an `<Outlet />` for sub-pages. The SettingsSidebar SHALL list accessible sections in five semantic groups (ACCOUNT / NOTIFY / DATA PLANE / SECURITY / AI & ML). Navigating to `/settings` SHALL redirect to `/settings/general`.

#### Scenario: Direct deep link selects the right section

- **WHEN** the user opens `/settings/cipher_keys`
- **THEN** the SettingsLayout mounts the CipherKeys sub-page
- **AND** the SettingsSidebar highlights "Cipher keys" under SECURITY
- **AND** the browser title reflects the section

#### Scenario: Bare /settings redirects to general

- **WHEN** the user opens `/settings` with no trailing segment
- **THEN** the router replaces the URL with `/settings/general`

### Requirement: General Workspace Section

The page at `/settings/general` SHALL render the current workspace name, stable slug and id, the current user's role, and workspace defaults. Workspace metadata comes from `GET /api/v1/me/profile`; users with `org.settings.manage` MAY update the workspace name.

#### Scenario: Workspace metadata loads

- **WHEN** the user opens `/settings/general`
- **THEN** the page issues `GET /api/v1/me/profile`
- **AND** renders the workspace name, slug, id, and current role
- **AND** the stable slug and id provide copy actions

#### Scenario: Legacy organization URL redirects

- **WHEN** the user opens the legacy `/settings/organization` URL
- **THEN** the router replaces it with `/settings/general`
- **AND** the SettingsSidebar does not expose a duplicate Organization entry

### Requirement: License Section

The page at `/settings/license` SHALL display the active license (plan name, seats, expiry). Until a `GET /license` endpoint exists the page SHALL render an `EmptyState awaitingBackend`.

#### Scenario: License missing backend

- **WHEN** the user opens `/settings/license` and the backend has no `/license` endpoint
- **THEN** the page renders an "Awaiting backend" empty state
- **AND** the route remains reachable so URL navigation works

### Requirement: Notify Settings Group

System Settings SHALL contain a Notify group with child pages for Connectors,
Users, Policies, Templates, Defaults, and Deliveries. Each page SHALL use the
same page header, content sections, navigation selection, and responsive
content shell as General.

#### Scenario: Connectors list

- **WHEN** the user opens `/settings/notify/connectors`
- **THEN** the Settings sidebar highlights Connectors under Notify
- **AND** the page issues `GET /api/v1/notify/connectors`
- **AND** renders one row per connector

#### Scenario: Notify templates list

- **WHEN** the user opens `/settings/notify/templates`
- **THEN** the page issues `GET /api/v1/notify/templates`
- **AND** renders one row per Notify template

#### Scenario: Unshipped alert notify URLs are absent

- **WHEN** the user opens `/alerts/notify/connectors`, `/alerts/channels`, or
  `/alerts/templates`
- **THEN** the router renders the not-found page
- **AND** no compatibility redirect is registered

### Requirement: Pipeline Destinations Section

The page at `/settings/pipeline_destinations` SHALL list configured external sinks via `GET /api/v1/connectors` and let admins create / delete connectors.

#### Scenario: Connector CRUD

- **WHEN** the user creates a connector via the drawer
- **THEN** the page POSTs to `/api/v1/connectors`
- **AND** the new row appears in the list after refetch

### Requirement: Cipher Keys Section

The page at `/settings/cipher_keys` SHALL list registered cipher keys via `GET /api/v1/cipher_keys` and let admins create, rotate, and delete keys.

#### Scenario: Rotate key

- **WHEN** the user clicks Rotate on a key row
- **THEN** the page POSTs to `/api/v1/cipher_keys/<name>/rotate`
- **AND** the row's "rotated at" timestamp updates after the response

### Requirement: Regex Patterns Section

The page at `/settings/regex_patterns` SHALL host VRL regex pattern shortcuts. Until a backend endpoint exists the page SHALL render `EmptyState awaitingBackend`.

#### Scenario: Empty backend

- **WHEN** the endpoint is missing
- **THEN** the page renders an "Awaiting backend" empty state with the planned `/regex_patterns` endpoint name

### Requirement: AI Toolsets Section

The page at `/settings/ai_toolsets` SHALL host LLM tool definitions for the Copilot. Until a backend endpoint exists the page SHALL render `EmptyState awaitingBackend`.

#### Scenario: AI toolsets pending backend

- **WHEN** the user opens `/settings/ai_toolsets`
- **THEN** the page renders an "Awaiting backend" empty state

### Requirement: Model Pricing Section

The page at `/settings/model_pricing` SHALL render the LLM model pricing matrix. Until a backend endpoint exists the page SHALL render `EmptyState awaitingBackend`.

#### Scenario: Pricing pending backend

- **WHEN** the user opens `/settings/model_pricing`
- **THEN** the page renders an "Awaiting backend" empty state

### Requirement: Query Management Section

The page at `/settings/query_management` SHALL surface in-flight queries and offer cancel actions. Until a `/query/running` aggregator endpoint exists the page SHALL render `EmptyState awaitingBackend`. When the endpoint lands, the page SHALL list rows with user_id, started_at, sql snippet, and a Cancel button.

#### Scenario: Running queries — list

- **WHEN** the `/query/running` endpoint exists and the user opens `/settings/query_management`
- **THEN** the page issues `GET /api/v1/query/running`
- **AND** renders one row per in-flight query

#### Scenario: Cancel a query

- **WHEN** the user clicks Cancel on a running query
- **THEN** the page POSTs to `/api/v1/query/<id>/cancel`
- **AND** the row disappears after the response

### Requirement: Storage Settings Section

The page at `/settings/storage_settings` SHALL render configured storage providers via `GET /api/v1/clusters/storage_providers` and let admins create / upsert providers.

#### Scenario: Storage provider upsert

- **WHEN** the user submits the provider form
- **THEN** the page PUTs to `/api/v1/clusters/storage_providers/<id>`
- **AND** the row reflects the new config after refetch

### Requirement: Nodes Section

The page at `/settings/nodes` SHALL list cluster nodes via `GET /api/v1/clusters` showing id, role, and health.

#### Scenario: Nodes list

- **WHEN** the user opens `/settings/nodes`
- **THEN** the page issues `GET /api/v1/clusters`
- **AND** renders one row per node with role, address, and a green/red dot for health

### Requirement: Domain Management Section

The page at `/settings/domain_management` SHALL list TLS / custom domains via `GET /api/v1/domains` and let admins create new domains or trigger renewals.

#### Scenario: Renew certificate

- **WHEN** the user clicks Renew on a domain row
- **THEN** the page POSTs to `/api/v1/domains/<id>/renew`
- **AND** the row's status flips to "renewing" until the response returns

### Requirement: Correlation Section

The page at `/settings/correlation` SHALL render the correlation provider registry used by `/web/correlation/*`. Read-only initially.

#### Scenario: Correlation registry view

- **WHEN** the user opens `/settings/correlation`
- **THEN** the page renders the configured provider sources from `useCorrelationStore`
- **AND** non-admins see read-only badges

### Requirement: Organization Management Section

The page at `/settings/organization_management` SHALL list all organizations visible to the user via `GET /api/v1/orgs` and let owners create new orgs / manage memberships. This is the multi-tenant superset of the existing `/iam/organizations` page.

#### Scenario: Create new org

- **WHEN** an Owner clicks "Create organization" and submits the form
- **THEN** the page POSTs to `/api/v1/orgs`
- **AND** the new org appears in the list and is selectable in the StatusStrip switcher

### Requirement: Settings i18n Namespace

All Settings sub-page strings SHALL come from a new `settings-admin` i18n namespace (en + zh-CN). Section labels in the SettingsSidebar SHALL reuse `settings-admin:nav.*` keys; per-section strings live under `settings-admin:<section>.*`.

#### Scenario: Locale switch propagates

- **WHEN** the user toggles language from en to zh-CN
- **THEN** the SettingsSidebar labels and the active sub-page's body re-render in zh-CN without a reload

### Requirement: Accessibility Coverage

The Playwright `a11y-routes.spec.ts` SHALL include every discoverable `/settings/*` page. Redirect-only compatibility aliases SHALL NOT be scanned as separate pages. Each page SHALL pass axe with `critical = 0` violations under the mock backend.

#### Scenario: Settings routes pass axe

- **WHEN** the a11y-routes spec runs against the discoverable settings paths
- **THEN** every page completes axe analysis with zero critical violations
