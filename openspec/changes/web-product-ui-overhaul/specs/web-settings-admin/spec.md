## ADDED Requirements

### Requirement: Settings Product Template

Every Settings sub-page SHALL use the shared SettingsPage template with consistent section heading, description, permission context, primary action, loading/error/empty state, and localized help copy.

#### Scenario: Settings section has consistent heading
- **WHEN** the user opens any `/settings/*` route
- **THEN** the page renders a localized heading, one-sentence purpose, and permission or backend status when relevant

#### Scenario: Settings route handles missing backend
- **WHEN** a settings route depends on an unavailable endpoint
- **THEN** it renders the shared backend-pending state
- **AND** it names the planned endpoint or capability in localized copy

### Requirement: Account And Edition Settings

Settings SHALL expose account/deployment surfaces appropriate to the active edition: license for self-hosted, trial/usage/billing for SaaS when metadata is available, and  admin features when licensed.

#### Scenario: Self-hosted user sees license
- **WHEN** the active deployment is self-hosted
- **THEN** Settings exposes License and deployment-related admin routes
- **AND** billing routes are hidden from primary settings navigation

#### Scenario: SaaS user sees usage
- **WHEN** the active deployment is SaaS and usage metadata exists
- **THEN** Settings exposes account usage and upgrade/billing actions

### Requirement: Admin IA Consistency

IAM and Settings SHALL avoid duplicate or contradictory org/account surfaces. Organization membership, org switching, license, storage, domains, security, and query management SHALL have one primary owner route and cross-link where necessary.

#### Scenario: Organization surfaces cross-link
- **WHEN** the user opens IAM Organizations or Settings Organization Management
- **THEN** the page explains whether it is for membership, org metadata, or multi-org administration
- **AND** related org routes are linked as secondary actions
