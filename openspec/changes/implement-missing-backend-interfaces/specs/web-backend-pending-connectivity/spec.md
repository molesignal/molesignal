## MODIFIED Requirements

### Requirement: Backend Pending Reserved For Missing APIs
Frontend pages SHALL render backend-pending states only when the backend endpoint or data source required by the page is genuinely unavailable.

#### Scenario: Newly implemented backend interface replaces pending UI
- **WHEN** a backend-pending page now has a matching API endpoint
- **THEN** the page calls the endpoint and renders data, loading, empty, and error states
- **AND** it does not display backend-pending copy for that endpoint
