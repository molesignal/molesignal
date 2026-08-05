## ADDED Requirements

### Requirement: Capability-Driven Route Registry

The web app SHALL maintain a static route registry whose entries declare required permissions, permission mode, required product features, allowed auth scopes, and whether the route is organization-scoped. The backend capability snapshot SHALL decide which registered routes are available; the backend MUST NOT return arbitrary component names or paths.

#### Scenario: Route becomes visible after capability load
- **WHEN** the active snapshot contains every permission and feature required by a registered route
- **THEN** that route is available to navigation, command palette, keyboard navigation, and direct URL access

#### Scenario: Denied route never mounts
- **WHEN** a user directly opens a registered route without its required permission
- **THEN** the route guard redirects to an accessible fallback
- **AND** the denied route component does not mount or issue its data request

### Requirement: Navigation And Routes Share One Access Function

Sidebar navigation, pinned/recent destinations, Settings navigation, IAM navigation, command palette results, route guards, and permission-aware controls SHALL call the same `canAccessProductRoute`/`canAccessProductPath` calculation over the current capability snapshot.

#### Scenario: Organization switch removes stale menu
- **WHEN** a user switches from an organization with `iam.roles.manage` to one without it
- **THEN** the old snapshot is cleared before the new session is installed
- **AND** IAM role navigation and direct access are unavailable in the new organization

### Requirement: Capability Bootstrap Does Not Flash Denied Content

For authenticated online sessions, the shell SHALL fetch `/api/v1/iam/capabilities` before mounting a protected leaf route. While the initial snapshot is pending, the shell SHALL render a neutral loading state rather than assuming access from the JWT role.

#### Scenario: Reload protected deep link
- **WHEN** a user reloads a protected deep link
- **THEN** the leaf component is not mounted until its capability decision is known
- **AND** an unauthorized user never sees the protected page flash
