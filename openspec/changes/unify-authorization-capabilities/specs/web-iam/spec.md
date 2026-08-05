## MODIFIED Requirements

### Requirement: Groups And Roles

The IAM role page SHALL render canonical permissions grouped by product domain, support named permission bundles with per-permission adjustments, generate an editable role key from the name before creation, and keep the key immutable afterward. The former raw policy page SHALL be replaced by a semantic access-grant workflow ordered as principal, organization, resource selector, permissions, and optional constraints; administrators MUST select known objects and permissions instead of typing subject ids or action/resource strings.

#### Scenario: Create grouped custom role
- **WHEN** an administrator selects the Pipeline Operator bundle, removes `pipelines.delete`, and creates the role
- **THEN** the request contains only registered atomic permission keys
- **AND** the role list renders the permissions grouped under the Pipeline domain

#### Scenario: Role key generated before creation
- **WHEN** an administrator types a role name and has not manually edited the key
- **THEN** the form derives a normalized immutable role key
- **AND** allows the administrator to change it before the create request

#### Scenario: Semantic grant avoids raw ids
- **WHEN** an administrator creates a resource grant
- **THEN** principal, resource, and permission values come from server-backed selectors
- **AND** no free-form action, subject id, resource type, or effect input is displayed

#### Scenario: Cross-organization sharing is separate
- **WHEN** an administrator wants to share a resource to another organization
- **THEN** the UI opens the dedicated cross-organization sharing workflow
- **AND** the ordinary access-grant form cannot accept another organization id
