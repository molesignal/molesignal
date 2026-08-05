## ADDED Requirements

### Requirement: IAM Top-Level Route

The web app SHALL expose `/iam` as a top-level navigation group with seven sub-routes: `users`, `service-accounts`, `organizations`, `groups`, `roles`, `quota`, `invitations`. Each sub-route is a standalone page; the Sidebar surfaces an "IAM" entry in the ADMIN group.

#### Scenario: IAM hub keyboard shortcut

- **WHEN** the user presses `g i` from anywhere in the app
- **THEN** the router navigates to `/iam/users` (default landing)

### Requirement: User Management

The page at `/iam/users` SHALL list every user in the current org by calling `GET /api/v1/users` and render rows with email, display name, role (joined from membership), last-active timestamp, and an action menu for disable / promote / remove.

#### Scenario: Invite by email

- **WHEN** an Admin clicks "Invite user" and submits an email
- **THEN** the page POSTs to `/api/v1/invitations` with `{ email, role }`
- **AND** on success the new entry appears in `/iam/invitations` as "pending"

### Requirement: Groups And Roles

The pages at `/iam/groups` and `/iam/roles` SHALL surface FGA groups + their policy bindings (from `/api/v1/rbac_policies`) and role definitions respectively. Group create/edit dialogs SHALL let the user pick member users and bound policies.

#### Scenario: Bind policy to group

- **WHEN** an Admin opens `/iam/groups/edit/<name>` and ticks a policy in the "Policies" list
- **THEN** the page PUTs to `/api/v1/rbac_policies/<id>` with the group added to its `groups` field
- **AND** the group's row shows the policy chip on the list page after refetch

### Requirement: Service Accounts

The page at `/iam/service-accounts` SHALL list non-human accounts that authenticate via `ms_*` API tokens. Each account row links to its tokens (filtered view of `/iam/api-tokens` or the existing Settings panel).

#### Scenario: Create service account with auto-token

- **WHEN** an Admin clicks "Create service account" and submits a name + scope
- **THEN** the page POSTs to `/api/v1/users` with `{ kind: 'service', name, scopes }`
- **AND** the response includes a one-time-display API token shown in a copy dialog
- **AND** the new account appears in the list with "0 tokens" rolling up to 1 after acknowledgement

### Requirement: Quota And License

The page at `/iam/quota` SHALL render the current org's quota usage (ingest bytes, query CPU-seconds, dashboards count, alert rules count) against the plan limit (from `/api/v1/license` or `/api/v1/quota`).

#### Scenario: Approaching quota shows banner

- **WHEN** ingest bytes >= 80% of plan limit
- **THEN** the page renders a yellow banner with current usage + reset date
- **AND** a "Upgrade plan" link routes to `/settings/license`

### Requirement: Invitations

The page at `/iam/invitations` SHALL list pending invites (email, inviter, sent_at, role) and let admins resend or revoke. Pulled from `GET /api/v1/invitations`.

#### Scenario: Revoke pending invite

- **WHEN** an Admin clicks "Revoke" on a pending invite
- **THEN** the page DELETEs `/api/v1/invitations/<id>`
- **AND** the row disappears after confirmation
