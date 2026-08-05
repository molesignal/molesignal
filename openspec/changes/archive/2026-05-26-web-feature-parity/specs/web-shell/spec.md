## ADDED Requirements

### Requirement: Sidebar Extended Nav

The Sidebar SHALL surface three new top-level entries beyond the existing 11: **RUM** (under OBSERVE), **Functions** (under DATA), **IAM** (under ADMIN). Each new entry uses a distinct Lucide icon and links to its module's default landing route (`/rum/sessions`, `/functions`, `/iam/users` respectively).

#### Scenario: New entries appear in collapsed and expanded states

- **WHEN** the user opens the app
- **THEN** the Sidebar shows 14 entries grouped into OVERVIEW (1), INGEST (1), OBSERVE (6), DATA (4), ADMIN (2)
- **AND** in collapsed state every entry's icon is keyboard-reachable via `Tab`
- **AND** the active route's left rail rail tick is rendered

### Requirement: Sitemap Coverage Audit

The repo SHALL include `docs/web/sitemap-diff.md` enumerating every openobserve route, our current molesignal equivalent, and the gap status (P0 / P1 / P2 / done). This file is updated when this change applies and when each follow-up (`web-feature-parity-settings`, `web-feature-parity-misc`) lands.

#### Scenario: Sitemap diff includes every route

- **WHEN** a contributor adds a new top-level route
- **THEN** `docs/web/sitemap-diff.md` lists it under the right section
- **AND** the CI lints the markdown for "TODO" entries that have outlived a release
