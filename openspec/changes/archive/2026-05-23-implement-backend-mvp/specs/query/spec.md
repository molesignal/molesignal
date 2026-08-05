## ADDED Requirements

### Requirement: SQL Query Execution

The system SHALL accept SQL queries via `POST /api/v1/query` with body `QueryRequest { language: "sql", statement, time_range, stream?, limit? }`, plan them with DataFusion against the union of `ParquetFileMeta` files within `time_range`, execute them, and return a `QueryResult { columns, rows, scanned_rows, took_ms }`.

#### Scenario: Simple aggregation completes
- **WHEN** the request is `SELECT count(*) FROM app WHERE _timestamp BETWEEN t0 AND t1`
- **THEN** the response contains one column, one row with the count, `scanned_rows` reflecting only the files actually scanned (after pruning), and `took_ms` measured from request entry to result serialization

#### Scenario: Query referencing missing stream
- **WHEN** the SQL references a table that does not exist for the org
- **THEN** the response is `400 Bad Request` with `{ "error": "stream not found: <name>" }`

#### Scenario: Limit clamps result size
- **WHEN** the request includes `limit: 1000` for a query that would otherwise return more
- **THEN** only the first 1000 rows are returned and `scanned_rows` is unchanged

### Requirement: PromQL Query Entry Point

The system SHALL accept `language: "promql"` requests and dispatch them to a `PromqlEngine` trait (initial implementation may return `unimplemented` for instant/range queries while exposing the route shape).

#### Scenario: PromQL request reaches the engine
- **WHEN** a `QueryRequest { language: "promql", ... }` is posted
- **THEN** the system dispatches to `PromqlEngine::execute` rather than `DataFusionEngine`

### Requirement: Distributed Querier via Arrow Flight

When more than one querier node is registered, the querier handling the request SHALL split the candidate `ParquetFileMeta` list by file, dispatch sub-plans to peer queriers via Arrow Flight, and merge the resulting `RecordBatch` streams locally.

#### Scenario: Two-node fan-out
- **WHEN** two queriers are registered and 100 files match the request
- **THEN** roughly half the files are scanned on each node and the requesting node merges both result streams before returning

#### Scenario: Single-node fallback
- **WHEN** only one querier is registered
- **THEN** the request is executed locally without invoking Arrow Flight

### Requirement: Query Authorization

The query handler SHALL require `Permission::StreamRead` on the target stream(s); cross-org access is rejected.

#### Scenario: Cross-org query rejected
- **WHEN** the caller is a member of org A but the SQL references a stream owned by org B
- **THEN** the response is `403 Forbidden` and the query is not planned
