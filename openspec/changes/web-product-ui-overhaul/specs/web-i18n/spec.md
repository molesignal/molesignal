## ADDED Requirements

### Requirement: Product Copy Namespaces

The i18n layer SHALL include namespaces for product IA, onboarding, design-system states, and edition gates. All new shell labels, page template strings, empty states, onboarding tasks, feature gates, and SaaS/ copy SHALL use these namespaces in English and zh-CN.

#### Scenario: Onboarding copy translates
- **WHEN** the user switches from English to zh-CN
- **THEN** onboarding checklist titles, descriptions, buttons, and status labels update without a reload

#### Scenario: Gate copy translates
- **WHEN** a license-gated page renders in zh-CN
- **THEN** the gate title, explanation, and actions render in zh-CN

### Requirement: No Raw Product Copy In Migrated Pages

Migrated product routes SHALL NOT introduce new hard-coded user-visible strings in JSX except for product names, telemetry field names, query language keywords, or user data.

#### Scenario: Static copy audit passes
- **WHEN** the i18n audit script runs on migrated route directories
- **THEN** it reports zero unapproved hard-coded user-visible strings

#### Scenario: Missing key fails targeted tests
- **WHEN** a page template references a missing i18n key
- **THEN** the targeted i18n test fails before the page is considered migrated
