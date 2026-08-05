## ADDED Requirements

### Requirement: Groups Management Page

The web app SHALL provide `/settings/groups` rendering a table of FGA groups within the current org, with create / edit / delete actions and a per-group policy binding drawer. The page SHALL call `GET /api/v1/fga/groups`, `POST /api/v1/fga/groups`, `PATCH /api/v1/fga/groups/{id}`, `DELETE /api/v1/fga/groups/{id}`, and `POST /api/v1/fga/groups/{id}/policies` to bind a policy id.

#### Scenario: Group row shows policy chips

- **WHEN** a group has 3 policies bound
- **THEN** the table row's `Policies` column renders 3 chips with each policy's short id
- **AND** hovering a chip shows a tooltip with the full policy name + description

#### Scenario: Bind policy drawer

- **WHEN** the user clicks `Manage policies` on a group row
- **THEN** a drawer opens listing all available policies with checkboxes
- **AND** toggling a checkbox issues `POST /api/v1/fga/groups/{id}/policies` (bind) or `DELETE /api/v1/fga/groups/{id}/policies/{pid}` (unbind)
- **AND** the row's policy-chip column updates without full table refetch

#### Scenario: Non-admin sees no-access state

- **WHEN** a user with role `Viewer` navigates to `/settings/groups`
- **THEN** the page renders a `Need admin permission` panel and no list is fetched
