## ADDED Requirements

### Requirement: Org Listing And Current Org

The web app SHALL maintain a `useOrgStore` zustand store exposing `{ orgs: Org[], currentOrgId: string | null, loadOrgs(), switchOrg(id) }`. The store SHALL load `orgs` from `GET /api/v1/orgs` on app boot when the user is authenticated; `currentOrgId` SHALL stay in sync with `useAuthStore.ctx.org_id`.

#### Scenario: Boot loads org list

- **WHEN** the authenticated shell mounts
- **THEN** `useOrgStore.loadOrgs()` issues `GET /api/v1/orgs` once
- **AND** the store populates `orgs` with the returned `{ id, name, role }` items
- **AND** `currentOrgId` resolves to the JWT's `org_id` claim

#### Scenario: Unauthenticated does not fetch

- **WHEN** the user is on `/login`
- **THEN** `useOrgStore` does NOT issue any `/api/v1/orgs` request
- **AND** `orgs` stays empty

### Requirement: Org Switcher UI

The StatusStrip SHALL render the current org name as a `DropdownMenu` trigger; opening it SHALL list every org from `useOrgStore.orgs` with the current org highlighted. Selecting a different org SHALL call `useOrgStore.switchOrg(id)`.

#### Scenario: Dropdown lists all available orgs

- **WHEN** the user clicks the org name in the StatusStrip
- **THEN** a dropdown opens with one item per `useOrgStore.orgs` entry
- **AND** the current org row carries a `data-current="true"` attribute
- **AND** pressing `Esc` closes the dropdown

#### Scenario: Switching org clears cache and resets stack

- **WHEN** the user selects an org id different from `currentOrgId`
- **THEN** the client issues `POST /api/v1/orgs/{id}/select`
- **AND** on success, `useAuthStore.setSession` is called with the returned token
- **AND** `queryClient.clear()` is called to drop org-scoped cached data
- **AND** `useInvestigationStack.reset()` is called to drop any open drawers
- **AND** the router navigates to `/home`

#### Scenario: Switching org failure rolls back

- **WHEN** `POST /api/v1/orgs/{id}/select` returns 4xx or 5xx
- **THEN** `useAuthStore` / `currentOrgId` remain unchanged
- **AND** a toast `Could not switch org: <message>` shows
- **AND** the dropdown closes
