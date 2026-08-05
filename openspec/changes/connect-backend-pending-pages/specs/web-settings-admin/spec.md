## ADDED Requirements

### Requirement: Implemented Settings Endpoints Render Live Empty States
Settings pages backed by implemented endpoints SHALL use live API data and normal empty states.

#### Scenario: Notify templates endpoint is empty
- **WHEN** `GET /api/v1/notify/templates` returns an empty list
- **THEN** `/settings/notify/templates` renders a normal empty template state and keeps the create action available

#### Scenario: Regex patterns endpoint is empty
- **WHEN** `GET /api/v1/regex_patterns` returns an empty list
- **THEN** `/settings/regex_patterns` renders a normal empty pattern state and keeps the create action available

#### Scenario: Model pricing endpoint is empty
- **WHEN** `GET /api/v1/model_prices` returns an empty list
- **THEN** `/settings/model_pricing` renders a normal empty pricing state and keeps the upsert action available

#### Scenario: License endpoint loads
- **WHEN** `GET /api/v1/license` returns a snapshot
- **THEN** `/settings/license` renders the license fields without backend-pending copy

### Requirement: Query Management Uses Runtime Registry
The Query Management settings page SHALL consume the live query runtime-control endpoints.

#### Scenario: No running queries
- **WHEN** `GET /api/v1/query/running` returns an empty list
- **THEN** `/settings/query_management` renders a normal "no running queries" state
- **AND** it continues polling for new entries
