## ADDED Requirements

### Requirement: Existing Domain Models Are Exposed Through HTTP
When a domain model and repository already exist, the backend SHALL expose create/update HTTP handlers instead of returning generic "not implemented" errors.

#### Scenario: Alert and Notify write endpoints accept typed requests
- **WHEN** a caller POSTs or PUTs alert rules, escalation policies, or Notify connectors
- **THEN** the backend validates the request, assigns server-controlled fields, persists it, and returns the saved resource

#### Scenario: Schedule writes update stored schedules
- **WHEN** a caller creates or updates a schedule or schedule override
- **THEN** the backend persists the schedule and returns the current schedule shape

### Requirement: Missing UI Data Surfaces Have Backing APIs
Backend routes SHALL exist for enrichment tables, invitations, correlation provider catalogs, report template catalogs, and function dry-run.

#### Scenario: Enrichment table CRUD
- **WHEN** a caller lists tables, lists rows for a table, upserts a key, or deletes a key
- **THEN** the backend reads or writes `enrichment_kv` and refreshes the in-memory enrichment table for that org/table

#### Scenario: Invitation lifecycle
- **WHEN** a caller creates, lists, resends, or revokes an invitation
- **THEN** the backend persists and returns invitation records scoped to the current organization

#### Scenario: Deterministic catalogs
- **WHEN** a caller requests report templates or correlation providers
- **THEN** the backend returns stable catalog entries without requiring frontend hardcoded placeholders

#### Scenario: Function dry-run
- **WHEN** a caller submits VRL source and a JSON sample to the dry-run endpoint
- **THEN** the backend compiles and executes the function against the sample and returns the transformed output
