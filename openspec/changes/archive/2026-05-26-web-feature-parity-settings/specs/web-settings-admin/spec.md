## ADDED Requirements

### Requirement: Settings Layout And Sub-Navigation

The web app SHALL expose `/settings` as a layout route that renders an internal SettingsSidebar plus an `<Outlet />` for sub-pages. The SettingsSidebar SHALL list 16 sections in five semantic groups (ACCOUNT / DATA PLANE / ALERTS / SECURITY / ML OPS). Navigating to `/settings` SHALL redirect to `/settings/general`.

#### Scenario: Direct deep link selects the right section

- **WHEN** the user opens `/settings/cipher_keys`
- **THEN** the SettingsLayout mounts the CipherKeys sub-page
- **AND** the SettingsSidebar highlights "Cipher keys" under SECURITY
- **AND** the browser title reflects the section

#### Scenario: Bare /settings redirects to general

- **WHEN** the user opens `/settings` with no trailing segment
- **THEN** the router replaces the URL with `/settings/general`

### Requirement: General Profile Section

The page at `/settings/general` SHALL render the current user's profile (display name, email) plus per-user preferences (default home route, time format, keyboard shortcuts toggle). User data comes from `GET /api/v1/users/:id` for the authenticated user. Profile fields whose endpoint is missing SHALL render as read-only with an "Awaiting backend" badge.

#### Scenario: Profile loads for current user

- **WHEN** the user opens `/settings/general`
- **THEN** the page issues `GET /api/v1/users/<current-id>`
- **AND** renders display name + email
- **AND** preference toggles persist locally until a `/users/:id/preferences` endpoint lands

### Requirement: Organization Section

The page at `/settings/organization` SHALL render the current org's metadata (id, slug, plan, created_at). Data comes from the existing `/orgs/:id` endpoint via `useOrgStore`.

#### Scenario: Org metadata renders

- **WHEN** the user opens `/settings/organization`
- **THEN** the page renders the current org id, slug, and creation date sourced from `useOrgStore`
- **AND** offers the same org switcher affordance as the StatusStrip

### Requirement: License Section

The page at `/settings/license` SHALL display the active license (plan name, seats, expiry). Until a `GET /license` endpoint exists the page SHALL render an `EmptyState awaitingBackend`.

#### Scenario: License missing backend

- **WHEN** the user opens `/settings/license` and the backend has no `/license` endpoint
- **THEN** the page renders an "Awaiting backend" empty state
- **AND** the route remains reachable so URL navigation works

### Requirement: Alert Destinations Section

The page at `/settings/alert_destinations` SHALL list configured alert channels via `GET /api/v1/alerts/channels` and let admins create / delete channels.

#### Scenario: Channels list

- **WHEN** the user opens `/settings/alert_destinations`
- **THEN** the page issues `GET /api/v1/alerts/channels`
- **AND** renders one row per channel with name, kind (slack / webhook / pagerduty), and last_used

### Requirement: Alert Templates Section

The page at `/settings/alert_templates` SHALL list message templates used by alert channels. Until a `/alerts/templates` endpoint lands the page SHALL render `EmptyState awaitingBackend`.

#### Scenario: Templates list when backend present

- **WHEN** the `/alerts/templates` endpoint exists
- **THEN** the page issues `GET /api/v1/alerts/templates` and renders one row per template

#### Scenario: Templates list when backend missing

- **WHEN** the endpoint is missing
- **THEN** the page renders an "Awaiting backend" empty state

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

The playwright `a11y-routes.spec.ts` SHALL include all 16 new `/settings/*` paths. Each SHALL pass axe with `critical = 0` violations under the offline-dev mock backend.

#### Scenario: New routes pass axe

- **WHEN** the a11y-routes spec runs against the 16 new settings paths
- **THEN** every page completes axe analysis with zero critical violations
