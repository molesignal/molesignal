## ADDED Requirements

### Requirement: Query Runtime UI Consumes Registry Endpoints
The web UI SHALL expose the running query registry through Settings Query Management.

#### Scenario: Running query appears in settings
- **WHEN** `GET /api/v1/query/running` returns one or more entries
- **THEN** `/settings/query_management` renders id, user, start time, statement, and a cancel action for each entry

#### Scenario: Cancel action refetches rows
- **WHEN** a user cancels a running query from the table
- **THEN** the UI POSTs `/api/v1/query/{id}/cancel`
- **AND** refetches the running-query list after success
