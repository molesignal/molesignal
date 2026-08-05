# Query Runtime Control Capability

## Purpose

In-process registry and HTTP surface for inspecting and cancelling in-flight queries. Enables operators to see what's running per-org and abort long-running statements at DataFusion record-batch boundaries.

## Requirements

### Requirement: Running Query Registry

`QueryService::execute_query` and `QueryService::execute_stream_query` SHALL register every in-flight query into an in-process `QueryRegistry` keyed by a server-generated `QueryId`. Registration carries `{ id, org_id, user_id, statement, started_at_micros, cancel: Arc<AtomicBool> }`. The query SHALL be removed from the registry when execution completes (success, error, or drop).

#### Scenario: Successful query registers and unregisters

- **WHEN** a `POST /api/v1/query` executes for 500ms and returns a result
- **THEN** the registry contains the entry while executing
- **AND** the entry is removed after the response is sent

#### Scenario: Errored query unregisters

- **WHEN** a query fails during planning
- **THEN** the registry entry is removed before the error returns to the caller

### Requirement: Running Query List Endpoint

The system SHALL expose `GET /api/v1/query/running` (OrgAdmin+) that returns a snapshot of registry entries scoped to the caller's `org_id`. Owners SHALL see entries across all orgs.

#### Scenario: Admin sees own org only

- **WHEN** an OrgAdmin in org `A` GETs `/api/v1/query/running` while queries are in flight across orgs `A` and `B`
- **THEN** the response only contains queries whose `org_id == A`

#### Scenario: Owner sees all orgs

- **WHEN** an Owner GETs `/api/v1/query/running`
- **THEN** the response includes queries from every org

### Requirement: Query Cancellation Endpoint

The system SHALL expose `POST /api/v1/query/{id}/cancel` (OrgAdmin+) that flips the registry entry's `cancel` flag. `QueryService::execute_query` SHALL check the flag at each DataFusion record-batch boundary and abort with `Error::cancelled("query cancelled")` when set.

#### Scenario: Cancel flips flag and aborts

- **WHEN** a long-running query is registered and an Admin POSTs to `/cancel`
- **THEN** the cancel flag becomes true
- **AND** the in-flight query returns `error.code = "cancelled"` to the original caller within one batch boundary

#### Scenario: Cancel on unknown id

- **WHEN** an Admin POSTs `/api/v1/query/{unknown}/cancel`
- **THEN** the response is `404 Not Found` with `{ "error": "query not found" }`

#### Scenario: Cross-org cancel denied for non-Owner

- **WHEN** an OrgAdmin in org `A` POSTs cancel for a `QueryId` belonging to org `B`
- **THEN** the response is `403 Forbidden`
