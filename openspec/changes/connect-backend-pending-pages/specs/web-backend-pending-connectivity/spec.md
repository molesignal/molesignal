## ADDED Requirements

### Requirement: Backend Pending Reserved For Missing APIs
Frontend pages SHALL render backend-pending states only when the backend endpoint or data source required by the page is genuinely unavailable.

#### Scenario: Implemented endpoint returns empty rows
- **WHEN** a page calls an implemented endpoint and receives an empty list
- **THEN** the UI renders a normal empty state with feature-specific next actions
- **AND** it does not claim that the backend endpoint is pending

#### Scenario: Implemented action endpoint is available
- **WHEN** a UI action maps to an implemented backend endpoint
- **THEN** the action calls that endpoint and reports success or API error
- **AND** it does not use backend-pending copy as a placeholder result

#### Scenario: Missing endpoint remains explicit
- **WHEN** a page depends on an endpoint that is not implemented
- **THEN** the UI renders backend-pending copy that names or describes the missing backend surface

### Requirement: API Errors Are Not Hidden As Pending Work
Frontend pages SHALL distinguish live API errors, permission/license errors, empty results, and missing-backend states.

#### Scenario: Endpoint returns forbidden
- **WHEN** an implemented endpoint returns 403
- **THEN** the UI renders a permission or edition-gated state instead of backend-pending
