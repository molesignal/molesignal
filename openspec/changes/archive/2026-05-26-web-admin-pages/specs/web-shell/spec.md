## MODIFIED Requirements

### Requirement: Route Map

The router SHALL register the following authenticated route segments, each lazy-loaded: `/home`, `/investigate`, `/logs`, `/metrics`, `/traces`, `/dashboards`, `/dashboards/:id`, `/dashboards/:id/edit`, `/alerts`, `/alerts/rules`, `/alerts/incidents`, `/alerts/incidents/:id`, `/streams`, `/pipelines`, `/reports`, `/saved-views`, `/services`, `/services/:service`, `/settings`, `/settings/*`. **The `/settings/*` subtree SHALL render a Settings layout with a left sub-nav (Profile / Teams / Groups / License / SSO / API Tokens / Audit Log) and child routes mapped: `/settings/teams` → Teams page, `/settings/groups` → Groups page, `/settings/license` → License page; remaining sub-routes (profile, sso, api-tokens, audit) SHALL render `PagePlaceholder` until their feature change lands.**

#### Scenario: Settings sub-nav is visible

- **WHEN** the user navigates to `/settings` (no sub-route)
- **THEN** the sub-nav is visible with 7 items and the route auto-redirects to `/settings/profile`

#### Scenario: Teams / Groups / License render real pages

- **WHEN** the user clicks `Teams` in the Settings sub-nav
- **THEN** the URL becomes `/settings/teams`
- **AND** the Teams management page renders (per `web-admin-teams` spec)

#### Scenario: Admin gating

- **WHEN** a user with `Viewer` role opens `/settings/teams`
- **THEN** the page renders a `Need admin permission` panel
- **AND** no `GET /api/v1/identity/teams` request is sent (route guard short-circuits before fetch)
