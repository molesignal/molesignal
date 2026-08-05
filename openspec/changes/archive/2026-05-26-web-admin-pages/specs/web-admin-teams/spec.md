## ADDED Requirements

### Requirement: Teams Management Page

The web app SHALL provide `/settings/teams` rendering a table of teams within the current org, with create / edit / delete actions and a per-team member management drawer. The page SHALL call `GET /api/v1/identity/teams` (list), `POST /api/v1/identity/teams` (create), `PATCH /api/v1/identity/teams/{id}` (rename), `DELETE /api/v1/identity/teams/{id}` (delete), `GET /api/v1/identity/teams/{id}/members`, `POST /api/v1/identity/teams/{id}/members` (add), and `DELETE /api/v1/identity/teams/{id}/members/{user_id}` (remove).

#### Scenario: List renders team rows

- **WHEN** the user opens `/settings/teams` with at least one team in the org
- **THEN** the table shows columns: name, member count, created at, actions (edit, delete)
- **AND** clicking a row opens the member-management drawer

#### Scenario: Create team validates name

- **WHEN** the user clicks `New team`, enters an empty name, and submits
- **THEN** the form shows an inline error `Name is required`
- **AND** no `POST /api/v1/identity/teams` request is sent

#### Scenario: Destructive delete confirms

- **WHEN** the user clicks `Delete` on a team
- **THEN** a confirm dialog appears with title `Delete team <name>?` and confirm-label `Delete`
- **AND** only after confirm does `DELETE /api/v1/identity/teams/{id}` fire
- **AND** the row disappears optimistically; on 4xx the row reappears with a toast

#### Scenario: Non-admin sees no-access state

- **WHEN** a user with role `Viewer` navigates to `/settings/teams`
- **THEN** the page renders a `Need admin permission` panel and no table data is fetched
